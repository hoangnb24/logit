use logit::adapters::codex::{parse_diagnostic_log_file, parse_diagnostic_log_text};
use logit::models::{ActorRole, EventType, RecordFormat, TimestampQuality};
use serde_json::Value;

#[test]
fn parses_tui_and_desktop_diagnostic_logs_to_runtime_events() {
    let fixture = fixture_path("tui_diagnostic.log");
    let input = std::fs::read_to_string(&fixture).expect("fixture readable");
    let result = parse_diagnostic_log_text(&input, "run-test", fixture.to_string_lossy().as_ref());

    assert!(result.warnings.is_empty());
    assert_eq!(result.events.len(), 3);
    assert!(
        result
            .events
            .iter()
            .all(|event| event.record_format == RecordFormat::Diagnostic)
    );
    assert!(
        result
            .events
            .iter()
            .all(|event| event.role == ActorRole::Runtime)
    );

    assert_eq!(result.events[0].event_type, EventType::DebugLog);
    assert_eq!(result.events[0].session_id.as_deref(), Some("codex-s-001"));
    assert!(
        result.events[0]
            .tags
            .iter()
            .any(|tag| tag == "tui_diagnostic")
    );

    assert_eq!(result.events[1].event_type, EventType::StatusUpdate);
    assert!(
        result.events[1]
            .content_text
            .as_deref()
            .is_some_and(|text| text.contains("slow_render"))
    );

    assert_eq!(result.events[2].event_type, EventType::DebugLog);
    assert!(
        result.events[2]
            .tags
            .iter()
            .any(|tag| tag == "desktop_diagnostic")
    );
    assert_eq!(
        result.events[2].metadata.get("codex_log_event"),
        Some(&Value::String("sync_complete".to_string()))
    );
}

#[test]
fn handles_malformed_and_invalid_timestamp_log_lines_without_crashing() {
    let input = r#"
not-a-log-line
2026-02-01T12:00:01Z INFO codex.tui
bad-timestamp WARN codex.tui render_stall frame_ms=102
"#;
    let result = parse_diagnostic_log_text(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("unrecognized codex log shape"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid `created_at`"))
    );
    assert_eq!(result.events[0].event_type, EventType::StatusUpdate);
    assert_eq!(
        result.events[0].timestamp_quality,
        TimestampQuality::Fallback
    );
}

#[test]
fn parses_diagnostic_log_file_from_disk() {
    let path = fixture_path("tui_diagnostic.log");
    let result =
        parse_diagnostic_log_file(&path, "run-test").expect("diagnostic log file should parse");
    assert_eq!(result.events.len(), 3);
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/codex")
        .join(name)
}
