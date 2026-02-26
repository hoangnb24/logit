use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use logit::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, SchemaVersion, TimestampQuality,
};
use logit::normalize::write_events_artifact;
use logit::sqlite::{
    SqliteWriterConfig, open_sqlite_connection, verify_jsonl_sqlite_parity, write_events_to_sqlite,
};

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
        warnings: Vec::new(),
        errors: Vec::new(),
        raw_hash: format!("raw-{event_id}"),
        canonical_hash: format!("canonical-{event_id}"),
        metadata: BTreeMap::new(),
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

fn write_jsonl(path: &Path, events: &[AgentLogEvent]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("jsonl parent should be creatable");
    }
    write_events_artifact(path, events).expect("jsonl artifact write should succeed");
}

#[test]
fn parity_report_passes_for_identical_jsonl_and_sqlite() {
    let temp_dir = unique_temp_dir("logit-sqlite-parity-pass");
    let jsonl_path = temp_dir.join("events.jsonl");
    let sqlite_path = temp_dir.join("events.sqlite");
    let events = vec![sample_event("evt-1", 1), sample_event("evt-2", 2)];

    write_jsonl(&jsonl_path, &events);
    write_events_to_sqlite(&sqlite_path, &events, SqliteWriterConfig { batch_size: 1 })
        .expect("sqlite mirror write should succeed");

    let report =
        verify_jsonl_sqlite_parity(&jsonl_path, &sqlite_path).expect("parity check should succeed");
    assert!(report.is_match(), "expected parity report to match");
    assert_eq!(report.jsonl_records, 2);
    assert_eq!(report.sqlite_records, 2);
    assert_eq!(report.compared_records, 2);
    assert!(report.mismatches.is_empty());
}

#[test]
fn parity_report_detects_count_and_field_mismatches() {
    let temp_dir = unique_temp_dir("logit-sqlite-parity-fail");
    let jsonl_path = temp_dir.join("events.jsonl");
    let sqlite_path = temp_dir.join("events.sqlite");
    let events = vec![sample_event("evt-1", 1), sample_event("evt-2", 2)];

    write_jsonl(&jsonl_path, &events);
    write_events_to_sqlite(
        &sqlite_path,
        &[events[0].clone()],
        SqliteWriterConfig { batch_size: 1 },
    )
    .expect("sqlite mirror write should succeed");

    let connection = open_sqlite_connection(&sqlite_path).expect("sqlite should reopen");
    connection
        .execute(
            "UPDATE agentlog_events SET canonical_hash = 'mismatch-hash' WHERE event_id = 'evt-1'",
            [],
        )
        .expect("update should succeed");

    let report =
        verify_jsonl_sqlite_parity(&jsonl_path, &sqlite_path).expect("parity check should succeed");
    assert!(!report.is_match(), "expected parity mismatches");

    let has_count_mismatch = report
        .mismatches
        .iter()
        .any(|mismatch| mismatch.field == "record_count");
    let has_canonical_hash_mismatch = report.mismatches.iter().any(|mismatch| {
        mismatch.event_id.as_deref() == Some("evt-1") && mismatch.field == "canonical_hash"
    });
    let has_missing_event = report.mismatches.iter().any(|mismatch| {
        mismatch.event_id.as_deref() == Some("evt-2")
            && mismatch.detail.contains("missing from SQLite")
    });

    assert!(has_count_mismatch, "expected record_count mismatch");
    assert!(
        has_canonical_hash_mismatch,
        "expected canonical_hash mismatch for evt-1"
    );
    assert!(
        has_missing_event,
        "expected missing event mismatch for evt-2"
    );
}
