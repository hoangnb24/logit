use logit::adapters::gemini::{parse_chat_session_file, parse_chat_session_json};
use logit::models::{ActorRole, EventType, RecordFormat, TimestampQuality};
use serde_json::Value;

#[test]
fn parses_chat_messages_fixture_to_canonical_events() {
    let input = include_str!("../../../fixtures/gemini/chat_messages.json");
    let result = parse_chat_session_json(input, "run-1", "fixtures/gemini/chat_messages.json")
        .expect("chat parse should succeed");

    assert_eq!(result.events.len(), 2);
    assert!(result.warnings.is_empty());

    let first = &result.events[0];
    assert_eq!(first.record_format, RecordFormat::Message);
    assert_eq!(first.event_type, EventType::Prompt);
    assert_eq!(first.role, ActorRole::User);
    assert_eq!(first.timestamp_quality, TimestampQuality::Exact);
    assert_eq!(first.source_record_locator, "messages:1");
    assert_eq!(first.conversation_id.as_deref(), Some("gemini-c-001"));
    assert_eq!(
        first.metadata.get("gemini_conversation_id_source"),
        Some(&Value::String("root".to_string()))
    );
    assert_eq!(
        first.metadata.get("gemini_root_conversation_id"),
        Some(&Value::String("gemini-c-001".to_string()))
    );
    assert!(
        first
            .content_text
            .as_deref()
            .is_some_and(|text| text.contains("Show me the diff summary."))
    );

    let second = &result.events[1];
    assert_eq!(second.record_format, RecordFormat::Message);
    assert_eq!(second.event_type, EventType::Response);
    assert_eq!(second.role, ActorRole::Assistant);
    assert_eq!(second.source_record_locator, "messages:2");
    assert_eq!(second.conversation_id.as_deref(), Some("gemini-c-001"));
    assert_eq!(
        second.metadata.get("gemini_conversation_id_source"),
        Some(&Value::String("root".to_string()))
    );
}

#[test]
fn handles_content_variants_and_sparse_message_data() {
    let input = include_str!("../../../fixtures/gemini/content_variants.json");
    let result = parse_chat_session_json(
        input,
        "run-variants",
        "fixtures/gemini/content_variants.json",
    )
    .expect("chat parse should succeed");

    assert_eq!(result.events.len(), 3);
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
            .any(|warning| warning.contains("missing content text"))
    );

    let first = &result.events[0];
    assert_eq!(first.event_type, EventType::Prompt);
    assert!(
        first
            .content_text
            .as_deref()
            .is_some_and(|text| text.contains("Analyze attached diagram."))
    );
    assert_eq!(
        first.metadata.get("gemini_content_source"),
        Some(&Value::String("content".to_string()))
    );
    assert_eq!(
        first.metadata.get("gemini_content_parts"),
        Some(&Value::from(2))
    );

    let second = &result.events[1];
    assert_eq!(second.event_type, EventType::Response);
    assert_eq!(second.source_record_locator, "messages:2");

    let third = &result.events[2];
    assert_eq!(third.event_type, EventType::Response);
    assert_eq!(third.timestamp_quality, TimestampQuality::Fallback);
}

#[test]
fn chat_parser_extracts_heterogeneous_message_content_containers() {
    let input = r#"{
  "conversation_id":"gemini-c-mixed",
  "messages":[
    {"role":"user","timestamp":"2026-02-10T08:00:00Z","response":{"parts":[{"text":"response parts text"}]}},
    {"role":"model","timestamp":"2026-02-10T08:00:01Z","payload":{"message":{"text":"payload nested message"}}},
    {"role":"model","timestamp":"2026-02-10T08:00:02Z","candidates":[{"content":[{"text":"candidate one"}]},{"content":{"text":"candidate two"}}]},
    {"role":"model","timestamp":"2026-02-10T08:00:03Z","metadata":{"status":"done"}}
  ]
}"#;

    let result = parse_chat_session_json(input, "run-mixed", "fixtures/gemini/mixed.json")
        .expect("chat parse should succeed");

    assert_eq!(result.events.len(), 4);
    assert_eq!(
        result
            .warnings
            .iter()
            .filter(|warning| warning.contains("missing content text"))
            .count(),
        1
    );

    assert_eq!(
        result.events[0].content_text.as_deref(),
        Some("response parts text")
    );
    assert_eq!(
        result.events[0].metadata.get("gemini_content_source"),
        Some(&Value::String("response".to_string()))
    );
    assert_eq!(
        result.events[1].content_text.as_deref(),
        Some("payload nested message")
    );
    assert_eq!(
        result.events[1].metadata.get("gemini_content_source"),
        Some(&Value::String("payload".to_string()))
    );
    assert_eq!(
        result.events[2].content_text.as_deref(),
        Some("candidate one\ncandidate two")
    );
    assert_eq!(
        result.events[2].metadata.get("gemini_content_source"),
        Some(&Value::String("candidates".to_string()))
    );
    assert!(result.events[3].content_text.is_none());
}

