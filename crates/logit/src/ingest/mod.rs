use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use rusqlite::params;
use serde::Serialize;
use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::models::{AgentLogEvent, AgentSource};
use crate::sqlite::{
    INGEST_RUNS_TABLE, INGEST_WATERMARKS_TABLE, SqliteWriterConfig, open_sqlite_connection,
    write_events_batched,
};

pub const INGEST_REPORT_SCHEMA_VERSION: &str = "logit.ingest-report.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestRunStatus {
    Success,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestRefreshPlan {
    pub events_jsonl_path: PathBuf,
    pub sqlite_path: PathBuf,
    pub source_root: PathBuf,
    pub fail_fast: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IngestRefreshReport {
    pub ingest_run_id: String,
    pub source_root: String,
    pub status: IngestRunStatus,
    pub started_at_utc: String,
    pub finished_at_utc: String,
    pub duration_ms: u64,
    pub events_read: usize,
    pub events_written: usize,
    pub events_skipped: usize,
    pub warnings_count: usize,
    pub errors_count: usize,
    pub watermarks_upserted: usize,
    pub watermark_staleness_state: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IngestReportCounts {
    pub read: usize,
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IngestReportWatermarkStatus {
    pub sources_upserted: usize,
    pub staleness_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IngestReportArtifact {
    pub schema_version: String,
    pub ingest_run_id: String,
    pub source_root: String,
    pub status: IngestRunStatus,
    pub started_at_utc: String,
    pub finished_at_utc: String,
    pub duration_ms: u64,
    pub counts: IngestReportCounts,
    pub warnings: Vec<String>,
    pub watermarks: IngestReportWatermarkStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WatermarkUpsertOutcome {
    sources_upserted: usize,
    staleness_state: String,
}

pub fn run_refresh(plan: &IngestRefreshPlan) -> Result<IngestRefreshReport> {
    let started_at_utc = now_utc_rfc3339()?;
    let started_at = std::time::Instant::now();
    let ingest_run_id = build_ingest_run_id();
    let source_root = plan.source_root.to_string_lossy().to_string();

    let input = std::fs::read_to_string(&plan.events_jsonl_path).with_context(|| {
        format!(
            "failed to read normalized events file: {}",
            plan.events_jsonl_path.display()
        )
    })?;
    let (events, warnings) = parse_events_jsonl(&input, plan.fail_fast)?;

    let mut connection = open_sqlite_connection(&plan.sqlite_path)?;
    crate::sqlite::ensure_sqlite_schema(&connection)?;
    insert_ingest_run_started(
        &connection,
        &ingest_run_id,
        &started_at_utc,
        &source_root,
        events.len(),
        warnings.len(),
    )?;

    let write_stats =
        match write_events_batched(&mut connection, &events, SqliteWriterConfig::default()) {
            Ok(write_stats) => write_stats,
            Err(error) => {
                let finished_at_utc = now_utc_rfc3339()?;
                let error_summary = json!({ "message": format!("{error:#}") }).to_string();
                let _ = finalize_ingest_run(
                    &connection,
                    &ingest_run_id,
                    IngestRunStatus::Failed,
                    &finished_at_utc,
                    events.len(),
                    0,
                    warnings.len(),
                    1,
                    &error_summary,
                );
                return Err(error).context("failed to write ingested rows to sqlite mart");
            }
        };

    let finished_at_utc = now_utc_rfc3339()?;
    let watermark_outcome =
        upsert_source_watermarks(&connection, &ingest_run_id, &finished_at_utc, &events)?;
    finalize_ingest_run(
        &connection,
        &ingest_run_id,
        IngestRunStatus::Success,
        &finished_at_utc,
        events.len(),
        write_stats.records_written,
        warnings.len(),
        0,
        "{}",
    )?;

    Ok(IngestRefreshReport {
        ingest_run_id,
        source_root,
        status: IngestRunStatus::Success,
        started_at_utc,
        finished_at_utc,
        duration_ms: started_at.elapsed().as_millis() as u64,
        events_read: events.len(),
        events_written: write_stats.records_written,
        events_skipped: warnings.len(),
        warnings_count: warnings.len(),
        errors_count: 0,
        watermarks_upserted: watermark_outcome.sources_upserted,
        watermark_staleness_state: watermark_outcome.staleness_state,
        warnings,
    })
}

fn insert_ingest_run_started(
    connection: &rusqlite::Connection,
    ingest_run_id: &str,
    started_at_utc: &str,
    source_root: &str,
    events_read: usize,
    warnings_count: usize,
) -> Result<()> {
    connection
        .execute(
            &format!(
                "INSERT INTO {INGEST_RUNS_TABLE} (ingest_run_id, started_at_utc, status, source_root, events_read, events_written, warnings_count, errors_count, error_summary_json)
                 VALUES (?1, ?2, 'running', ?3, ?4, 0, ?5, 0, '{{}}')"
            ),
            params![
                ingest_run_id,
                started_at_utc,
                source_root,
                to_i64(events_read, "events_read")?,
                to_i64(warnings_count, "warnings_count")?
            ],
        )
        .with_context(|| format!("failed to insert ingest run start row: {ingest_run_id}"))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finalize_ingest_run(
    connection: &rusqlite::Connection,
    ingest_run_id: &str,
    status: IngestRunStatus,
    finished_at_utc: &str,
    events_read: usize,
    events_written: usize,
    warnings_count: usize,
    errors_count: usize,
    error_summary_json: &str,
) -> Result<()> {
    connection
        .execute(
            &format!(
                "UPDATE {INGEST_RUNS_TABLE}
                 SET finished_at_utc = ?2,
                     status = ?3,
                     events_read = ?4,
                     events_written = ?5,
                     warnings_count = ?6,
                     errors_count = ?7,
                     error_summary_json = ?8
                 WHERE ingest_run_id = ?1"
            ),
            params![
                ingest_run_id,
                finished_at_utc,
                ingest_run_status_key(status),
                to_i64(events_read, "events_read")?,
                to_i64(events_written, "events_written")?,
                to_i64(warnings_count, "warnings_count")?,
                to_i64(errors_count, "errors_count")?,
                error_summary_json,
            ],
        )
        .with_context(|| format!("failed to finalize ingest run row: {ingest_run_id}"))?;
    Ok(())
}

fn upsert_source_watermarks(
    connection: &rusqlite::Connection,
    ingest_run_id: &str,
    refreshed_at_utc: &str,
    events: &[AgentLogEvent],
) -> Result<WatermarkUpsertOutcome> {
    let existing_by_source = load_existing_source_watermarks(connection)?;
    let mut watermark_by_source = BTreeMap::<String, SourceWatermarkState>::new();
    for event in events {
        let source_key = format!(
            "{}|{}",
            source_kind_key(event.source_kind),
            event.source_path
        );
        let candidate = SourceWatermarkState {
            source_kind: source_kind_key(event.source_kind).to_string(),
            source_path: event.source_path.clone(),
            source_record_locator: event.source_record_locator.clone(),
            source_record_hash: event.source_record_hash.clone(),
            last_event_timestamp_unix_ms: event.timestamp_unix_ms as i64,
        };
        if let Some(current) = watermark_by_source.get_mut(&source_key) {
            if candidate.last_event_timestamp_unix_ms >= current.last_event_timestamp_unix_ms {
                *current = candidate;
            }
        } else {
            watermark_by_source.insert(source_key, candidate);
        }
    }

    let upsert_sql = format!(
        "INSERT INTO {INGEST_WATERMARKS_TABLE}
             (source_key, source_kind, source_path, source_record_locator, source_record_hash, last_event_timestamp_unix_ms, last_ingest_run_id, refreshed_at_utc, staleness_state, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(source_key) DO UPDATE SET
             source_kind = excluded.source_kind,
             source_path = excluded.source_path,
             source_record_locator = excluded.source_record_locator,
             source_record_hash = excluded.source_record_hash,
             last_event_timestamp_unix_ms = excluded.last_event_timestamp_unix_ms,
             last_ingest_run_id = excluded.last_ingest_run_id,
             refreshed_at_utc = excluded.refreshed_at_utc,
             staleness_state = excluded.staleness_state,
             metadata_json = excluded.metadata_json"
    );

    let mut observed_sources = BTreeSet::<String>::new();
    for (source_key, watermark) in &watermark_by_source {
        let (decision, decision_reason, pre_refresh_staleness_state) =
            derive_incremental_decision(existing_by_source.get(source_key), watermark);
        let metadata_json = json!({
            "observed_in_refresh": true,
            "incremental_decision": incremental_decision_key(decision),
            "decision_reason": decision_reason,
            "pre_refresh_staleness_state": pre_refresh_staleness_state,
        })
        .to_string();
        connection.execute(
            &upsert_sql,
            params![
                source_key,
                watermark.source_kind,
                watermark.source_path,
                watermark.source_record_locator,
                watermark.source_record_hash,
                watermark.last_event_timestamp_unix_ms,
                ingest_run_id,
                refreshed_at_utc,
                "fresh",
                metadata_json,
            ],
        )?;
        observed_sources.insert(source_key.clone());
    }

    let mark_stale_sql = format!(
        "UPDATE {INGEST_WATERMARKS_TABLE}
         SET staleness_state = 'stale',
             metadata_json = ?2
         WHERE source_key = ?1"
    );
    let mut stale_source_count = 0usize;
    for source_key in existing_by_source.keys() {
        if observed_sources.contains(source_key) {
            continue;
        }
        let metadata_json = json!({
            "observed_in_refresh": false,
            "incremental_decision": "process",
            "decision_reason": "missing_in_latest_refresh",
            "pre_refresh_staleness_state": "stale",
        })
        .to_string();
        connection.execute(&mark_stale_sql, params![source_key, metadata_json])?;
        stale_source_count += 1;
    }

    let staleness_state = if watermark_by_source.is_empty() && existing_by_source.is_empty() {
        "unknown".to_string()
    } else if stale_source_count > 0 {
        "stale".to_string()
    } else {
        "fresh".to_string()
    };

    Ok(WatermarkUpsertOutcome {
        sources_upserted: watermark_by_source.len(),
        staleness_state,
    })
}

fn parse_events_jsonl(input: &str, fail_fast: bool) -> Result<(Vec<AgentLogEvent>, Vec<String>)> {
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<AgentLogEvent>(trimmed) {
            Ok(event) => events.push(event),
            Err(error) if fail_fast => {
                return Err(anyhow!(
                    "invalid events jsonl row at line {line_number}: {error}"
                ));
            }
            Err(error) => warnings.push(format!(
                "invalid events jsonl row at line {line_number}: {error}"
            )),
        }
    }

    Ok((events, warnings))
}

fn now_utc_rfc3339() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format ingest timestamp as RFC3339")
}

fn build_ingest_run_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64);
    format!("ingest-{nanos:016x}")
}

const fn source_kind_key(source: AgentSource) -> &'static str {
    match source {
        AgentSource::Codex => "codex",
        AgentSource::Claude => "claude",
        AgentSource::Gemini => "gemini",
        AgentSource::Amp => "amp",
        AgentSource::OpenCode => "opencode",
    }
}

const fn ingest_run_status_key(status: IngestRunStatus) -> &'static str {
    match status {
        IngestRunStatus::Success => "success",
        IngestRunStatus::Failed => "failed",
    }
}

fn to_i64(value: usize, field: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| anyhow!("{field} exceeds sqlite INTEGER range"))
}

