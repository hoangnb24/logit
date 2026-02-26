use logit::adapters::gemini::{parse_chat_session_json, parse_logs_json_array};
use logit::models::{ActorRole, EventType, RecordFormat, TimestampQuality};

#[test]
fn chat_unknown_and_missing_roles_map_to_runtime_with_warnings() {
    let input = r#"{
  "conversationId":"gemini-c-edge",
  "sessionId":"gemini-s-edge",
  "messages":[
    {"role":"auditor","timestamp":"2026-02-09T10:00:00Z","content":[{"text":"audit trace"}]},
    {"timestamp":"2026-02-09T10:00:01Z","content":[{"text":"missing role"}]}
  ]
}"#;

    let result = parse_chat_session_json(input, "run-edge", "fixtures/gemini/edge_chat.json")
        .expect(
            "chat parser should tolerate unknown/missing roles with explicit warning semantics",
        );

    assert_eq!(result.events.len(), 2);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("unknown role `auditor`"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("missing role"))
    );
    assert_eq!(result.events[0].record_format, RecordFormat::Diagnostic);
    assert_eq!(result.events[0].event_type, EventType::DebugLog);
    assert_eq!(result.events[0].role, ActorRole::Runtime);
    assert_eq!(
        result.events[0].conversation_id.as_deref(),
        Some("gemini-c-edge")
    );
    assert_eq!(
        result.events[0].session_id.as_deref(),
        Some("gemini-s-edge")
    );
    assert_eq!(result.events[1].record_format, RecordFormat::Diagnostic);
    assert_eq!(result.events[1].event_type, EventType::DebugLog);
    assert_eq!(result.events[1].role, ActorRole::Runtime);
}

#[test]
fn logs_classify_level_kind_and_role_precedence_explicitly() {
    let input = r#"[
  {"id":"log-warn","level":"warning","timestamp":"2026-02-09T11:00:00Z","message":"warn signal"},
  {"id":"log-art","kind":"artifact_reference","timestamp":"2026-02-09T11:00:01Z","message":"artifact item"},
  {"id":"log-metric","kind":"metric","timestamp":"2026-02-09T11:00:02Z","message":"latency=45"},
  {"id":"log-tool","role":"tool","timestamp":"2026-02-09T11:00:03Z","message":"tool output"},
  {"id":"log-system","actor":"system","time":"2026-02-09T11:00:04Z","message":"sys notice"},
  {"level":"error","role":" ","timestamp":"2026-02-09T11:00:05Z","message":"empty-role fallback"}
]"#;

    let result = parse_logs_json_array(input, "run-edge", "fixtures/gemini/edge_logs.json")
        .expect("logs parser should succeed");

    assert_eq!(result.events.len(), 6);
    assert!(result.warnings.is_empty());

    assert_eq!(result.events[0].event_type, EventType::StatusUpdate);
    assert_eq!(result.events[1].event_type, EventType::ArtifactReference);
    assert_eq!(result.events[2].event_type, EventType::Metric);
    assert_eq!(result.events[3].record_format, RecordFormat::ToolResult);
    assert_eq!(result.events[3].role, ActorRole::Tool);
    assert_eq!(result.events[4].record_format, RecordFormat::System);
    assert_eq!(result.events[4].event_type, EventType::SystemNotice);
    assert_eq!(result.events[4].role, ActorRole::System);
    assert_eq!(result.events[5].event_type, EventType::Error);
    assert_eq!(result.events[5].role, ActorRole::Runtime);
    assert_eq!(result.events[5].event_id, "gemini-log-000006");
}

#[test]
fn logs_missing_timestamp_and_blank_id_use_fallbacks() {
    let input = r#"[{"id":"   ","timestamp":"bad-ts","message":"hello"},{"message":"still here"}]"#;
    let result = parse_logs_json_array(input, "run-edge", "fixtures/gemini/edge_sparse.json")
        .expect("logs parser should succeed");

    assert_eq!(result.events.len(), 2);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid timestamp"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("missing timestamp"))
    );
    assert_eq!(result.events[0].event_id, "gemini-log-000001");
    assert_eq!(
        result.events[0].timestamp_quality,
        TimestampQuality::Fallback
    );
    assert_eq!(result.events[1].event_id, "gemini-log-000002");
    assert_eq!(
        result.events[1].timestamp_quality,
        TimestampQuality::Fallback
    );
}
