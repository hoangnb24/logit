use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, params, params_from_iter};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, TimestampQuality,
};

pub const SQLITE_SCHEMA_VERSION: &str = "agentlog.v1.sqlite.v1";
pub const EVENTS_TABLE: &str = "agentlog_events";
pub const INGEST_RUNS_TABLE: &str = "ingest_runs";
pub const INGEST_WATERMARKS_TABLE: &str = "ingest_watermarks";
pub const TOOL_CALLS_VIEW: &str = "v_tool_calls";
pub const SESSIONS_VIEW: &str = "v_sessions";
pub const ADAPTERS_VIEW: &str = "v_adapters";
pub const QUALITY_VIEW: &str = "v_quality";
pub const SCHEMA_META_TABLE: &str = "agentlog_schema_meta";
pub const DEFAULT_INSERT_BATCH_SIZE: usize = 500;

pub const EVENT_INSERT_COLUMNS: &[&str] = &[
    "schema_version",
    "event_id",
    "run_id",
    "sequence_global",
    "sequence_source",
    "source_kind",
    "source_path",
    "source_record_locator",
    "source_record_hash",
    "adapter_name",
    "adapter_version",
    "record_format",
    "event_type",
    "role",
    "timestamp_utc",
    "timestamp_unix_ms",
    "timestamp_quality",
    "session_id",
    "conversation_id",
    "turn_id",
    "parent_event_id",
    "actor_id",
    "actor_name",
    "provider",
    "model",
    "content_text",
    "content_excerpt",
    "content_mime",
    "tool_name",
    "tool_call_id",
    "tool_arguments_json",
    "tool_result_text",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "cost_usd",
    "tags_json",
    "flags_json",
    "pii_redacted",
    "warnings_json",
    "errors_json",
    "raw_hash",
    "canonical_hash",
    "metadata_json",
];

const CREATE_EVENTS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS agentlog_events (
    schema_version TEXT NOT NULL,
    event_id TEXT NOT NULL PRIMARY KEY,
    run_id TEXT NOT NULL,
    sequence_global INTEGER NOT NULL,
    sequence_source INTEGER,
    source_kind TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_record_locator TEXT NOT NULL,
    source_record_hash TEXT,
    adapter_name TEXT NOT NULL,
    adapter_version TEXT,
    record_format TEXT NOT NULL,
    event_type TEXT NOT NULL,
    role TEXT NOT NULL,
    timestamp_utc TEXT NOT NULL,
    timestamp_unix_ms INTEGER NOT NULL,
    timestamp_quality TEXT NOT NULL,
    session_id TEXT,
    conversation_id TEXT,
    turn_id TEXT,
    parent_event_id TEXT,
    actor_id TEXT,
    actor_name TEXT,
    provider TEXT,
    model TEXT,
    content_text TEXT,
    content_excerpt TEXT,
    content_mime TEXT,
    tool_name TEXT,
    tool_call_id TEXT,
    tool_arguments_json TEXT,
    tool_result_text TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    total_tokens INTEGER,
    cost_usd REAL,
    tags_json TEXT NOT NULL DEFAULT '[]',
    flags_json TEXT NOT NULL DEFAULT '[]',
    pii_redacted INTEGER,
    warnings_json TEXT NOT NULL DEFAULT '[]',
    errors_json TEXT NOT NULL DEFAULT '[]',
    raw_hash TEXT NOT NULL,
    canonical_hash TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    CHECK (schema_version = 'agentlog.v1'),
    CHECK (source_kind IN ('codex', 'claude', 'gemini', 'amp', 'opencode')),
    CHECK (adapter_name IN ('codex', 'claude', 'gemini', 'amp', 'opencode')),
    CHECK (record_format IN ('message', 'tool_call', 'tool_result', 'system', 'diagnostic')),
    CHECK (event_type IN (
        'prompt',
        'response',
        'system_notice',
        'tool_invocation',
        'tool_output',
        'status_update',
        'error',
        'metric',
        'artifact_reference',
        'debug_log'
    )),
    CHECK (role IN ('user', 'assistant', 'system', 'tool', 'runtime')),
    CHECK (timestamp_quality IN ('exact', 'derived', 'fallback')),
    CHECK (pii_redacted IN (0, 1) OR pii_redacted IS NULL)
);
"#;

const CREATE_INDEX_RUN_SEQUENCE_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_agentlog_events_run_sequence
ON agentlog_events (run_id, sequence_global);
"#;

const CREATE_INDEX_TIME_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_agentlog_events_timestamp
ON agentlog_events (timestamp_unix_ms, sequence_global);
"#;

