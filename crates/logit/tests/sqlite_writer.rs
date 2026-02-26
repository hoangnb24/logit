use std::collections::BTreeMap;
use std::path::PathBuf;

use logit::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, SchemaVersion, TimestampQuality,
};
use logit::sqlite::{
    SCHEMA_META_TABLE, SqliteWriterConfig, ensure_sqlite_schema, open_sqlite_connection,
    write_events_batched, write_events_to_sqlite,
};
use rusqlite::Connection;

fn sample_event(event_id: &str, sequence_global: u64) -> AgentLogEvent {
    AgentLogEvent {
        schema_version: SchemaVersion::AgentLogV1,
        event_id: event_id.to_string(),
        run_id: "run-1".to_string(),
        sequence_global,
        sequence_source: Some(sequence_global),
        source_kind: AgentSource::Codex,
        source_path: "/tmp/events.jsonl".to_string(),
        source_record_locator: format!("line:{sequence_global}"),
        source_record_hash: Some(format!("source-{event_id}")),
        adapter_name: AgentSource::Codex,
        adapter_version: Some("v1".to_string()),
        record_format: RecordFormat::Message,
        event_type: EventType::Prompt,
        role: ActorRole::User,
        timestamp_utc: "2026-02-25T00:00:00Z".to_string(),
        timestamp_unix_ms: 1_771_977_600_000 + sequence_global,
        timestamp_quality: TimestampQuality::Exact,
        session_id: Some("session-1".to_string()),
        conversation_id: Some("conversation-1".to_string()),
        turn_id: Some(format!("turn-{sequence_global}")),
        parent_event_id: None,
        actor_id: Some("actor-1".to_string()),
        actor_name: Some("Agent".to_string()),
        provider: Some("openai".to_string()),
        model: Some("gpt-5".to_string()),
        content_text: Some(format!("hello {sequence_global}")),
        content_excerpt: Some(format!("hello {sequence_global}")),
        content_mime: Some("text/plain".to_string()),
        tool_name: None,
        tool_call_id: None,
        tool_arguments_json: None,
        tool_result_text: None,
        input_tokens: Some(1),
        output_tokens: Some(2),
        total_tokens: Some(3),
        cost_usd: Some(0.1),
        tags: vec!["tag".to_string()],
        flags: vec!["flag".to_string()],
        pii_redacted: Some(false),
        warnings: vec![],
        errors: vec![],
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

fn count_events(connection: &Connection) -> i64 {
    connection
        .query_row("SELECT COUNT(*) FROM agentlog_events", [], |row| row.get(0))
        .expect("count query should succeed")
}

#[test]
fn writes_events_in_batches_and_records_schema_meta() {
    let db_path = temp_db_path("sqlite-writer-batch");
    let events = (0..5)
        .map(|i| sample_event(&format!("evt-{i}"), i))
        .collect::<Vec<_>>();

    let stats = write_events_to_sqlite(&db_path, &events, SqliteWriterConfig { batch_size: 2 })
        .expect("sqlite writer should succeed");

    assert_eq!(stats.input_records, 5);
    assert_eq!(stats.records_written, 5);
    assert_eq!(stats.batches_committed, 3);

    let connection = open_sqlite_connection(&db_path).expect("db should reopen");
    assert_eq!(count_events(&connection), 5);
    let meta_count: i64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {SCHEMA_META_TABLE}"),
            [],
            |row| row.get(0),
        )
        .expect("meta count query should succeed");
    assert_eq!(meta_count, 1);
}

#[test]
fn duplicate_event_in_later_batch_rolls_back_only_that_batch() {
    let db_path = temp_db_path("sqlite-writer-rollback");
    let mut connection = open_sqlite_connection(&db_path).expect("db open should succeed");
    ensure_sqlite_schema(&connection).expect("schema init should succeed");

    let events = vec![
        sample_event("evt-1", 1),
        sample_event("evt-2", 2),
        sample_event("evt-1", 3),
    ];

    let error = write_events_batched(
        &mut connection,
        &events,
        SqliteWriterConfig { batch_size: 2 },
    )
    .expect_err("duplicate event_id should fail");
    let message = format!("{error:#}");
    assert!(message.contains("failed to insert event_id=evt-1"));

    assert_eq!(count_events(&connection), 2);
}
