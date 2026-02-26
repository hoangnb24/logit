use std::collections::BTreeMap;
use std::path::PathBuf;

use logit::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, SchemaVersion, TimestampQuality,
};
use logit::sqlite::{SqliteWriterConfig, open_sqlite_connection, write_events_to_sqlite};
use rusqlite::Connection;

fn sample_event(
    event_id: &str,
    run_id: &str,
    sequence_global: u64,
    session_id: &str,
    adapter_name: AgentSource,
    event_type: EventType,
    timestamp_unix_ms: u64,
) -> AgentLogEvent {
    AgentLogEvent {
        schema_version: SchemaVersion::AgentLogV1,
        event_id: event_id.to_string(),
        run_id: run_id.to_string(),
        sequence_global,
        sequence_source: Some(sequence_global),
        source_kind: adapter_name,
        source_path: "/tmp/events.jsonl".to_string(),
        source_record_locator: format!("line:{sequence_global}"),
        source_record_hash: None,
        adapter_name,
        adapter_version: Some("v1".to_string()),
        record_format: RecordFormat::Message,
        event_type,
        role: ActorRole::User,
        timestamp_utc: "2026-02-25T00:00:00Z".to_string(),
        timestamp_unix_ms,
        timestamp_quality: TimestampQuality::Exact,
        session_id: Some(session_id.to_string()),
        conversation_id: Some(format!("conversation-{session_id}")),
        turn_id: Some(format!("turn-{sequence_global}")),
        parent_event_id: None,
        actor_id: None,
        actor_name: None,
        provider: None,
        model: None,
        content_text: Some(format!("payload-{event_id}")),
        content_excerpt: Some(format!("payload-{event_id}")),
        content_mime: Some("text/plain".to_string()),
        tool_name: None,
        tool_call_id: None,
        tool_arguments_json: None,
        tool_result_text: None,
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        cost_usd: None,
        tags: Vec::new(),
        flags: Vec::new(),
        pii_redacted: Some(false),
        warnings: Vec::new(),
        errors: Vec::new(),
        raw_hash: format!("raw-{event_id}"),
        canonical_hash: format!("canonical-{event_id}"),
        metadata: BTreeMap::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn sample_tool_event(
    event_id: &str,
    run_id: &str,
    sequence_global: u64,
    session_id: &str,
    adapter_name: AgentSource,
    record_format: RecordFormat,
    event_type: EventType,
    role: ActorRole,
    timestamp_unix_ms: u64,
    tool_name: &str,
    tool_call_id: &str,
) -> AgentLogEvent {
    let mut event = sample_event(
        event_id,
        run_id,
        sequence_global,
        session_id,
        adapter_name,
        event_type,
        timestamp_unix_ms,
    );
    event.record_format = record_format;
    event.role = role;
    event.tool_name = Some(tool_name.to_string());
    event.tool_call_id = Some(tool_call_id.to_string());
    event
}

fn temp_db_path(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("logit-{label}-{nanos}.sqlite"))
}

fn query_plan_details(connection: &Connection, query: &str) -> Vec<String> {
    let explain = format!("EXPLAIN QUERY PLAN {query}");
    let mut statement = connection
        .prepare(&explain)
        .expect("explain query should prepare");
    statement
        .query_map([], |row| row.get::<usize, String>(3))
        .expect("explain query should execute")
        .map(|detail| detail.expect("detail column should decode"))
        .collect()
}

#[test]
fn sqlite_queries_return_expected_rows_for_common_filters() {
    let db_path = temp_db_path("sqlite-query-sanity-rows");
    let events = vec![
        sample_event(
            "evt-a-0",
            "run-a",
            0,
            "session-1",
            AgentSource::Codex,
            EventType::Prompt,
            1_000,
        ),
        sample_event(
            "evt-a-1",
            "run-a",
            1,
            "session-1",
            AgentSource::Codex,
            EventType::Response,
            1_001,
        ),
        sample_event(
            "evt-a-2",
            "run-a",
            2,
            "session-2",
            AgentSource::Claude,
            EventType::Prompt,
            1_002,
        ),
        sample_event(
            "evt-b-0",
            "run-b",
            0,
            "session-1",
            AgentSource::Codex,
            EventType::Prompt,
            2_000,
        ),
    ];

    write_events_to_sqlite(&db_path, &events, SqliteWriterConfig { batch_size: 2 })
        .expect("sqlite writer should succeed");
    let connection = open_sqlite_connection(&db_path).expect("db should reopen");

    let run_rows = {
        let mut statement = connection
            .prepare(
                "SELECT event_id FROM agentlog_events WHERE run_id = ?1 ORDER BY sequence_global",
            )
            .expect("run sequence query should prepare");
        statement
            .query_map(["run-a"], |row| row.get::<usize, String>(0))
            .expect("run sequence query should execute")
            .map(|row| row.expect("event id row should decode"))
            .collect::<Vec<_>>()
    };
    assert_eq!(run_rows, vec!["evt-a-0", "evt-a-1", "evt-a-2"]);

    let codex_prompt_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM agentlog_events WHERE adapter_name = ?1 AND event_type = ?2",
            ["codex", "prompt"],
            |row| row.get(0),
        )
        .expect("adapter/event count query should succeed");
    assert_eq!(codex_prompt_count, 2);

    let session_rows = {
        let mut statement = connection
            .prepare(
                "SELECT event_id FROM agentlog_events WHERE session_id = ?1 ORDER BY timestamp_unix_ms",
            )
            .expect("session timeline query should prepare");
        statement
            .query_map(["session-1"], |row| row.get::<usize, String>(0))
            .expect("session timeline query should execute")
            .map(|row| row.expect("event id row should decode"))
            .collect::<Vec<_>>()
    };
    assert_eq!(session_rows, vec!["evt-a-0", "evt-a-1", "evt-b-0"]);
}