const CREATE_INDEX_ADAPTER_EVENT_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_agentlog_events_adapter_event
ON agentlog_events (adapter_name, event_type);
"#;

const CREATE_INDEX_SOURCE_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_agentlog_events_source
ON agentlog_events (source_kind, source_path, source_record_locator);
"#;

const CREATE_INDEX_HASHES_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_agentlog_events_hashes
ON agentlog_events (canonical_hash, raw_hash);
"#;

const CREATE_INDEX_SESSION_TIME_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_agentlog_events_session_time
ON agentlog_events (session_id, timestamp_unix_ms);
"#;

const CREATE_VIEW_TOOL_CALLS_SQL: &str = r#"
CREATE VIEW IF NOT EXISTS v_tool_calls AS
WITH call_events AS (
    SELECT
        event_id AS call_event_id,
        run_id,
        session_id,
        conversation_id,
        turn_id,
        adapter_name,
        source_kind,
        tool_name,
        tool_call_id,
        timestamp_unix_ms AS call_timestamp_unix_ms
    FROM agentlog_events
    WHERE record_format = 'tool_call'
),
result_events AS (
    SELECT
        event_id AS result_event_id,
        run_id,
        session_id,
        conversation_id,
        turn_id,
        adapter_name,
        source_kind,
        tool_name,
        tool_call_id,
        timestamp_unix_ms AS result_timestamp_unix_ms
    FROM agentlog_events
    WHERE record_format = 'tool_result'
),
ranked_results AS (
    SELECT
        result_event_id,
        run_id,
        session_id,
        conversation_id,
        turn_id,
        adapter_name,
        source_kind,
        tool_name,
        tool_call_id,
        result_timestamp_unix_ms,
        ROW_NUMBER() OVER (
            PARTITION BY run_id, tool_call_id
            ORDER BY result_timestamp_unix_ms ASC, result_event_id ASC
        ) AS result_rank
    FROM result_events
    WHERE tool_call_id IS NOT NULL
      AND tool_call_id != ''
)
SELECT
    call_events.run_id,
    call_events.session_id,
    call_events.conversation_id,
    call_events.turn_id,
    call_events.adapter_name,
    call_events.source_kind,
    COALESCE(call_events.tool_name, ranked_results.tool_name) AS tool_name,
    call_events.tool_call_id,
    call_events.call_event_id,
    ranked_results.result_event_id,
    call_events.call_timestamp_unix_ms,
    ranked_results.result_timestamp_unix_ms,
    CASE
        WHEN ranked_results.result_timestamp_unix_ms IS NULL THEN NULL
        WHEN ranked_results.result_timestamp_unix_ms < call_events.call_timestamp_unix_ms THEN NULL
        ELSE ranked_results.result_timestamp_unix_ms - call_events.call_timestamp_unix_ms
    END AS duration_ms,
    CASE
        WHEN ranked_results.result_timestamp_unix_ms IS NOT NULL
         AND ranked_results.result_timestamp_unix_ms >= call_events.call_timestamp_unix_ms
        THEN 'paired'
        ELSE NULL
    END AS duration_source,
    CASE
        WHEN ranked_results.result_timestamp_unix_ms IS NOT NULL
         AND ranked_results.result_timestamp_unix_ms >= call_events.call_timestamp_unix_ms
        THEN 'medium'
        ELSE NULL
    END AS duration_quality,
    CASE
        WHEN ranked_results.result_timestamp_unix_ms IS NULL THEN 'missing_result'
        WHEN ranked_results.result_timestamp_unix_ms < call_events.call_timestamp_unix_ms THEN 'invalid_order'
        ELSE 'paired'
    END AS pairing_status
FROM call_events
LEFT JOIN ranked_results
    ON ranked_results.run_id = call_events.run_id
   AND ranked_results.tool_call_id = call_events.tool_call_id
   AND ranked_results.result_rank = 1
UNION ALL
SELECT
    result_events.run_id,
    result_events.session_id,
    result_events.conversation_id,
    result_events.turn_id,
    result_events.adapter_name,
    result_events.source_kind,
    result_events.tool_name,
    result_events.tool_call_id,
    NULL AS call_event_id,
    result_events.result_event_id,
    NULL AS call_timestamp_unix_ms,
    result_events.result_timestamp_unix_ms,
    NULL AS duration_ms,
    NULL AS duration_source,
    NULL AS duration_quality,
    'orphan_result' AS pairing_status
FROM result_events
LEFT JOIN call_events
    ON call_events.run_id = result_events.run_id
   AND call_events.tool_call_id = result_events.tool_call_id
