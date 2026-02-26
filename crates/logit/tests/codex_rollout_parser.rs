use logit::adapters::codex::{parse_rollout_file, parse_rollout_jsonl};
use logit::models::{ActorRole, EventType, RecordFormat, TimestampQuality};

#[test]
fn parses_primary_rollout_fixture_to_canonical_events() {
    let fixture = fixture_path("rollout_primary.jsonl");
    let input = std::fs::read_to_string(&fixture).expect("fixture readable");
    let result = parse_rollout_jsonl(&input, "run-test", fixture.to_string_lossy().as_ref());

    assert_eq!(result.warnings.len(), 0);
    assert_eq!(result.events.len(), 3);

    assert_eq!(result.events[0].event_type, EventType::Prompt);
    assert_eq!(result.events[0].role, ActorRole::User);
    assert_eq!(result.events[0].record_format, RecordFormat::Message);
    assert_eq!(result.events[0].session_id.as_deref(), Some("codex-s-001"));

    assert_eq!(result.events[1].event_type, EventType::Response);
    assert_eq!(result.events[1].role, ActorRole::Assistant);

    assert_eq!(result.events[2].event_type, EventType::ToolOutput);
    assert_eq!(result.events[2].role, ActorRole::Tool);
    assert_eq!(result.events[2].record_format, RecordFormat::ToolResult);
    assert_eq!(result.events[2].tool_name.as_deref(), Some("shell"));
}

#[test]
fn handles_malformed_lines_and_missing_timestamps_without_crashing() {
    let fixture = fixture_path("rollout_malformed.jsonl");
    let input = std::fs::read_to_string(&fixture).expect("fixture readable");
    let result = parse_rollout_jsonl(&input, "run-test", fixture.to_string_lossy().as_ref());

    assert_eq!(result.events.len(), 2);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid JSON payload"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid `created_at`"))
    );
    assert!(result.events[0].timestamp_quality == TimestampQuality::Fallback);
    assert!(result.events[1].timestamp_quality == TimestampQuality::Fallback);
}

#[test]
fn parses_rollout_file_from_disk() {
    let path = fixture_path("rollout_primary.jsonl");
    let result = parse_rollout_file(&path, "run-test").expect("rollout file should parse");
    assert_eq!(result.events.len(), 3);
}

#[test]
fn maps_event_msg_family_to_status_updates() {
    let input = r#"{"session_id":"codex-s-xyz","event_id":"evt-010","event_type":"event_msg.progress","created_at":"2026-02-01T12:00:05Z","text":"step done"}"#;
    let result = parse_rollout_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].event_type, EventType::StatusUpdate);
    assert_eq!(result.events[0].role, ActorRole::Runtime);
    assert_eq!(result.events[0].record_format, RecordFormat::System);
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/codex")
        .join(name)
}