#[test]
fn sqlite_query_plans_use_expected_indexes() {
    let db_path = temp_db_path("sqlite-query-sanity-plan");
    let events = vec![
        sample_event(
            "evt-a-0",
            "run-a",
            0,
            "session-1",
            AgentSource::Codex,
            EventType::Prompt,
            1_000,
        ),
        sample_event(
            "evt-a-1",
            "run-a",
            1,
            "session-1",
            AgentSource::Codex,
            EventType::Response,
            1_001,
        ),
        sample_event(
            "evt-b-0",
            "run-b",
            0,
            "session-1",
            AgentSource::Codex,
            EventType::Prompt,
            2_000,
        ),
    ];

    write_events_to_sqlite(&db_path, &events, SqliteWriterConfig { batch_size: 2 })
        .expect("sqlite writer should succeed");
    let connection = open_sqlite_connection(&db_path).expect("db should reopen");

    let run_plan = query_plan_details(
        &connection,
        "SELECT event_id FROM agentlog_events WHERE run_id = 'run-a' ORDER BY sequence_global",
    );
    assert!(
        run_plan
            .iter()
            .any(|detail| detail.contains("idx_agentlog_events_run_sequence"))
    );

    let adapter_event_plan = query_plan_details(
        &connection,
        "SELECT event_id FROM agentlog_events WHERE adapter_name = 'codex' AND event_type = 'prompt'",
    );
    assert!(
        adapter_event_plan
            .iter()
            .any(|detail| detail.contains("idx_agentlog_events_adapter_event"))
    );

    let session_plan = query_plan_details(
        &connection,
        "SELECT event_id FROM agentlog_events WHERE session_id = 'session-1' ORDER BY timestamp_unix_ms",
    );
    assert!(
        session_plan
            .iter()
            .any(|detail| detail.contains("idx_agentlog_events_session_time"))
    );
}

