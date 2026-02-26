use logit::adapters::codex::{parse_diagnostic_log_text, parse_history_jsonl, parse_rollout_jsonl};
use logit::models::{ActorRole, EventType, RecordFormat};

#[test]
fn rollout_unknown_event_type_and_missing_message_text_are_explicitly_warned() {
    let input = r#"
{"session_id":"codex-s-1","event_id":"evt-1","event_type":"mystery_kind","created_at":"2026-02-01T12:00:00Z","text":"diag fallback"}
{"session_id":"codex-s-1","event_id":"evt-2","event_type":"user_prompt","created_at":"2026-02-01T12:00:01Z","text":null}
"#;
    let result = parse_rollout_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 2);

    assert_eq!(result.events[0].record_format, RecordFormat::Diagnostic);
    assert_eq!(result.events[0].event_type, EventType::DebugLog);
    assert_eq!(result.events[0].role, ActorRole::Runtime);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("unrecognized `event_type` `mystery_kind`"))
    );

    assert_eq!(result.events[1].record_format, RecordFormat::Message);
    assert_eq!(result.events[1].event_type, EventType::Prompt);
    assert_eq!(result.events[1].role, ActorRole::User);
    assert!(result.events[1].content_text.is_none());
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("missing message content text"))
    );
}

#[test]
fn history_null_role_and_sparse_fields_fall_back_without_crashing() {
    let input = r#"
{"source":"codex_history","session_id":"codex-s-1","prompt_id":"","created_at":"2026-02-01T12:00:00Z","role":null,"content":null}
"#;
    let result = parse_history_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].record_format, RecordFormat::Diagnostic);
    assert_eq!(result.events[0].event_type, EventType::DebugLog);
    assert_eq!(result.events[0].role, ActorRole::Runtime);
    assert!(result.events[0].event_id.starts_with("codex-history-line-"));
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("missing history role"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("missing content text"))
    );
}

#[test]
fn diagnostic_log_unknown_level_is_warned_and_mapped_to_debug_log() {
    let input = r#"2026-02-01T12:00:00Z NOTICE codex.tui ui_tick count=1"#;
    let result = parse_diagnostic_log_text(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].record_format, RecordFormat::Diagnostic);
    assert_eq!(result.events[0].event_type, EventType::DebugLog);
    assert_eq!(result.events[0].role, ActorRole::Runtime);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("unrecognized diagnostic log level `NOTICE`"))
    );
}