WHERE call_events.call_event_id IS NULL;
"#;

const CREATE_VIEW_SESSIONS_SQL: &str = r#"
CREATE VIEW IF NOT EXISTS v_sessions AS
SELECT
    run_id,
    session_id,
    MIN(timestamp_unix_ms) AS first_event_timestamp_unix_ms,
    MAX(timestamp_unix_ms) AS last_event_timestamp_unix_ms,
    MAX(timestamp_unix_ms) - MIN(timestamp_unix_ms) AS duration_ms,
    COUNT(*) AS event_count,
    SUM(CASE WHEN record_format = 'tool_call' THEN 1 ELSE 0 END) AS tool_call_count,
    SUM(CASE WHEN record_format = 'tool_result' THEN 1 ELSE 0 END) AS tool_result_count,
    SUM(CASE WHEN event_type = 'prompt' THEN 1 ELSE 0 END) AS prompt_count,
    SUM(CASE WHEN event_type = 'response' THEN 1 ELSE 0 END) AS response_count,
    SUM(CASE WHEN event_type = 'error' THEN 1 ELSE 0 END) AS error_count,
    COUNT(DISTINCT conversation_id) AS distinct_conversation_count,
    COUNT(DISTINCT turn_id) AS distinct_turn_count,
    COUNT(DISTINCT tool_name) AS distinct_tool_count,
    COUNT(DISTINCT adapter_name) AS distinct_adapter_count
FROM agentlog_events
WHERE session_id IS NOT NULL
  AND session_id != ''
GROUP BY run_id, session_id;
"#;

const CREATE_VIEW_ADAPTERS_SQL: &str = r#"
CREATE VIEW IF NOT EXISTS v_adapters AS
SELECT
    adapter_name,
    COUNT(*) AS event_count,
    COUNT(DISTINCT run_id) AS run_count,
    COUNT(DISTINCT session_id) AS session_count,
    MIN(timestamp_unix_ms) AS first_event_timestamp_unix_ms,
    MAX(timestamp_unix_ms) AS last_event_timestamp_unix_ms,
    SUM(CASE WHEN event_type = 'prompt' THEN 1 ELSE 0 END) AS prompt_count,
    SUM(CASE WHEN event_type = 'response' THEN 1 ELSE 0 END) AS response_count,
    SUM(CASE WHEN record_format = 'tool_call' THEN 1 ELSE 0 END) AS tool_call_count,
    SUM(CASE WHEN record_format = 'tool_result' THEN 1 ELSE 0 END) AS tool_result_count,
    SUM(CASE WHEN event_type = 'error' THEN 1 ELSE 0 END) AS error_event_count,
    SUM(CASE WHEN warnings_json != '[]' THEN 1 ELSE 0 END) AS warning_record_count,
    SUM(CASE WHEN errors_json != '[]' THEN 1 ELSE 0 END) AS error_record_count,
    SUM(CASE WHEN pii_redacted = 1 THEN 1 ELSE 0 END) AS pii_redacted_count
FROM agentlog_events
GROUP BY adapter_name;
"#;

const CREATE_VIEW_QUALITY_SQL: &str = r#"
CREATE VIEW IF NOT EXISTS v_quality AS
SELECT
    adapter_name,
    timestamp_quality,
    COUNT(*) AS event_count,
    SUM(CASE WHEN warnings_json != '[]' THEN 1 ELSE 0 END) AS warning_record_count,
    SUM(CASE WHEN errors_json != '[]' THEN 1 ELSE 0 END) AS error_record_count,
    SUM(CASE WHEN flags_json != '[]' THEN 1 ELSE 0 END) AS flagged_record_count,
    SUM(CASE WHEN pii_redacted = 1 THEN 1 ELSE 0 END) AS pii_redacted_count
FROM agentlog_events
GROUP BY adapter_name, timestamp_quality;
"#;

const CREATE_INGEST_RUNS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS ingest_runs (
    ingest_run_id TEXT NOT NULL PRIMARY KEY,
    started_at_utc TEXT NOT NULL,
    finished_at_utc TEXT,
    status TEXT NOT NULL,
    source_root TEXT NOT NULL,
    events_read INTEGER NOT NULL DEFAULT 0,
    events_written INTEGER NOT NULL DEFAULT 0,
    warnings_count INTEGER NOT NULL DEFAULT 0,
    errors_count INTEGER NOT NULL DEFAULT 0,
    error_summary_json TEXT NOT NULL DEFAULT '{}',
    CHECK (status IN ('running', 'success', 'partial_failure', 'failed')),
    CHECK (events_read >= 0),
    CHECK (events_written >= 0),
    CHECK (warnings_count >= 0),
    CHECK (errors_count >= 0)
);
"#;

