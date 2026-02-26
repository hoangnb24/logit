use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use logit::adapters::opencode::{parse_auxiliary_log_file, parse_auxiliary_log_text};
use logit::models::{ActorRole, EventType, RecordFormat, TimestampQuality};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

#[test]
fn parses_runtime_and_prompt_history_auxiliary_lines() {
    let raw = include_str!("../../../fixtures/opencode/runtime_prompt_history.log");
    let parsed = parse_auxiliary_log_text(raw, "test-run", "~/.opencode/logs/runtime.log");

    assert_eq!(parsed.events.len(), 2);
    assert!(parsed.warnings.is_empty());

    let prompt_event = parsed
        .events
        .iter()
        .find(|event| {
            event
                .tags
                .iter()
                .any(|tag| tag == "prompt_history_auxiliary")
        })
        .expect("prompt history event should be present");
    assert_eq!(prompt_event.record_format, RecordFormat::Message);
    assert_eq!(prompt_event.event_type, EventType::Prompt);
    assert_eq!(prompt_event.role, ActorRole::User);
    assert_eq!(prompt_event.event_id, "msg-001");
    assert!(
        prompt_event
            .content_text
            .as_deref()
            .is_some_and(|text| text.contains("message_id=msg-001"))
    );

    let runtime_event = parsed
        .events
        .iter()
        .find(|event| event.event_type == EventType::Metric)
        .expect("runtime token usage event should be present");
    assert_eq!(runtime_event.record_format, RecordFormat::Diagnostic);
    assert_eq!(runtime_event.role, ActorRole::Runtime);
    assert_eq!(runtime_event.input_tokens, Some(12));
    assert_eq!(runtime_event.output_tokens, Some(19));
    assert_eq!(runtime_event.total_tokens, Some(31));
    assert!(
        runtime_event
            .tags
            .iter()
            .any(|tag| tag == "runtime_diagnostic")
    );
    assert!(runtime_event.tags.iter().any(|tag| tag == "diagnostic_log"));
}

#[test]
fn handles_malformed_and_invalid_timestamp_auxiliary_lines() {
    let raw = r#"
not parseable
bad-ts INFO opencode.runtime token_usage prompt=abc completion=5
2026-02-05T08:00:00Z WARN opencode.runtime heartbeat status=degraded
"#;

    let parsed = parse_auxiliary_log_text(raw, "test-run", "~/.opencode/logs/runtime.log");

    assert_eq!(parsed.events.len(), 2);
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("unrecognized auxiliary log format"))
    );
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid timestamp"))
    );
    assert_eq!(
        parsed.events[0].timestamp_quality,
        TimestampQuality::Fallback
    );
    assert_eq!(parsed.events[0].input_tokens, None);
    assert_eq!(parsed.events[0].output_tokens, Some(5));
    assert_eq!(parsed.events[0].total_tokens, None);
    assert_eq!(parsed.events[1].event_type, EventType::StatusUpdate);
}

#[test]
fn parses_auxiliary_log_file_from_disk() {
    let temp_dir = unique_temp_dir("logit-opencode-aux");
    std::fs::create_dir_all(&temp_dir).expect("temp dir should be creatable");
    let path = temp_dir.join("runtime_prompt_history.log");
    std::fs::write(
        &path,
        include_str!("../../../fixtures/opencode/runtime_prompt_history.log"),
    )
    .expect("fixture file should be writable");

    let parsed = parse_auxiliary_log_file(&path, "test-run").expect("file parse should succeed");
    assert_eq!(parsed.events.len(), 2);
    assert!(parsed.warnings.is_empty());
}

#[test]
fn maps_error_levels_and_missing_message_id_with_stable_fallbacks() {
    let raw = r#"
2026-02-08T09:00:00Z ERROR opencode.runtime worker_crash code=E42
2026-02-08T09:00:01Z INFO opencode.prompt_history chars=12
"#;
    let parsed = parse_auxiliary_log_text(raw, "test-run", "~/.opencode/logs/runtime.log");

    assert_eq!(parsed.events.len(), 2);
    assert!(parsed.warnings.is_empty());

    assert_eq!(parsed.events[0].event_type, EventType::Error);
    assert_eq!(parsed.events[0].role, ActorRole::Runtime);
    assert!(
        parsed.events[0]
            .tags
            .iter()
            .any(|tag| tag == "runtime_diagnostic")
    );

    assert_eq!(parsed.events[1].event_type, EventType::Prompt);
    assert_eq!(parsed.events[1].role, ActorRole::User);
    assert!(parsed.events[1].event_id.starts_with("opencode-aux-line-"));
    assert!(
        parsed.events[1]
            .tags
            .iter()
            .any(|tag| tag == "prompt_history_auxiliary")
    );
}