#[test]
fn sqlite_writer_upserts_by_event_id_for_idempotent_replays() {
    let db_path = temp_db_path("sqlite-idempotent-upsert");
    let original = sample_event(
        "evt-upsert-1",
        "run-a",
        0,
        "session-1",
        AgentSource::Codex,
        EventType::Prompt,
        1_000,
    );

    write_events_to_sqlite(
        &db_path,
        std::slice::from_ref(&original),
        SqliteWriterConfig { batch_size: 1 },
    )
    .expect("initial sqlite write should succeed");
    write_events_to_sqlite(
        &db_path,
        std::slice::from_ref(&original),
        SqliteWriterConfig { batch_size: 1 },
    )
    .expect("replay sqlite write should succeed");

    let mut updated = original.clone();
    updated.content_text = Some("payload-updated".to_string());
    updated.content_excerpt = Some("payload-updated".to_string());
    updated.event_type = EventType::Response;
    updated.timestamp_unix_ms = 1_005;

    write_events_to_sqlite(&db_path, &[updated], SqliteWriterConfig { batch_size: 1 })
        .expect("upsert update write should succeed");

    let connection = open_sqlite_connection(&db_path).expect("db should reopen");
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM agentlog_events", [], |row| row.get(0))
        .expect("event count query should succeed");
    assert_eq!(count, 1, "upsert should prevent duplicate event rows");

    let row = connection
        .query_row(
            "SELECT event_type, content_text, timestamp_unix_ms
             FROM agentlog_events
             WHERE event_id = 'evt-upsert-1'",
            [],
            |row| {
                Ok((
                    row.get::<usize, String>(0)?,
                    row.get::<usize, Option<String>>(1)?,
                    row.get::<usize, i64>(2)?,
                ))
            },
        )
        .expect("upserted row query should succeed");
    assert_eq!(row.0, "response");
    assert_eq!(row.1.as_deref(), Some("payload-updated"));
    assert_eq!(row.2, 1_005);
}