const CREATE_INDEX_INGEST_RUNS_STATUS_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_ingest_runs_status_time
ON ingest_runs (status, started_at_utc);
"#;

const CREATE_INGEST_WATERMARKS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS ingest_watermarks (
    source_key TEXT NOT NULL PRIMARY KEY,
    source_kind TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_record_locator TEXT,
    source_record_hash TEXT,
    last_event_timestamp_unix_ms INTEGER,
    last_ingest_run_id TEXT,
    refreshed_at_utc TEXT NOT NULL,
    staleness_state TEXT NOT NULL DEFAULT 'unknown',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    CHECK (source_kind IN ('codex', 'claude', 'gemini', 'amp', 'opencode')),
    CHECK (staleness_state IN ('fresh', 'stale', 'unknown')),
    CHECK (last_event_timestamp_unix_ms IS NULL OR last_event_timestamp_unix_ms >= 0),
    FOREIGN KEY(last_ingest_run_id) REFERENCES ingest_runs(ingest_run_id)
);
"#;

const CREATE_INDEX_INGEST_WATERMARKS_SOURCE_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_ingest_watermarks_source
ON ingest_watermarks (source_kind, source_path);
"#;

const CREATE_INDEX_INGEST_WATERMARKS_REFRESH_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_ingest_watermarks_refresh
ON ingest_watermarks (refreshed_at_utc, staleness_state);
"#;

const CREATE_META_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS agentlog_schema_meta (
    schema_version TEXT NOT NULL,
    applied_at_utc TEXT NOT NULL
);
"#;

#[must_use]
pub fn schema_statements() -> &'static [&'static str] {
    &[
        CREATE_EVENTS_TABLE_SQL,
        CREATE_INDEX_RUN_SEQUENCE_SQL,
        CREATE_INDEX_TIME_SQL,
        CREATE_INDEX_ADAPTER_EVENT_SQL,
        CREATE_INDEX_SOURCE_SQL,
        CREATE_INDEX_HASHES_SQL,
        CREATE_INDEX_SESSION_TIME_SQL,
        CREATE_VIEW_TOOL_CALLS_SQL,
        CREATE_VIEW_SESSIONS_SQL,
        CREATE_VIEW_ADAPTERS_SQL,
        CREATE_VIEW_QUALITY_SQL,
        CREATE_INGEST_RUNS_TABLE_SQL,
        CREATE_INDEX_INGEST_RUNS_STATUS_SQL,
        CREATE_INGEST_WATERMARKS_TABLE_SQL,
        CREATE_INDEX_INGEST_WATERMARKS_SOURCE_SQL,
        CREATE_INDEX_INGEST_WATERMARKS_REFRESH_SQL,
        CREATE_META_TABLE_SQL,
    ]
}

