use logit::adapters::gemini::{parse_logs_file, parse_logs_json_array};
use logit::models::{ActorRole, EventType, RecordFormat, TimestampQuality};

#[test]
fn parses_empty_logs_array_without_errors() {
    let result =
        parse_logs_json_array("[]", "run-1", "fixtures/gemini/logs.json").expect("parse succeeds");

    assert!(result.events.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn parses_sparse_log_entries_with_fallbacks_and_warnings() {
    let input = r#"
[
  {
    "id": "log-1",
    "role": "user",
    "timestamp": "2026-02-03T08:30:00Z",
    "message": "Start diagnostics",
    "conversation_id": "gemini-c-001"
  },
  {
    "id": "log-2",
    "role": "model",
    "message": "Ready"
  },
  null,
  {
    "id": "log-3",
    "level": "error",
    "message": "stack trace"
  },
  {
    "id": "log-4",
    "kind": "metric",
    "timestamp": "bad-ts",
    "message": "latency=123"
  }
]
"#;

    let result =
        parse_logs_json_array(input, "run-1", "fixtures/gemini/logs.json").expect("parse succeeds");

    assert_eq!(result.events.len(), 4);
    assert_eq!(result.warnings.len(), 4);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("entry is not an object"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("missing timestamp"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid timestamp"))
    );

    let first = &result.events[0];
    assert_eq!(first.record_format, RecordFormat::Message);
    assert_eq!(first.event_type, EventType::Prompt);
    assert_eq!(first.role, ActorRole::User);
    assert_eq!(first.timestamp_quality, TimestampQuality::Exact);
    assert_eq!(first.conversation_id.as_deref(), Some("gemini-c-001"));

    let second = &result.events[1];
    assert_eq!(second.record_format, RecordFormat::Message);
    assert_eq!(second.event_type, EventType::Response);
    assert_eq!(second.role, ActorRole::Assistant);
    assert_eq!(second.timestamp_quality, TimestampQuality::Fallback);

    let third = &result.events[2];
    assert_eq!(third.record_format, RecordFormat::Diagnostic);
    assert_eq!(third.event_type, EventType::Error);
    assert_eq!(third.role, ActorRole::Runtime);
}

#[test]
fn parses_logs_file_from_disk() {
    let file = std::env::temp_dir().join(format!(
        "logit-gemini-logs-{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is valid")
            .as_nanos()
    ));
    std::fs::write(
        &file,
        r#"[{"id":"file-1","timestamp":"2026-02-03T08:30:00Z","message":"ok"}]"#,
    )
    .expect("fixture write should succeed");

    let result = parse_logs_file(&file, "run-file").expect("file parse should succeed");
    assert_eq!(result.events.len(), 1);
    assert!(result.warnings.is_empty());
    assert_eq!(result.events[0].event_id, "file-1");
}
