use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};

use crate::adapters::{AdapterKind, all_adapter_kinds};
use crate::discovery::{
    self, DiscoveryPathRole, HistoryScore, PrioritizedSource, SourceFormatHint,
    SourceSelectionFilter,
};
use crate::models::{
    AgentLogEvent, AgentSource, EventType, RecordFormat, SCHEMA_VERSION, TimestampQuality,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizationPlan {
    pub adapters: Vec<AdapterKind>,
    pub fail_fast: bool,
}

impl Default for NormalizationPlan {
    fn default() -> Self {
        Self {
            adapters: all_adapter_kinds().to_vec(),
            fail_fast: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactLayout {
    pub events_jsonl: PathBuf,
    pub schema_json: PathBuf,
    pub stats_json: PathBuf,
}

#[must_use]
pub fn default_plan() -> NormalizationPlan {
    NormalizationPlan::default()
}

#[must_use]
pub fn build_artifact_layout(out_dir: &Path) -> ArtifactLayout {
    ArtifactLayout {
        events_jsonl: out_dir.join("events.jsonl"),
        schema_json: out_dir.join("agentlog.v1.schema.json"),
        stats_json: out_dir.join("stats.json"),
    }
}

#[must_use]
pub fn build_schema_document() -> Value {
    crate::models::json_schema()
}

pub fn write_schema_artifact(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("failed to create schema artifact directory")?;
    }

    let schema = build_schema_document();
    let encoded = serde_json::to_vec_pretty(&schema).context("failed to encode schema json")?;
    std::fs::write(path, encoded).context("failed to write schema artifact")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizeCounts {
    pub input_records: usize,
    pub records_emitted: usize,
    pub duplicates_removed: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizeStats {
    pub schema_version: String,
    pub counts: NormalizeCounts,
    pub adapter_contributions: BTreeMap<String, usize>,
    pub source_contributions: BTreeMap<String, usize>,
    pub record_format_counts: BTreeMap<String, usize>,
    pub event_type_counts: BTreeMap<String, usize>,
    pub timestamp_quality_counts: BTreeMap<String, usize>,
}

#[must_use]
pub fn build_normalize_stats(
    events: &[AgentLogEvent],
    dedupe_stats: DedupeStats,
) -> NormalizeStats {
    let mut adapter_contributions =
        seeded_counts(&["codex", "claude", "gemini", "amp", "opencode"]);
    let mut source_contributions = seeded_counts(&["codex", "claude", "gemini", "amp", "opencode"]);
    let mut record_format_counts = seeded_counts(&[
        "message",
        "tool_call",
        "tool_result",
        "system",
        "diagnostic",
    ]);
    let mut event_type_counts = seeded_counts(&[
        "prompt",
        "response",
        "system_notice",
        "tool_invocation",
        "tool_output",
        "status_update",
        "error",
        "metric",
        "artifact_reference",
        "debug_log",
    ]);
    let mut timestamp_quality_counts = seeded_counts(&["exact", "derived", "fallback"]);
    let mut warning_count = 0_usize;
    let mut error_count = 0_usize;

    for event in events {
        increment_count(
            &mut adapter_contributions,
            source_kind_key(event.adapter_name),
        );
        increment_count(
            &mut source_contributions,
            source_kind_key(event.source_kind),
        );
        increment_count(
            &mut record_format_counts,
            record_format_key(event.record_format),
        );
        increment_count(&mut event_type_counts, event_type_key(event.event_type));
        increment_count(
            &mut timestamp_quality_counts,
            timestamp_quality_key(event.timestamp_quality),
        );
        warning_count += event.warnings.len();
        error_count += event.errors.len();
    }

    NormalizeStats {
        schema_version: SCHEMA_VERSION.to_string(),
        counts: NormalizeCounts {
            input_records: dedupe_stats.input_records,
            records_emitted: events.len(),
            duplicates_removed: dedupe_stats.duplicate_records,
            warnings: warning_count,
            errors: error_count,
        },
        adapter_contributions,
        source_contributions,
        record_format_counts,
        event_type_counts,
        timestamp_quality_counts,
    }
}

pub fn write_events_artifact(path: &Path, events: &[AgentLogEvent]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("failed to create events artifact directory")?;
    }

    let file = std::fs::File::create(path).context("failed to create events artifact")?;
    let mut writer = BufWriter::new(file);
    for event in events {
        serde_json::to_writer(&mut writer, event).context("failed to encode events jsonl row")?;
        writer
            .write_all(b"\n")
            .context("failed to write events newline")?;
    }
    writer
        .flush()
        .context("failed to flush events artifact writer")
}

pub fn write_stats_artifact(path: &Path, stats: &NormalizeStats) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("failed to create stats artifact directory")?;
    }

    let encoded = serde_json::to_vec_pretty(stats).context("failed to encode stats json")?;
    std::fs::write(path, encoded).context("failed to write stats artifact")
}

pub fn write_normalize_artifacts(
    layout: &ArtifactLayout,
    events: &[AgentLogEvent],
    dedupe_stats: DedupeStats,
) -> Result<NormalizeStats> {
    write_events_artifact(&layout.events_jsonl, events)?;
    write_schema_artifact(&layout.schema_json)?;
    let stats = build_normalize_stats(events, dedupe_stats);
    write_stats_artifact(&layout.stats_json, &stats)?;
    Ok(stats)
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizeOrchestrationResult {
    pub events: Vec<AgentLogEvent>,
    pub dedupe_stats: DedupeStats,
    pub prioritized_sources: Vec<PrioritizedSource>,
    pub history_scores: Vec<HistoryScore>,
    pub warnings: Vec<String>,
    pub adapter_health: BTreeMap<String, AdapterHealthReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterHealthStatus {
    Success,
    PartialFailure,
    Failed,
    Skipped,
}

impl AdapterHealthStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::PartialFailure => "partial_failure",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdapterHealthReport {
    pub status: AdapterHealthStatus,
    pub reason: Option<String>,
    pub sources_considered: usize,
    pub files_discovered: usize,
    pub files_parsed: usize,
    pub events_emitted: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct AdapterHealthAccumulator {
    unsupported: bool,
    sources_considered: usize,
    files_discovered: usize,
    files_parsed: usize,
    events_emitted: usize,
    warnings: Vec<String>,
    errors: Vec<String>,
}

impl AdapterHealthAccumulator {
    fn finalize(self) -> AdapterHealthReport {
        let (status, reason) = if self.unsupported {
            (
                AdapterHealthStatus::Skipped,
                Some("adapter_not_supported_by_normalize_v1".to_string()),
            )
        } else if self.sources_considered == 0 {
            (
                AdapterHealthStatus::Skipped,
                Some("no_discovered_sources".to_string()),
            )
        } else if self.errors.is_empty() {
            (AdapterHealthStatus::Success, None)
        } else if self.events_emitted > 0 {
            (
                AdapterHealthStatus::PartialFailure,
                Some("adapter_emitted_partial_results".to_string()),
            )
        } else {
            (
                AdapterHealthStatus::Failed,
                Some("adapter_failed_without_emitting_events".to_string()),
            )
        };

        AdapterHealthReport {
            status,
            reason,
            sources_considered: self.sources_considered,
            files_discovered: self.files_discovered,
            files_parsed: self.files_parsed,
            events_emitted: self.events_emitted,
            warnings: self.warnings,
            errors: self.errors,
        }
    }
}

pub fn orchestrate_normalization(
    plan: &NormalizationPlan,
    home_dir: &Path,
    source_root_override: Option<&Path>,
    zsh_history: &str,
) -> Result<NormalizeOrchestrationResult> {
    let filter = SourceSelectionFilter {
        adapters: plan.adapters.clone(),
        ..SourceSelectionFilter::default()
    };
    let history_scores = discovery::zsh_history_scores(zsh_history);
    let discovery_rules = discovery::known_path_registry();
    let prioritized_sources =
        discovery::prioritize_sources(&discovery_rules, &history_scores, &filter);
    let run_id = "normalize-orchestrator-v1";

    let mut warnings = Vec::new();
    let mut events = Vec::new();
    let mut adapter_health = plan
        .adapters
        .iter()
        .map(|adapter| {
            (
                adapter.as_str().to_string(),
                AdapterHealthAccumulator::default(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for source in &prioritized_sources {
        let health = adapter_health
            .entry(source.adapter.as_str().to_string())
            .or_default();
        health.sources_considered += 1;

        if !adapter_supported_for_v1(source.adapter) {
            let warning = format!(
                "adapter `{}` not yet supported by normalize orchestrator; skipped `{}`",
                source.adapter.as_str(),
                source.path
            );
            health.unsupported = true;
            health.warnings.push(warning.clone());
            warnings.push(warning);
            continue;
        }

        if !matches!(
            source.format_hint,
            SourceFormatHint::Directory | SourceFormatHint::Jsonl
        ) {
            continue;
        }

        let resolved = resolve_candidate_path(&source.path, home_dir, source_root_override);
        if !resolved.exists() {
            warnings.push(format!("source path not found: {}", resolved.display()));
            continue;
        }

        let candidate_files = match collect_parseable_files_resolved(&resolved, source) {
            Ok(files) => files,
            Err(error) if plan.fail_fast => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to collect parseable files for adapter `{}` from `{}`",
                        source.adapter.as_str(),
                        resolved.display()
                    )
                });
            }
            Err(error) => {
                let diagnostic = format!(
                    "adapter `{}` source path unreadable `{}`: {error}",
                    source.adapter.as_str(),
                    resolved.display()
                );
                health.errors.push(diagnostic.clone());
                warnings.push(diagnostic);
                continue;
            }
        };
        health.files_discovered += candidate_files.len();
        for file in candidate_files {
            match parse_supported_source_file(source.adapter, source.role, &file, run_id) {
                Ok((mut parsed_events, mut parse_warnings)) => {
                    health.files_parsed += 1;
                    health.events_emitted += parsed_events.len();
                    health.warnings.extend(parse_warnings.iter().cloned());
                    events.append(&mut parsed_events);
                    warnings.append(&mut parse_warnings);
                }
                Err(error) if plan.fail_fast => {
                    return Err(error).with_context(|| {
                        format!(
                            "normalize orchestrator failed while parsing `{}` for adapter `{}`",
                            file.display(),
                            source.adapter.as_str()
                        )
                    });
                }
                Err(error) => {
                    let diagnostic = format!(
                        "adapter `{}` parse error for `{}`: {error}",
                        source.adapter.as_str(),
                        file.display()
                    );
                    health.errors.push(diagnostic.clone());
                    warnings.push(diagnostic);
                }
            }
        }
    }

    let (events, dedupe_stats) = dedupe_and_sort_events(events);
    let adapter_health = adapter_health
        .into_iter()
        .map(|(adapter, health)| (adapter, health.finalize()))
        .collect();
    Ok(NormalizeOrchestrationResult {
        events,
        dedupe_stats,
        prioritized_sources,
        history_scores,
        warnings,
        adapter_health,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DedupeStats {
    pub input_records: usize,
    pub unique_records: usize,
    pub duplicate_records: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DedupeStrategy {
    CanonicalHash,
    FallbackA,
    FallbackB,
}

impl DedupeStrategy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CanonicalHash => "canonical_hash",
            Self::FallbackA => "fallback_a",
            Self::FallbackB => "fallback_b",
        }
    }
}

#[derive(Debug, Clone)]
struct MergeBucket {
    primary: AgentLogEvent,
    strategy: DedupeStrategy,
    member_ids: BTreeSet<String>,
    provenance_entries: Vec<Value>,
    provenance_keys: BTreeSet<String>,
}

#[must_use]
pub fn dedupe_and_sort_events(events: Vec<AgentLogEvent>) -> (Vec<AgentLogEvent>, DedupeStats) {
    let input_records = events.len();
    let mut grouped = BTreeMap::<String, MergeBucket>::new();

    for event in events {
        let strategy = dedupe_strategy_for(&event);
        let key = dedupe_key_for(&event, strategy);

        if let Some(bucket) = grouped.get_mut(&key) {
            add_event_to_bucket(bucket, event);
        } else {
            grouped.insert(key, new_bucket(event, strategy));
        }
    }

    let mut deduped = grouped
        .into_values()
        .map(finalize_bucket)
        .collect::<Vec<AgentLogEvent>>();
    deduped.sort_by(compare_events);

    for (index, event) in deduped.iter_mut().enumerate() {
        event.sequence_global = index as u64;
    }

    let unique_records = deduped.len();
    let stats = DedupeStats {
        input_records,
        unique_records,
        duplicate_records: input_records.saturating_sub(unique_records),
    };

    (deduped, stats)
}

fn new_bucket(event: AgentLogEvent, strategy: DedupeStrategy) -> MergeBucket {
    let mut member_ids = BTreeSet::new();
    member_ids.insert(event.event_id.clone());

    let mut provenance_entries = Vec::new();
    let mut provenance_keys = BTreeSet::new();
    let provenance_key = provenance_key(&event);
    provenance_keys.insert(provenance_key);
    provenance_entries.push(provenance_entry(&event));

    MergeBucket {
        primary: event,
        strategy,
        member_ids,
        provenance_entries,
        provenance_keys,
    }
}

fn add_event_to_bucket(bucket: &mut MergeBucket, event: AgentLogEvent) {
    bucket.member_ids.insert(event.event_id.clone());

    let key = provenance_key(&event);
    if bucket.provenance_keys.insert(key) {
        bucket.provenance_entries.push(provenance_entry(&event));
    }

    if prefers_candidate(&event, &bucket.primary) {
        bucket.primary = event;
    }
}

fn finalize_bucket(mut bucket: MergeBucket) -> AgentLogEvent {
    bucket
        .primary
        .metadata
        .insert("dedupe_count".to_string(), json!(bucket.member_ids.len()));
    bucket.primary.metadata.insert(
        "dedupe_strategy".to_string(),
        Value::String(bucket.strategy.as_str().to_string()),
    );
    bucket.primary.metadata.insert(
        "dedupe_members".to_string(),
        Value::Array(bucket.member_ids.into_iter().map(Value::String).collect()),
    );
    bucket.primary.metadata.insert(
        "provenance_entries".to_string(),
        Value::Array(bucket.provenance_entries),
    );
    bucket.primary
}

fn dedupe_strategy_for(event: &AgentLogEvent) -> DedupeStrategy {
    if !event.canonical_hash.trim().is_empty() {
        DedupeStrategy::CanonicalHash
    } else if event.conversation_id.is_some()
        || event.turn_id.is_some()
        || event.content_text.is_some()
    {
        DedupeStrategy::FallbackA
    } else {
        DedupeStrategy::FallbackB
    }
}

fn dedupe_key_for(event: &AgentLogEvent, strategy: DedupeStrategy) -> String {
    match strategy {
        DedupeStrategy::CanonicalHash => format!("canonical:{}", event.canonical_hash),
        DedupeStrategy::FallbackA => format!(
            "a:{}|{}|{}|{}|{}",
            source_kind_key(event.source_kind),
            event.conversation_id.as_deref().unwrap_or(""),
            event.turn_id.as_deref().unwrap_or(""),
            role_key(event),
            content_key(event.content_text.as_deref())
        ),
        DedupeStrategy::FallbackB => format!(
            "b:{}|{}|{}",
            source_kind_key(event.source_kind),
            event.source_path,
            event.source_record_locator
        ),
    }
}

fn content_key(content: Option<&str>) -> String {
    content
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

fn adapter_supported_for_v1(adapter: AdapterKind) -> bool {
    matches!(
        adapter,
        AdapterKind::Codex | AdapterKind::Claude | AdapterKind::Gemini | AdapterKind::Amp
    )
}

fn resolve_candidate_path(
    candidate: &str,
    home_dir: &Path,
    source_root_override: Option<&Path>,
) -> PathBuf {
    if let Some(stripped) = candidate.strip_prefix("~/") {
        if let Some(root) = source_root_override {
            return root.join(stripped);
        }
        return home_dir.join(stripped);
    }

    if candidate == "~" {
        return source_root_override.unwrap_or(home_dir).to_path_buf();
    }

    if let Some(root) = source_root_override {
        return root.join(candidate);
    }

    PathBuf::from(candidate)
}

fn collect_parseable_files_resolved(
    resolved: &Path,
    source: &PrioritizedSource,
) -> Result<Vec<PathBuf>> {
    if resolved.is_file() {
        return Ok(vec![resolved.to_path_buf()]);
    }

    if !resolved.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_dir_files(resolved, source.recursive, &mut files)?;
    files.sort();
    files.retain(|path| is_parseable_source_file(path, source));
    Ok(files)
}

fn is_parseable_source_file(path: &Path, source: &PrioritizedSource) -> bool {
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase);
    let amp_file_changes_source =
        source.adapter == AdapterKind::Amp && source.path.contains("/.amp/file-changes");

    match source.adapter {
        AdapterKind::Codex | AdapterKind::Claude => {
            matches!(extension.as_deref(), Some("jsonl") | Some("ndjson"))
        }
        AdapterKind::Gemini => matches!(
            extension.as_deref(),
            Some("json") | Some("jsonl") | Some("ndjson")
        ),
        AdapterKind::Amp => {
            if amp_file_changes_source {
                true
            } else {
                matches!(
                    extension.as_deref(),
                    Some("json") | Some("jsonl") | Some("ndjson")
                )
            }
        }
        AdapterKind::OpenCode => matches!(
            extension.as_deref(),
            Some("json") | Some("jsonl") | Some("ndjson")
        ),
    }
}

fn collect_dir_files(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = std::fs::read_dir(dir)
        .with_context(|| format!("failed to read source directory: {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to enumerate source directory: {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_file() {
            out.push(path);
        } else if recursive && path.is_dir() {
            collect_dir_files(&path, recursive, out)?;
        }
    }

    Ok(())
}

fn parse_supported_source_file(
    adapter: AdapterKind,
    source_role: DiscoveryPathRole,
    path: &Path,
    run_id: &str,
) -> Result<(Vec<AgentLogEvent>, Vec<String>)> {
    match adapter {
        AdapterKind::Codex => {
            let parsed = match source_role {
                DiscoveryPathRole::HistoryStream => {
                    let parsed = crate::adapters::codex::parse_history_file(path, run_id)?;
                    return Ok((parsed.events, parsed.warnings));
                }
                _ => crate::adapters::codex::parse_rollout_file(path, run_id)?,
            };
            Ok((parsed.events, parsed.warnings))
        }
        AdapterKind::Claude => {
            let parsed = crate::adapters::claude::parse_project_session_file(path, run_id)?;
            Ok((parsed.events, parsed.warnings))
        }
        AdapterKind::Gemini => {
            let file_name = path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let source_path = path.to_string_lossy();
            if file_name == "logs.json" {
                let parsed = crate::adapters::gemini::parse_logs_file(path, run_id)?;
                return Ok((parsed.events, parsed.warnings));
            }
            if source_path.contains("/chats/") || file_name.starts_with("session-") {
                let parsed = crate::adapters::gemini::parse_chat_session_file(path, run_id)?;
                return Ok((parsed.events, parsed.warnings));
            }
            Ok((
                Vec::new(),
                vec![format!(
                    "adapter `gemini` skipped unsupported source file shape in normalize orchestrator: {}",
                    path.display()
                )],
            ))
        }
        AdapterKind::Amp => {
            let source_path = path.to_string_lossy();
            if source_path.contains("/file-changes/") || source_path.ends_with("/file-changes") {
                let parsed = crate::adapters::amp::parse_file_change_event_file(path, run_id)?;
                return Ok((parsed.events, parsed.warnings));
            }
            Ok((
                Vec::new(),
                vec![format!(
                    "adapter `amp` skipped unsupported source file shape in normalize orchestrator: {}",
                    path.display()
                )],
            ))
        }
        AdapterKind::OpenCode => Ok((
            Vec::new(),
            vec![format!(
                "adapter `{}` file parsing not yet implemented in normalize orchestrator: {}",
                adapter.as_str(),
                path.display()
            )],
        )),
    }
}

fn seeded_counts(keys: &[&str]) -> BTreeMap<String, usize> {
    keys.iter()
        .map(|key| ((*key).to_string(), 0_usize))
        .collect()
}

fn increment_count(counts: &mut BTreeMap<String, usize>, key: &str) {
    if let Some(count) = counts.get_mut(key) {
        *count += 1;
    } else {
        counts.insert(key.to_string(), 1);
    }
}

fn prefers_candidate(candidate: &AgentLogEvent, current: &AgentLogEvent) -> bool {
    let candidate_rank = timestamp_quality_rank(candidate.timestamp_quality);
    let current_rank = timestamp_quality_rank(current.timestamp_quality);

    if candidate_rank != current_rank {
        return candidate_rank < current_rank;
    }

    if candidate.metadata.len() != current.metadata.len() {
        return candidate.metadata.len() > current.metadata.len();
    }

    candidate.event_id < current.event_id
}

fn compare_events(left: &AgentLogEvent, right: &AgentLogEvent) -> Ordering {
    left.timestamp_unix_ms
        .cmp(&right.timestamp_unix_ms)
        .then_with(|| {
            timestamp_quality_rank(left.timestamp_quality)
                .cmp(&timestamp_quality_rank(right.timestamp_quality))
        })
        .then_with(|| source_kind_key(left.source_kind).cmp(source_kind_key(right.source_kind)))
        .then_with(|| left.source_path.cmp(&right.source_path))
        .then_with(|| left.source_record_locator.cmp(&right.source_record_locator))
        .then_with(|| {
            left.sequence_source
                .unwrap_or(u64::MAX)
                .cmp(&right.sequence_source.unwrap_or(u64::MAX))
        })
        .then_with(|| left.canonical_hash.cmp(&right.canonical_hash))
        .then_with(|| left.event_id.cmp(&right.event_id))
}

const fn timestamp_quality_rank(quality: TimestampQuality) -> u8 {
    match quality {
        TimestampQuality::Exact => 0,
        TimestampQuality::Derived => 1,
        TimestampQuality::Fallback => 2,
    }
}

const fn timestamp_quality_key(quality: TimestampQuality) -> &'static str {
    match quality {
        TimestampQuality::Exact => "exact",
        TimestampQuality::Derived => "derived",
        TimestampQuality::Fallback => "fallback",
    }
}

const fn source_kind_key(kind: AgentSource) -> &'static str {
    match kind {
        AgentSource::Codex => "codex",
        AgentSource::Claude => "claude",
        AgentSource::Gemini => "gemini",
        AgentSource::Amp => "amp",
        AgentSource::OpenCode => "opencode",
    }
}

const fn record_format_key(record_format: RecordFormat) -> &'static str {
    match record_format {
        RecordFormat::Message => "message",
        RecordFormat::ToolCall => "tool_call",
        RecordFormat::ToolResult => "tool_result",
        RecordFormat::System => "system",
        RecordFormat::Diagnostic => "diagnostic",
    }
}

const fn event_type_key(event_type: EventType) -> &'static str {
    match event_type {
        EventType::Prompt => "prompt",
        EventType::Response => "response",
        EventType::SystemNotice => "system_notice",
        EventType::ToolInvocation => "tool_invocation",
        EventType::ToolOutput => "tool_output",
        EventType::StatusUpdate => "status_update",
        EventType::Error => "error",
        EventType::Metric => "metric",
        EventType::ArtifactReference => "artifact_reference",
        EventType::DebugLog => "debug_log",
    }
}

fn role_key(event: &AgentLogEvent) -> &'static str {
    match event.role {
        crate::models::ActorRole::User => "user",
        crate::models::ActorRole::Assistant => "assistant",
        crate::models::ActorRole::System => "system",
        crate::models::ActorRole::Tool => "tool",
        crate::models::ActorRole::Runtime => "runtime",
    }
}

fn provenance_key(event: &AgentLogEvent) -> String {
    format!(
        "{}|{}|{}|{}",
        source_kind_key(event.source_kind),
        event.source_path,
        event.source_record_locator,
        event.raw_hash
    )
}

fn provenance_entry(event: &AgentLogEvent) -> Value {
    json!({
        "source_kind": source_kind_key(event.source_kind),
        "source_path": event.source_path,
        "source_record_locator": event.source_record_locator,
        "raw_hash": event.raw_hash,
        "adapter_name": source_kind_key(event.adapter_name),
        "adapter_version": event.adapter_version,
    })
}