#[must_use]
pub fn create_schema_sql() -> String {
    schema_statements().join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqliteWriterConfig {
    pub batch_size: usize,
}

impl Default for SqliteWriterConfig {
    fn default() -> Self {
        Self {
            batch_size: DEFAULT_INSERT_BATCH_SIZE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqliteWriteStats {
    pub input_records: usize,
    pub records_written: usize,
    pub batches_committed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqliteParityMismatch {
    pub event_id: Option<String>,
    pub field: String,
    pub jsonl_value: Option<String>,
    pub sqlite_value: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqliteParityReport {
    pub jsonl_records: usize,
    pub sqlite_records: usize,
    pub compared_records: usize,
    pub mismatches: Vec<SqliteParityMismatch>,
}

type SqliteMirrorRows = BTreeMap<String, Vec<SqlValue>>;
type JsonlParityParseResult = (SqliteMirrorRows, Vec<SqliteParityMismatch>);

impl SqliteParityReport {
    #[must_use]
    pub fn is_match(&self) -> bool {
        self.mismatches.is_empty()
    }
}

pub fn open_sqlite_connection(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create sqlite parent directory: {}",
                parent.display()
            )
        })?;
    }

    Connection::open(path)
        .with_context(|| format!("failed to open sqlite database: {}", path.display()))
}

pub fn ensure_sqlite_schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(&create_schema_sql())
        .context("failed to create sqlite schema")?;

    if schema_meta_has_version(connection, SQLITE_SCHEMA_VERSION)? {
        return Ok(());
    }

    let applied_at_utc = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format sqlite schema applied timestamp")?;
    connection
        .execute(
            &format!(
                "INSERT INTO {SCHEMA_META_TABLE} (schema_version, applied_at_utc) VALUES (?1, ?2)"
            ),
            params![SQLITE_SCHEMA_VERSION, applied_at_utc],
        )
        .context("failed to write sqlite schema meta row")?;

    Ok(())
}

fn schema_meta_has_version(connection: &Connection, schema_version: &str) -> Result<bool> {
    let query = format!(
        "SELECT EXISTS(SELECT 1 FROM {SCHEMA_META_TABLE} WHERE schema_version = ?1 LIMIT 1)"
    );
    let exists = connection
        .query_row(&query, [schema_version], |row| row.get::<usize, i64>(0))
        .context("failed to query sqlite schema version metadata")?;
    Ok(exists != 0)
}

pub fn write_events_to_sqlite(
    path: &Path,
    events: &[AgentLogEvent],
    config: SqliteWriterConfig,
) -> Result<SqliteWriteStats> {
    let mut connection = open_sqlite_connection(path)?;
    ensure_sqlite_schema(&connection)?;
    write_events_batched(&mut connection, events, config)
}

pub fn write_events_batched(
    connection: &mut Connection,
    events: &[AgentLogEvent],
    config: SqliteWriterConfig,
) -> Result<SqliteWriteStats> {
    let batch_size = config.batch_size.max(1);
    let insert_sql = build_insert_sql();
    let mut records_written = 0usize;
    let mut batches_committed = 0usize;

    for batch in events.chunks(batch_size) {
        let tx = connection
            .transaction()
            .context("failed to open sqlite transaction")?;
        {
            let mut statement = tx
                .prepare_cached(&insert_sql)
                .context("failed to prepare sqlite insert statement")?;

            for event in batch {
                let values = event_insert_values(event)?;
                statement
                    .execute(params_from_iter(values))
                    .with_context(|| format!("failed to insert event_id={}", event.event_id))?;
                records_written += 1;
            }
        }
        tx.commit()
            .context("failed to commit sqlite batch transaction")?;
        batches_committed += 1;
    }

    Ok(SqliteWriteStats {
        input_records: events.len(),
        records_written,
        batches_committed,
    })
}

pub fn verify_jsonl_sqlite_parity(
    jsonl_path: &Path,
    sqlite_path: &Path,
) -> Result<SqliteParityReport> {
    let jsonl_input = std::fs::read_to_string(jsonl_path)
        .with_context(|| format!("failed to read JSONL file: {}", jsonl_path.display()))?;
    let (jsonl_rows, mut mismatches) = parse_jsonl_expected_rows(&jsonl_input)?;

    let connection = open_sqlite_connection(sqlite_path)?;
    let sqlite_rows = read_sqlite_rows(&connection)?;

    let mut compared_records = 0usize;

    if jsonl_rows.len() != sqlite_rows.len() {
        mismatches.push(SqliteParityMismatch {
            event_id: None,
            field: "record_count".to_string(),
            jsonl_value: Some(jsonl_rows.len().to_string()),
            sqlite_value: Some(sqlite_rows.len().to_string()),
            detail: "record counts differ between JSONL and SQLite mirror".to_string(),
        });
    }

    for (event_id, expected_values) in &jsonl_rows {
        let Some(actual_values) = sqlite_rows.get(event_id) else {
            mismatches.push(SqliteParityMismatch {
                event_id: Some(event_id.clone()),
                field: "event_id".to_string(),
                jsonl_value: Some(event_id.clone()),
                sqlite_value: None,
                detail: "record present in JSONL but missing from SQLite".to_string(),
            });
            continue;
        };

        compared_records += 1;

        for (index, column) in EVENT_INSERT_COLUMNS.iter().enumerate() {
            let expected = &expected_values[index];
            let actual = &actual_values[index];
            if !sql_values_equal(expected, actual) {
                mismatches.push(SqliteParityMismatch {
                    event_id: Some(event_id.clone()),
                    field: (*column).to_string(),
                    jsonl_value: Some(format_sql_value(expected)),
                    sqlite_value: Some(format_sql_value(actual)),
                    detail: format!("column `{column}` differs for event"),
                });
            }
        }
    }

    for event_id in sqlite_rows.keys() {
        if !jsonl_rows.contains_key(event_id) {
            mismatches.push(SqliteParityMismatch {
                event_id: Some(event_id.clone()),
                field: "event_id".to_string(),
                jsonl_value: None,
                sqlite_value: Some(event_id.clone()),
                detail: "record present in SQLite but missing from JSONL".to_string(),
            });
        }
    }

    mismatches.sort_by(|left, right| {
        left.event_id
            .as_deref()
            .unwrap_or("")
            .cmp(right.event_id.as_deref().unwrap_or(""))
            .then_with(|| left.field.cmp(&right.field))
            .then_with(|| left.detail.cmp(&right.detail))
    });

    Ok(SqliteParityReport {
        jsonl_records: jsonl_rows.len(),
        sqlite_records: sqlite_rows.len(),
        compared_records,
        mismatches,
    })
}

fn build_insert_sql() -> String {
    let placeholders = (1..=EVENT_INSERT_COLUMNS.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let upsert_assignments = EVENT_INSERT_COLUMNS
        .iter()
        .filter(|column| **column != "event_id")
        .map(|column| format!("{column} = excluded.{column}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO {EVENTS_TABLE} ({}) VALUES ({placeholders})
         ON CONFLICT(event_id) DO UPDATE SET {upsert_assignments}",
        EVENT_INSERT_COLUMNS.join(", "),
    )
}

fn event_insert_values(event: &AgentLogEvent) -> Result<Vec<SqlValue>> {
    Ok(vec![
        text_value(crate::models::SCHEMA_VERSION),
        text_value(&event.event_id),
        text_value(&event.run_id),
        int_value(to_i64(event.sequence_global, "sequence_global")?),
        opt_int_value(event.sequence_source, "sequence_source")?,
        text_value(agent_source_key(event.source_kind)),
        text_value(&event.source_path),
        text_value(&event.source_record_locator),
        opt_text_value(event.source_record_hash.as_deref()),
        text_value(agent_source_key(event.adapter_name)),
        opt_text_value(event.adapter_version.as_deref()),
        text_value(record_format_key(event.record_format)),
        text_value(event_type_key(event.event_type)),
        text_value(actor_role_key(event.role)),
        text_value(&event.timestamp_utc),
        int_value(to_i64(event.timestamp_unix_ms, "timestamp_unix_ms")?),
        text_value(timestamp_quality_key(event.timestamp_quality)),
        opt_text_value(event.session_id.as_deref()),
        opt_text_value(event.conversation_id.as_deref()),
        opt_text_value(event.turn_id.as_deref()),
        opt_text_value(event.parent_event_id.as_deref()),
        opt_text_value(event.actor_id.as_deref()),
        opt_text_value(event.actor_name.as_deref()),
        opt_text_value(event.provider.as_deref()),
        opt_text_value(event.model.as_deref()),
        opt_text_value(event.content_text.as_deref()),
        opt_text_value(event.content_excerpt.as_deref()),
        opt_text_value(event.content_mime.as_deref()),
        opt_text_value(event.tool_name.as_deref()),
        opt_text_value(event.tool_call_id.as_deref()),
        opt_text_value(event.tool_arguments_json.as_deref()),
        opt_text_value(event.tool_result_text.as_deref()),
        opt_int_value(event.input_tokens, "input_tokens")?,
        opt_int_value(event.output_tokens, "output_tokens")?,
        opt_int_value(event.total_tokens, "total_tokens")?,
        opt_real_value(event.cost_usd),
        text_value(&to_json_string(&event.tags)?),
        text_value(&to_json_string(&event.flags)?),
        opt_bool_int_value(event.pii_redacted),
        text_value(&to_json_string(&event.warnings)?),
        text_value(&to_json_string(&event.errors)?),
        text_value(&event.raw_hash),
        text_value(&event.canonical_hash),
        text_value(&to_json_string(&event.metadata)?),
    ])
}

fn to_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| anyhow!("{field} exceeds sqlite INTEGER range"))
}

fn to_json_string<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).context("failed to encode sqlite json surrogate column")
}