#[test]
fn sqlite_semantic_views_surface_tool_and_session_rollups() {
    let db_path = temp_db_path("sqlite-semantic-views");
    let events = vec![
        sample_event(
            "evt-msg-1",
            "run-a",
            0,
            "session-1",
            AgentSource::Codex,
            EventType::Prompt,
            1_000,
        ),
        sample_event(
            "evt-msg-2",
            "run-a",
            1,
            "session-1",
            AgentSource::Codex,
            EventType::Response,
            1_010,
        ),
        sample_tool_event(
            "evt-tc-1",
            "run-a",
            2,
            "session-1",
            AgentSource::Codex,
            RecordFormat::ToolCall,
            EventType::ToolInvocation,
            ActorRole::Assistant,
            1_100,
            "shell",
            "tc-1",
        ),
        sample_tool_event(
            "evt-tr-1",
            "run-a",
            3,
            "session-1",
            AgentSource::Codex,
            RecordFormat::ToolResult,
            EventType::ToolOutput,
            ActorRole::Tool,
            1_250,
            "shell",
            "tc-1",
        ),
        sample_tool_event(
            "evt-tc-2",
            "run-a",
            4,
            "session-1",
            AgentSource::Codex,
            RecordFormat::ToolCall,
            EventType::ToolInvocation,
            ActorRole::Assistant,
            1_300,
            "grep",
            "tc-2",
        ),
        sample_tool_event(
            "evt-tr-3",
            "run-a",
            5,
            "session-1",
            AgentSource::Codex,
            RecordFormat::ToolResult,
            EventType::ToolOutput,
            ActorRole::Tool,
            1_400,
            "ls",
            "tc-3",
        ),
        sample_event(
            "evt-err-1",
            "run-a",
            6,
            "session-1",
            AgentSource::Codex,
            EventType::Error,
            1_410,
        ),
        sample_event(
            "evt-s2-1",
            "run-a",
            7,
            "session-2",
            AgentSource::Claude,
            EventType::Prompt,
            2_000,
        ),
    ];

    write_events_to_sqlite(&db_path, &events, SqliteWriterConfig { batch_size: 2 })
        .expect("sqlite writer should succeed");
    let connection = open_sqlite_connection(&db_path).expect("db should reopen");

    let tool_rows = {
        let mut statement = connection
            .prepare(
                "SELECT tool_call_id, pairing_status, call_event_id, result_event_id, duration_ms, duration_source, duration_quality
                 FROM v_tool_calls
                 ORDER BY tool_call_id, pairing_status",
            )
            .expect("tool view query should prepare");
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<usize, Option<String>>(0)?,
                    row.get::<usize, String>(1)?,
                    row.get::<usize, Option<String>>(2)?,
                    row.get::<usize, Option<String>>(3)?,
                    row.get::<usize, Option<i64>>(4)?,
                    row.get::<usize, Option<String>>(5)?,
                    row.get::<usize, Option<String>>(6)?,
                ))
            })
            .expect("tool view query should execute")
            .map(|row| row.expect("tool view row should decode"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        tool_rows,
        vec![
            (
                Some("tc-1".to_string()),
                "paired".to_string(),
                Some("evt-tc-1".to_string()),
                Some("evt-tr-1".to_string()),
                Some(150),
                Some("paired".to_string()),
                Some("medium".to_string()),
            ),
            (
                Some("tc-2".to_string()),
                "missing_result".to_string(),
                Some("evt-tc-2".to_string()),
                None,
                None,
                None,
                None,
            ),
            (
                Some("tc-3".to_string()),
                "orphan_result".to_string(),
                None,
                Some("evt-tr-3".to_string()),
                None,
                None,
                None,
            ),
        ]
    );

    let session_rows = {
        let mut statement = connection
            .prepare(
                "SELECT run_id, session_id, event_count, first_event_timestamp_unix_ms, last_event_timestamp_unix_ms, duration_ms,
                        tool_call_count, tool_result_count, prompt_count, response_count, error_count, distinct_tool_count
                 FROM v_sessions
                 ORDER BY run_id, session_id",
            )
            .expect("session view query should prepare");
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<usize, String>(0)?,
                    row.get::<usize, String>(1)?,
                    row.get::<usize, i64>(2)?,
                    row.get::<usize, i64>(3)?,
                    row.get::<usize, i64>(4)?,
                    row.get::<usize, i64>(5)?,
                    row.get::<usize, i64>(6)?,
                    row.get::<usize, i64>(7)?,
                    row.get::<usize, i64>(8)?,
                    row.get::<usize, i64>(9)?,
                    row.get::<usize, i64>(10)?,
                    row.get::<usize, i64>(11)?,
                ))
            })
            .expect("session view query should execute")
            .map(|row| row.expect("session view row should decode"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        session_rows,
        vec![
            (
                "run-a".to_string(),
                "session-1".to_string(),
                7,
                1_000,
                1_410,
                410,
                2,
                2,
                1,
                1,
                1,
                3,
            ),
            (
                "run-a".to_string(),
                "session-2".to_string(),
                1,
                2_000,
                2_000,
                0,
                0,
                0,
                1,
                0,
                0,
                0,
            ),
        ]
    );
}