#[derive(Debug, Clone)]
struct SourceWatermarkState {
    source_kind: String,
    source_path: String,
    source_record_locator: String,
    source_record_hash: Option<String>,
    last_event_timestamp_unix_ms: i64,
}

#[derive(Debug, Clone)]
struct ExistingSourceWatermarkState {
    source_record_locator: Option<String>,
    source_record_hash: Option<String>,
    last_event_timestamp_unix_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncrementalDecision {
    Process,
    Skip,
}

fn load_existing_source_watermarks(
    connection: &rusqlite::Connection,
) -> Result<BTreeMap<String, ExistingSourceWatermarkState>> {
    let mut statement = connection.prepare(&format!(
        "SELECT source_key, source_record_locator, source_record_hash, last_event_timestamp_unix_ms
         FROM {INGEST_WATERMARKS_TABLE}"
    ))?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            ExistingSourceWatermarkState {
                source_record_locator: row.get(1)?,
                source_record_hash: row.get(2)?,
                last_event_timestamp_unix_ms: row.get(3)?,
            },
        ))
    })?;

    let mut result = BTreeMap::new();
    for row in rows {
        let (source_key, watermark) = row?;
        result.insert(source_key, watermark);
    }
    Ok(result)
}

fn derive_incremental_decision(
    previous: Option<&ExistingSourceWatermarkState>,
    current: &SourceWatermarkState,
) -> (IncrementalDecision, &'static str, &'static str) {
    let Some(previous) = previous else {
        return (IncrementalDecision::Process, "no_prior_watermark", "stale");
    };

    let Some(previous_timestamp) = previous.last_event_timestamp_unix_ms else {
        return (IncrementalDecision::Process, "no_prior_timestamp", "stale");
    };

    if current.last_event_timestamp_unix_ms > previous_timestamp {
        return (IncrementalDecision::Process, "advanced_timestamp", "stale");
    }
    if current.last_event_timestamp_unix_ms < previous_timestamp {
        return (IncrementalDecision::Process, "regressed_timestamp", "stale");
    }

    if previous.source_record_locator.as_deref() != Some(current.source_record_locator.as_str())
        || previous.source_record_hash.as_deref() != current.source_record_hash.as_deref()
    {
        return (IncrementalDecision::Process, "changed_marker", "stale");
    }

    (IncrementalDecision::Skip, "unchanged_frontier", "fresh")
}