fn text_value(value: &str) -> SqlValue {
    SqlValue::Text(value.to_string())
}

fn opt_text_value(value: Option<&str>) -> SqlValue {
    value.map_or(SqlValue::Null, text_value)
}

fn int_value(value: i64) -> SqlValue {
    SqlValue::Integer(value)
}

fn opt_int_value(value: Option<u64>, field: &str) -> Result<SqlValue> {
    match value {
        Some(value) => Ok(int_value(to_i64(value, field)?)),
        None => Ok(SqlValue::Null),
    }
}

fn opt_real_value(value: Option<f64>) -> SqlValue {
    value.map_or(SqlValue::Null, SqlValue::Real)
}

fn opt_bool_int_value(value: Option<bool>) -> SqlValue {
    match value {
        Some(true) => SqlValue::Integer(1),
        Some(false) => SqlValue::Integer(0),
        None => SqlValue::Null,
    }
}

fn agent_source_key(source: AgentSource) -> &'static str {
    match source {
        AgentSource::Codex => "codex",
        AgentSource::Claude => "claude",
        AgentSource::Gemini => "gemini",
        AgentSource::Amp => "amp",
        AgentSource::OpenCode => "opencode",
    }
}

fn record_format_key(value: RecordFormat) -> &'static str {
    match value {
        RecordFormat::Message => "message",
        RecordFormat::ToolCall => "tool_call",
        RecordFormat::ToolResult => "tool_result",
        RecordFormat::System => "system",
        RecordFormat::Diagnostic => "diagnostic",
    }
}

