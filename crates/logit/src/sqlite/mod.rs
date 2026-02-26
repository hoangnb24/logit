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

    let applied_at_utc = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format sqlite schema applied timestamp")?;
    connection
        .execute(&format!("DELETE FROM {SCHEMA_META_TABLE}"), [])
        .context("failed to reset sqlite schema meta rows")?;
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
    format!(
        "INSERT INTO {EVENTS_TABLE} ({}) VALUES ({placeholders})",
        EVENT_INSERT_COLUMNS.join(", ")
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