const fn incremental_decision_key(decision: IncrementalDecision) -> &'static str {
    match decision {
        IncrementalDecision::Process => "process",
        IncrementalDecision::Skip => "skip",
    }
}

#[must_use]
pub fn default_plan_from_paths(
    out_dir: &Path,
    source_root: &Path,
    fail_fast: bool,
) -> IngestRefreshPlan {
    IngestRefreshPlan {
        events_jsonl_path: out_dir.join("events.jsonl"),
        sqlite_path: out_dir.join("mart.sqlite"),
        source_root: source_root.to_path_buf(),
        fail_fast,
    }
}

#[must_use]
pub fn ingest_report_artifact_path(out_dir: &Path) -> PathBuf {
    out_dir.join("ingest").join("report.json")
}

#[must_use]
pub fn build_ingest_report_artifact(report: &IngestRefreshReport) -> IngestReportArtifact {
    IngestReportArtifact {
        schema_version: INGEST_REPORT_SCHEMA_VERSION.to_string(),
        ingest_run_id: report.ingest_run_id.clone(),
        source_root: report.source_root.clone(),
        status: report.status,
        started_at_utc: report.started_at_utc.clone(),
        finished_at_utc: report.finished_at_utc.clone(),
        duration_ms: report.duration_ms,
        counts: IngestReportCounts {
            read: report.events_read,
            inserted: report.events_written,
            updated: 0,
            skipped: report.events_skipped,
        },
        warnings: report.warnings.clone(),
        watermarks: IngestReportWatermarkStatus {
            sources_upserted: report.watermarks_upserted,
            staleness_state: report.watermark_staleness_state.clone(),
        },
    }
}

pub fn write_ingest_report_artifact(path: &Path, report: &IngestRefreshReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create ingest report artifact directory: {}",
                parent.display()
            )
        })?;
    }
    let artifact = build_ingest_report_artifact(report);
    let encoded =
        serde_json::to_vec_pretty(&artifact).context("failed to encode ingest report artifact")?;
    std::fs::write(path, encoded)
        .with_context(|| format!("failed to write ingest report artifact: {}", path.display()))
}