fn event_type_key(value: EventType) -> &'static str {
    match value {
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

fn actor_role_key(value: ActorRole) -> &'static str {
    match value {
        ActorRole::User => "user",
        ActorRole::Assistant => "assistant",
        ActorRole::System => "system",
        ActorRole::Tool => "tool",
        ActorRole::Runtime => "runtime",
    }
}

fn timestamp_quality_key(value: TimestampQuality) -> &'static str {
    match value {
        TimestampQuality::Exact => "exact",
        TimestampQuality::Derived => "derived",
        TimestampQuality::Fallback => "fallback",
    }
}

fn parse_jsonl_expected_rows(input: &str) -> Result<JsonlParityParseResult> {
    let mut rows = BTreeMap::new();
    let mut mismatches = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let event = match serde_json::from_str::<AgentLogEvent>(trimmed) {
            Ok(event) => event,
            Err(error) => {
                mismatches.push(SqliteParityMismatch {
                    event_id: None,
                    field: format!("jsonl_line:{line_number}"),
                    jsonl_value: Some(trimmed.to_string()),
                    sqlite_value: None,
                    detail: format!("failed to parse JSONL event record: {error}"),
                });
                continue;
            }
        };

        let event_id = event.event_id.clone();
        let values = event_insert_values(&event)?;
        if rows.insert(event_id.clone(), values).is_some() {
            mismatches.push(SqliteParityMismatch {
                event_id: Some(event_id.clone()),
                field: "event_id".to_string(),
                jsonl_value: Some(event_id),
                sqlite_value: None,
                detail: "duplicate event_id in JSONL".to_string(),
            });
        }
    }

    Ok((rows, mismatches))
}

fn read_sqlite_rows(connection: &Connection) -> Result<SqliteMirrorRows> {
    let event_id_index = EVENT_INSERT_COLUMNS
        .iter()
        .position(|column| *column == "event_id")
        .ok_or_else(|| anyhow!("event_id column missing from insert column list"))?;
    let query = format!(
        "SELECT {} FROM {EVENTS_TABLE} ORDER BY run_id, sequence_global, event_id",
        EVENT_INSERT_COLUMNS.join(", ")
    );

    let mut statement = connection
        .prepare(&query)
        .context("failed to prepare sqlite parity query")?;
    let rows = statement
        .query_map([], |row| {
            let event_id = row.get::<usize, String>(event_id_index)?;
            let mut values = Vec::with_capacity(EVENT_INSERT_COLUMNS.len());
            for index in 0..EVENT_INSERT_COLUMNS.len() {
                values.push(row.get::<usize, SqlValue>(index)?);
            }
            Ok((event_id, values))
        })
        .context("failed to execute sqlite parity query")?;

    let mut mapped = BTreeMap::new();
    for row in rows {
        let (event_id, values) = row.context("failed to decode sqlite parity row")?;
        mapped.insert(event_id, values);
    }

    Ok(mapped)
}

fn sql_values_equal(left: &SqlValue, right: &SqlValue) -> bool {
    match (left, right) {
        (SqlValue::Null, SqlValue::Null) => true,
        (SqlValue::Integer(left), SqlValue::Integer(right)) => left == right,
        (SqlValue::Real(left), SqlValue::Real(right)) => (left - right).abs() <= f64::EPSILON,
        (SqlValue::Text(left), SqlValue::Text(right)) => left == right,
        (SqlValue::Blob(left), SqlValue::Blob(right)) => left == right,
        _ => false,
    }
}