#[test]
fn chat_parser_propagates_root_and_message_session_context() {
    let input = r#"{
  "conversation_id":"gemini-c-root",
  "session_id":"gemini-s-root",
  "model":"gemini-2.0-pro",
  "messages":[
    {"role":"user","timestamp":"2026-02-10T09:00:00Z","content":[{"text":"root context"}]},
    {"role":"model","timestamp":"2026-02-10T09:00:01Z","conversationId":"gemini-c-msg","sessionId":"gemini-s-msg","model":"gemini-2.1-flash","content":[{"text":"message override"}]}
  ]
}"#;

    let result = parse_chat_session_json(input, "run-context", "fixtures/gemini/context.json")
        .expect("chat parse should succeed");

    assert_eq!(result.events.len(), 2);
    assert!(result.warnings.is_empty());

    let first = &result.events[0];
    assert_eq!(first.conversation_id.as_deref(), Some("gemini-c-root"));
    assert_eq!(first.session_id.as_deref(), Some("gemini-s-root"));
    assert_eq!(first.model.as_deref(), Some("gemini-2.0-pro"));
    assert_eq!(
        first.metadata.get("gemini_conversation_id_source"),
        Some(&Value::String("root".to_string()))
    );
    assert_eq!(
        first.metadata.get("gemini_session_id_source"),
        Some(&Value::String("root".to_string()))
    );
    assert_eq!(
        first.metadata.get("gemini_model_source"),
        Some(&Value::String("root".to_string()))
    );
    assert_eq!(
        first.metadata.get("gemini_root_conversation_id"),
        Some(&Value::String("gemini-c-root".to_string()))
    );
    assert_eq!(
        first.metadata.get("gemini_root_session_id"),
        Some(&Value::String("gemini-s-root".to_string()))
    );
    assert_eq!(
        first.metadata.get("gemini_root_model"),
        Some(&Value::String("gemini-2.0-pro".to_string()))
    );

    let second = &result.events[1];
    assert_eq!(second.conversation_id.as_deref(), Some("gemini-c-msg"));
    assert_eq!(second.session_id.as_deref(), Some("gemini-s-msg"));
    assert_eq!(second.model.as_deref(), Some("gemini-2.1-flash"));
    assert_eq!(
        second.metadata.get("gemini_conversation_id_source"),
        Some(&Value::String("message".to_string()))
    );
    assert_eq!(
        second.metadata.get("gemini_session_id_source"),
        Some(&Value::String("message".to_string()))
    );
    assert_eq!(
        second.metadata.get("gemini_model_source"),
        Some(&Value::String("message".to_string()))
    );
    assert_eq!(
        second.metadata.get("gemini_root_conversation_id"),
        Some(&Value::String("gemini-c-root".to_string()))
    );
    assert_eq!(
        second.metadata.get("gemini_root_session_id"),
        Some(&Value::String("gemini-s-root".to_string()))
    );
    assert_eq!(
        second.metadata.get("gemini_root_model"),
        Some(&Value::String("gemini-2.0-pro".to_string()))
    );
}

#[test]
fn chat_parser_rejects_payload_without_messages_array() {
    let err = parse_chat_session_json(
        r#"{"conversation_id":"gemini-c-001"}"#,
        "run-1",
        "fixtures/gemini/invalid.json",
    )
    .expect_err("missing messages array should fail");
    assert!(err.to_string().contains("messages"));
}

#[test]
fn parses_chat_file_from_disk() {
    let file = std::env::temp_dir().join(format!(
        "logit-gemini-chat-{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock is valid")
            .as_nanos()
    ));
    std::fs::write(
        &file,
        r#"{
  "conversation_id":"gemini-c-file",
  "messages":[{"role":"user","timestamp":"2026-02-03T08:30:00Z","content":[{"text":"ok"}]}]
}"#,
    )
    .expect("fixture write should succeed");

    let result = parse_chat_session_file(&file, "run-file").expect("file parse should succeed");
    assert_eq!(result.events.len(), 1);
    assert!(result.warnings.is_empty());
    assert_eq!(
        result.events[0].conversation_id.as_deref(),
        Some("gemini-c-file")
    );
}
