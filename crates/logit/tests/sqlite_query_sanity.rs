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
