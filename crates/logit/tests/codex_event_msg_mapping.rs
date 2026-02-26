use logit::adapters::codex::parse_rollout_jsonl;
use logit::models::{ActorRole, EventType, RecordFormat};
use serde_json::Value;

#[test]
fn maps_event_msg_progress_family_to_status_update_with_progress_category() {
    let input = r#"{"session_id":"codex-s-xyz","event_id":"evt-010","event_type":"event_msg.progress","created_at":"2026-02-01T12:00:05Z","text":"step done"}"#;
    let result = parse_rollout_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].record_format, RecordFormat::System);
    assert_eq!(result.events[0].event_type, EventType::StatusUpdate);
    assert_eq!(result.events[0].role, ActorRole::Runtime);
    assert_eq!(
        result.events[0].metadata.get("codex_event_msg_category"),
        Some(&Value::String("progress".to_string()))
    );
    assert_eq!(
        result.events[0].metadata.get("codex_event_msg_name"),
        Some(&Value::String("progress".to_string()))
    );
}

#[test]
fn maps_event_msg_meta_family_to_system_notice_with_meta_category() {
    let input = r#"{"session_id":"codex-s-xyz","event_id":"evt-011","event_type":"event_msg.meta","created_at":"2026-02-01T12:00:06Z","text":"toolchain switched"}"#;
    let result = parse_rollout_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].record_format, RecordFormat::System);
    assert_eq!(result.events[0].event_type, EventType::SystemNotice);
    assert_eq!(result.events[0].role, ActorRole::Runtime);
    assert_eq!(
        result.events[0].metadata.get("codex_event_msg_category"),
        Some(&Value::String("meta".to_string()))
    );
    assert_eq!(
        result.events[0].metadata.get("codex_event_msg_name"),
        Some(&Value::String("meta".to_string()))
    );
}

#[test]
fn maps_unknown_event_msg_suffix_to_generic_progress_category() {
    let input = r#"{"session_id":"codex-s-xyz","event_id":"evt-012","event_type":"event_msg.lifecycle","created_at":"2026-02-01T12:00:07Z","text":"worker joined"}"#;
    let result = parse_rollout_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].record_format, RecordFormat::System);
    assert_eq!(result.events[0].event_type, EventType::StatusUpdate);
    assert_eq!(result.events[0].role, ActorRole::Runtime);
    assert_eq!(
        result.events[0].metadata.get("codex_event_msg_category"),
        Some(&Value::String("generic".to_string()))
    );
    assert_eq!(
        result.events[0].metadata.get("codex_event_msg_name"),
        Some(&Value::String("lifecycle".to_string()))
    );
}
