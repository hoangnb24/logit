use logit::adapters::amp::{
    parse_auxiliary_history_session_file, parse_auxiliary_history_session_jsonl,
    parse_thread_envelope,
};

#[test]
fn parses_amp_thread_fixture_metadata() {
    let raw = include_str!("../../../fixtures/amp/thread_payloads.json");
    let parsed = parse_thread_envelope(raw).expect("fixture should parse");

    assert_eq!(parsed.thread.thread_id, "amp-t-001");
    assert_eq!(parsed.thread.message_count, 2);
    assert_eq!(parsed.thread.roles_seen, vec!["assistant", "user"]);
    assert_eq!(
        parsed.thread.first_created_at.as_deref(),
        Some("2026-02-04T10:10:00Z")
    );
    assert_eq!(
        parsed.thread.last_created_at.as_deref(),
        Some("2026-02-04T10:10:02Z")
    );
    assert!(parsed.warnings.is_empty());

    assert_eq!(parsed.messages[0].message_id, "m-001");
    assert_eq!(parsed.messages[0].part_count, 1);
    assert_eq!(parsed.messages[0].part_kinds, vec!["text"]);
}

#[test]
fn surfaces_warnings_for_malformed_parts_and_null_timestamps() {
    let raw = include_str!("../../../fixtures/amp/thread_payloads_malformed.json");
    let parsed = parse_thread_envelope(raw).expect("malformed fixture should still parse envelope");

    assert_eq!(parsed.thread.thread_id, "amp-t-bad");
    assert_eq!(parsed.thread.message_count, 2);
    assert!(parsed.thread.first_created_at.is_none());
    assert!(parsed.thread.last_created_at.is_none());
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("not an array"))
    );
}

#[test]
fn rejects_envelopes_without_messages_array() {
    let raw = include_str!("../../../fixtures/amp/blob_limits.json");
    let err = parse_thread_envelope(raw).expect_err("missing messages array must fail");
    assert!(err.to_string().contains("must contain `messages`"));
}

#[test]
fn skips_messages_with_non_string_id_or_role_without_failing() {
    let raw = r#"{
  "thread_id": "amp-t-edge",
  "messages": [
    {"id": 123, "role": "user", "parts": [{"type":"text","text":"skip-id"}]},
    {"id": "m-bad-role", "role": {"name":"assistant"}, "parts": [{"type":"text","text":"skip-role"}]},
    {"id": "m-good", "role": "assistant", "parts": [{"type":"text","text":"keep-me"}]}
  ]
}"#;

    let parsed =
        parse_thread_envelope(raw).expect("malformed message fields should not abort parse");
    assert_eq!(parsed.thread.message_count, 1);
    assert_eq!(parsed.messages.len(), 1);
    assert_eq!(parsed.messages[0].message_id, "m-good");
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("`id` must be a string when present"))
    );
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("`role` must be a string when present"))
    );
}

#[test]
fn defaults_non_string_part_type_to_unknown_with_warning() {
    let raw = r#"{
  "thread_id": "amp-t-type",
  "messages": [
    {"id": "m-1", "role": "assistant", "parts": [{"type": 7, "text":"value"}]}
  ]
}"#;

    let parsed = parse_thread_envelope(raw).expect("non-string part type should not abort parse");
    assert_eq!(parsed.messages.len(), 1);
    assert_eq!(parsed.messages[0].part_kinds, vec!["unknown"]);
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("`type` must be a string when present"))
    );
}

#[test]
fn parses_amp_auxiliary_history_session_rows() {
    let raw = concat!(
        "{\"history_id\":\"h-001\",\"kind\":\"history_entry\",\"thread_id\":\"amp-t-001\",\"session_id\":\"amp-s-001\",\"created_at\":\"2026-02-04T10:12:00Z\",\"summary\":\"resumed session\"}\n",
        "{\"event_id\":\"s-001\",\"type\":\"session_state\",\"thread\":\"amp-t-001\",\"session\":\"amp-s-001\",\"timestamp\":\"2026-02-04T10:12:05Z\",\"note\":\"applied retention policy\"}\n",
        "{\"message_id\":\"m-dup\",\"role\":\"assistant\",\"parts\":[{\"type\":\"text\",\"text\":\"duplicate thread message\"}]}\n",
        "not-json\n",
    );
    let parsed = parse_auxiliary_history_session_jsonl(raw);

    assert_eq!(parsed.records.len(), 2);
    assert_eq!(parsed.record_kinds, vec!["history_entry", "session_state"]);
    assert_eq!(parsed.skipped_message_duplicates, 1);
    assert_eq!(parsed.records[0].record_id, "h-001");
    assert_eq!(parsed.records[0].thread_id.as_deref(), Some("amp-t-001"));
    assert_eq!(parsed.records[0].session_id.as_deref(), Some("amp-s-001"));
    assert_eq!(
        parsed.records[0].content_text.as_deref(),
        Some("resumed session")
    );
    assert_eq!(parsed.records[1].record_id, "s-001");
    assert_eq!(
        parsed.records[1].content_text.as_deref(),
        Some("applied retention policy")
    );
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("skipped likely duplicate thread message"))
    );
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid JSON payload"))
    );
}

#[test]
fn parses_amp_auxiliary_history_session_file_from_disk() {
    let temp = unique_temp_dir("logit-amp-aux");
    std::fs::create_dir_all(&temp).expect("temp dir should be creatable");
    let file = temp.join("history.jsonl");
    std::fs::write(
        &file,
        "{\"id\":\"aux-1\",\"kind\":\"session_state\",\"thread_id\":\"amp-t-9\",\"note\":\"checkpoint\"}\n",
    )
    .expect("auxiliary file should be writable");

    let parsed =
        parse_auxiliary_history_session_file(&file).expect("auxiliary file should parse cleanly");
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.records[0].record_id, "aux-1");
    assert_eq!(parsed.records[0].record_kind, "session_state");
    assert_eq!(parsed.records[0].thread_id.as_deref(), Some("amp-t-9"));
    assert_eq!(
        parsed.records[0].content_text.as_deref(),
        Some("checkpoint")
    );
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}