#[test]
fn sqlite_adapter_and_quality_views_surface_expected_rollups() {
    let db_path = temp_db_path("sqlite-adapter-quality-views");

    let codex_prompt = sample_event(
        "evt-aq-1",
        "run-a",
        0,
        "session-1",
        AgentSource::Codex,
        EventType::Prompt,
        1_000,
    );

    let mut codex_response = sample_event(
        "evt-aq-2",
        "run-a",
        1,
        "session-1",
        AgentSource::Codex,
        EventType::Response,
        1_050,
    );
    codex_response.timestamp_quality = TimestampQuality::Derived;

    let mut codex_tool_call = sample_tool_event(
        "evt-aq-3",
        "run-a",
        2,
        "session-1",
        AgentSource::Codex,
        RecordFormat::ToolCall,
        EventType::ToolInvocation,
        ActorRole::Assistant,
        1_100,
        "shell",
        "tc-aq-1",
    );
    codex_tool_call.warnings = vec!["tool-warning".to_string()];

    let mut codex_tool_result = sample_tool_event(
        "evt-aq-4",
        "run-a",
        3,
        "session-1",
        AgentSource::Codex,
        RecordFormat::ToolResult,
        EventType::ToolOutput,
        ActorRole::Tool,
        1_180,
        "shell",
        "tc-aq-1",
    );
    codex_tool_result.timestamp_quality = TimestampQuality::Fallback;
    codex_tool_result.errors = vec!["tool-error".to_string()];
    codex_tool_result.flags = vec!["redacted".to_string()];
    codex_tool_result.pii_redacted = Some(true);

    let mut codex_error = sample_event(
        "evt-aq-5",
        "run-a",
        4,
        "session-2",
        AgentSource::Codex,
        EventType::Error,
        1_220,
    );
    codex_error.timestamp_quality = TimestampQuality::Fallback;
    codex_error.warnings = vec!["clock-skew".to_string()];
    codex_error.errors = vec!["parse-failure".to_string()];

    let claude_prompt = sample_event(
        "evt-aq-6",
        "run-a",
        5,
        "session-9",
        AgentSource::Claude,
        EventType::Prompt,
        1_300,
    );

    let events = vec![
        codex_prompt,
        codex_response,
        codex_tool_call,
        codex_tool_result,
        codex_error,
        claude_prompt,
    ];

    write_events_to_sqlite(&db_path, &events, SqliteWriterConfig { batch_size: 2 })
        .expect("sqlite writer should succeed");
    let connection = open_sqlite_connection(&db_path).expect("db should reopen");

    let adapter_rows = {
        let mut statement = connection
            .prepare(
                "SELECT adapter_name, event_count, run_count, session_count, prompt_count, response_count,
                        tool_call_count, tool_result_count, error_event_count, warning_record_count,
                        error_record_count, pii_redacted_count
                 FROM v_adapters
                 ORDER BY adapter_name",
            )
            .expect("adapter view query should prepare");
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<usize, String>(0)?,
                    row.get::<usize, i64>(1)?,
                    row.get::<usize, i64>(2)?,
                    row.get::<usize, i64>(3)?,
                    row.get::<usize, i64>(4)?,
                    row.get::<usize, i64>(5)?,
                    row.get::<usize, i64>(6)?,
                    row.get::<usize, i64>(7)?,
                    row.get::<usize, i64>(8)?,
                    row.get::<usize, i64>(9)?,
                    row.get::<usize, i64>(10)?,
                    row.get::<usize, i64>(11)?,
                ))
            })
            .expect("adapter view query should execute")
            .map(|row| row.expect("adapter row should decode"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        adapter_rows,
        vec![
            ("claude".to_string(), 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0),
            ("codex".to_string(), 5, 1, 2, 1, 1, 1, 1, 1, 2, 2, 1),
        ]
    );

    let quality_rows = {
        let mut statement = connection
            .prepare(
                "SELECT adapter_name, timestamp_quality, event_count, warning_record_count,
                        error_record_count, flagged_record_count, pii_redacted_count
                 FROM v_quality
                 ORDER BY adapter_name, timestamp_quality",
            )
            .expect("quality view query should prepare");
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<usize, String>(0)?,
                    row.get::<usize, String>(1)?,
                    row.get::<usize, i64>(2)?,
                    row.get::<usize, i64>(3)?,
                    row.get::<usize, i64>(4)?,
                    row.get::<usize, i64>(5)?,
                    row.get::<usize, i64>(6)?,
                ))
            })
            .expect("quality view query should execute")
            .map(|row| row.expect("quality row should decode"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        quality_rows,
        vec![
            ("claude".to_string(), "exact".to_string(), 1, 0, 0, 0, 0),
            ("codex".to_string(), "derived".to_string(), 1, 0, 0, 0, 0),
            ("codex".to_string(), "exact".to_string(), 2, 1, 0, 0, 0),
            ("codex".to_string(), "fallback".to_string(), 2, 1, 2, 1, 1),
        ]
    );
}
