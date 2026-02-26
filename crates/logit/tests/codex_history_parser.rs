use logit::adapters::codex::{parse_history_file, parse_history_jsonl, parse_rollout_jsonl};
use logit::models::{ActorRole, EventType, RecordFormat, TimestampQuality};

#[test]
fn parses_history_fixture_as_auxiliary_prompt_events() {
    let fixture = fixture_path("history_auxiliary.jsonl");
    let input = std::fs::read_to_string(&fixture).expect("fixture readable");
    let result = parse_history_jsonl(&input, "run-test", fixture.to_string_lossy().as_ref());

    assert!(result.warnings.is_empty());
    assert_eq!(result.events.len(), 2);

    assert_eq!(result.events[0].record_format, RecordFormat::Message);
    assert_eq!(result.events[0].event_type, EventType::Prompt);
    assert_eq!(result.events[0].role, ActorRole::User);
    assert!(
        result.events[0]
            .tags
            .iter()
            .any(|tag| tag == "history_auxiliary")
    );

    assert_eq!(result.events[1].record_format, RecordFormat::Message);
    assert_eq!(result.events[1].event_type, EventType::Response);
    assert_eq!(result.events[1].role, ActorRole::Assistant);
}

#[test]
fn handles_malformed_history_lines_without_crashing() {
    let input = r#"
{"session_id":"codex-s-001","prompt_id":"p-001","created_at":"bad-ts","role":"user","content":"hello"}
{"session_id":"codex-s-001","prompt_id":"p-002","created_at":"2026-02-01T12:00:00Z","role":"weird","content":"??"}
not-json
"#;
    let result = parse_history_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 2);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid `created_at`"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("unrecognized history role"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid JSON payload"))
    );
    assert_eq!(
        result.events[0].timestamp_quality,
        TimestampQuality::Fallback
    );
    assert_eq!(result.events[1].record_format, RecordFormat::Diagnostic);
    assert_eq!(result.events[1].event_type, EventType::DebugLog);
    assert_eq!(result.events[1].role, ActorRole::Runtime);
}

#[test]
fn parses_history_file_from_disk() {
    let path = fixture_path("history_auxiliary.jsonl");
    let result = parse_history_file(&path, "run-test").expect("history file should parse");
    assert_eq!(result.events.len(), 2);
}

#[test]
fn conversational_canonical_hash_matches_rollout_for_exact_duplicate_messages() {
    let rollout_input = r#"{"session_id":"codex-s-dup","event_id":"evt-001","event_type":"user_prompt","created_at":"2026-02-01T12:00:00Z","text":"same content"}"#;
    let history_input = r#"{"source":"codex_history","session_id":"codex-s-dup","prompt_id":"p-001","created_at":"2026-02-01T12:00:00Z","role":"user","content":"same content"}"#;

    let rollout = parse_rollout_jsonl(rollout_input, "run-test", "rollout");
    let history = parse_history_jsonl(history_input, "run-test", "history");
    assert_eq!(rollout.events.len(), 1);
    assert_eq!(history.events.len(), 1);
    assert_eq!(
        rollout.events[0].canonical_hash,
        history.events[0].canonical_hash
    );
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/codex")
        .join(name)
}