fn format_sql_value(value: &SqlValue) -> String {
    match value {
        SqlValue::Null => "null".to_string(),
        SqlValue::Integer(value) => value.to_string(),
        SqlValue::Real(value) => format!("{value:.17}"),
        SqlValue::Text(value) => value.clone(),
        SqlValue::Blob(value) => format!("blob:{} bytes", value.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ADAPTERS_VIEW, EVENTS_TABLE, INGEST_RUNS_TABLE, INGEST_WATERMARKS_TABLE, QUALITY_VIEW,
        SCHEMA_META_TABLE, SESSIONS_VIEW, SQLITE_SCHEMA_VERSION, TOOL_CALLS_VIEW,
        ensure_sqlite_schema,
    };
    use rusqlite::{Connection, params};

    #[test]
    fn ensure_schema_creates_mart_tables() {
        let connection = Connection::open_in_memory().expect("in-memory sqlite should open");
        ensure_sqlite_schema(&connection).expect("schema creation should succeed");

        assert!(table_exists(&connection, EVENTS_TABLE));
        assert!(table_exists(&connection, INGEST_RUNS_TABLE));
        assert!(table_exists(&connection, INGEST_WATERMARKS_TABLE));
        assert!(table_exists(&connection, SCHEMA_META_TABLE));
        assert!(view_exists(&connection, TOOL_CALLS_VIEW));
        assert!(view_exists(&connection, SESSIONS_VIEW));
        assert!(view_exists(&connection, ADAPTERS_VIEW));
        assert!(view_exists(&connection, QUALITY_VIEW));
    }

    #[test]
    fn ensure_schema_is_idempotent_and_preserves_schema_version_metadata() {
        let connection = Connection::open_in_memory().expect("in-memory sqlite should open");
        ensure_sqlite_schema(&connection).expect("first schema ensure should succeed");
        ensure_sqlite_schema(&connection).expect("second schema ensure should succeed");

        let query = format!("SELECT COUNT(*) FROM {SCHEMA_META_TABLE} WHERE schema_version = ?1");
        let count = connection
            .query_row(&query, [SQLITE_SCHEMA_VERSION], |row| {
                row.get::<usize, i64>(0)
            })
            .expect("schema meta query should succeed");
        assert_eq!(count, 1);
    }

    #[test]
    fn ensure_schema_preserves_legacy_schema_versions_and_adds_current_version() {
        let connection = Connection::open_in_memory().expect("in-memory sqlite should open");
        connection
            .execute(
                &format!(
                    "CREATE TABLE {SCHEMA_META_TABLE} (schema_version TEXT NOT NULL, applied_at_utc TEXT NOT NULL)"
                ),
                [],
            )
            .expect("schema meta table should be creatable");
        connection
            .execute(
                &format!(
                    "INSERT INTO {SCHEMA_META_TABLE} (schema_version, applied_at_utc) VALUES (?1, ?2)"
                ),
                params!["agentlog.v1.sqlite.v0", "2026-01-01T00:00:00Z"],
            )
            .expect("legacy schema row should be insertable");

        ensure_sqlite_schema(&connection).expect("schema ensure should succeed");

        let query = format!("SELECT COUNT(*) FROM {SCHEMA_META_TABLE}");
        let total_rows = connection
            .query_row(&query, [], |row| row.get::<usize, i64>(0))
            .expect("schema meta count query should succeed");
        assert_eq!(
            total_rows, 2,
            "legacy and current schema versions should both be retained"
        );

        let current_query =
            format!("SELECT COUNT(*) FROM {SCHEMA_META_TABLE} WHERE schema_version = ?1");
        let current_rows = connection
            .query_row(&current_query, [SQLITE_SCHEMA_VERSION], |row| {
                row.get::<usize, i64>(0)
            })
            .expect("current schema count query should succeed");
        assert_eq!(current_rows, 1);
    }

    #[test]
    fn ensure_schema_keeps_existing_local_tables_and_data() {
        let connection = Connection::open_in_memory().expect("in-memory sqlite should open");
        connection
            .execute(
                "CREATE TABLE legacy_local_data (k TEXT NOT NULL PRIMARY KEY, v TEXT NOT NULL)",
                [],
            )
            .expect("legacy table should be creatable");
        connection
            .execute(
                "INSERT INTO legacy_local_data (k, v) VALUES (?1, ?2)",
                params!["row-1", "payload"],
            )
            .expect("legacy data should be insertable");

        ensure_sqlite_schema(&connection).expect("schema ensure should succeed");

        let preserved = connection
            .query_row(
                "SELECT v FROM legacy_local_data WHERE k = ?1",
                ["row-1"],
                |row| row.get::<usize, String>(0),
            )
            .expect("legacy row should remain after schema ensure");
        assert_eq!(preserved, "payload");
    }

    fn table_exists(connection: &Connection, table_name: &str) -> bool {
        connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
                [table_name],
                |_| Ok(()),
            )
            .is_ok()
    }

    fn view_exists(connection: &Connection, view_name: &str) -> bool {
        connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'view' AND name = ?1 LIMIT 1",
                [view_name],
                |_| Ok(()),
            )
            .is_ok()
    }
}
