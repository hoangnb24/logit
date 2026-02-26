use logit::adapters::codex::parse_rollout_jsonl;
use logit::models::{ActorRole, EventType, RecordFormat};
use serde_json::Value;

#[test]
fn assistant_response_uses_response_item_object_content() {
    let input = r#"{"session_id":"codex-s-1","event_id":"evt-1","event_type":"assistant_response","created_at":"2026-02-01T12:00:00Z","response_item":{"type":"output_text","text":"answer from response item"}}"#;
    let result = parse_rollout_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert!(result.warnings.is_empty());
    assert_eq!(result.events[0].record_format, RecordFormat::Message);
    assert_eq!(result.events[0].event_type, EventType::Response);
    assert_eq!(result.events[0].role, ActorRole::Assistant);
    assert_eq!(
        result.events[0].content_text.as_deref(),
        Some("answer from response item")
    );
    assert_eq!(
        result.events[0].metadata.get("codex_content_source"),
        Some(&Value::String("response_item".to_string()))
    );
}

#[test]
fn assistant_response_uses_response_items_array_content() {
    let input = r#"{"session_id":"codex-s-1","event_id":"evt-1","event_type":"assistant_response","created_at":"2026-02-01T12:00:00Z","response_items":[{"type":"output_text","text":"first chunk"},{"type":"output_text","text":"second chunk"}]}"#;
    let result = parse_rollout_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert!(result.warnings.is_empty());
    assert_eq!(
        result.events[0].content_text.as_deref(),
        Some("first chunk\nsecond chunk")
    );
    assert_eq!(
        result.events[0].metadata.get("codex_content_source"),
        Some(&Value::String("response_items".to_string()))
    );
}

#[test]
fn sparse_response_item_still_emits_message_with_explicit_warning() {
    let input = r#"{"session_id":"codex-s-1","event_id":"evt-1","event_type":"assistant_response","created_at":"2026-02-01T12:00:00Z","response_item":{"type":"output_text","text":null}}"#;
    let result = parse_rollout_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].record_format, RecordFormat::Message);
    assert_eq!(result.events[0].event_type, EventType::Response);
    assert_eq!(result.events[0].role, ActorRole::Assistant);
    assert!(result.events[0].content_text.is_none());
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("missing message content text"))
    );
}
