use logit::adapters::opencode::{
    build_message_key_index, parse_part_records_jsonl, parse_session_metadata_jsonl,
};

#[test]
fn parses_opencode_part_records_with_known_message_index() {
    let message_raw = include_str!("../../../fixtures/opencode/session_messages.jsonl");
    let part_raw = include_str!("../../../fixtures/opencode/session_parts.jsonl");

    let messages = parse_session_metadata_jsonl(message_raw).expect("message fixture should parse");
    let message_index = build_message_key_index(&messages.messages);
    let parts = parse_part_records_jsonl(part_raw, Some(&message_index))
        .expect("part fixture should parse");

    assert_eq!(parts.parts.len(), 2);
    assert_eq!(parts.orphan_count, 0);
    assert!(parts.warnings.is_empty());
    assert_eq!(parts.parts[0].kind, "input_text");
    assert_eq!(
        parts.parts[0].text.as_deref(),
        Some("Please produce a release checklist.")
    );
}

#[test]
fn detects_orphan_part_records_against_known_message_index() {
    let message_raw = include_str!("../../../fixtures/opencode/session_messages.jsonl");
    let orphan_raw = include_str!("../../../fixtures/opencode/session_parts_orphan.jsonl");

    let messages = parse_session_metadata_jsonl(message_raw).expect("message fixture should parse");
    let message_index = build_message_key_index(&messages.messages);
    let parts = parse_part_records_jsonl(orphan_raw, Some(&message_index))
        .expect("orphan fixture should parse");

    assert_eq!(parts.parts.len(), 1);
    assert_eq!(parts.orphan_count, 1);
    assert!(parts.parts[0].is_orphan);
    assert!(
        parts
            .warnings
            .iter()
            .any(|warning| warning.contains("orphan part"))
    );
}

#[test]
fn handles_malformed_part_rows_without_crashing() {
    let raw = r#"
{"sessionID":"oc-s-001","messageID":"msg-001","partID":"part-001","kind":"step_event"}
not-json
{"sessionID":"oc-s-001","messageID":"msg-002","kind":"input_text"}
"#;

    let parts =
        parse_part_records_jsonl(raw, None).expect("parser should continue on malformed rows");

    assert_eq!(parts.parts.len(), 1);
    assert!(parts.parts[0].is_step_event);
    assert!(
        parts
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid JSON"))
    );
    assert!(
        parts
            .warnings
            .iter()
            .any(|warning| warning.contains("missing required `partID`"))
    );
}

#[test]
fn handles_part_kind_variance_and_null_text() {
    let raw = r#"
{"sessionID":"oc-s-010","messageID":"msg-010","partID":"part-a","kind":"STEP_EVENT","text":null}
{"sessionID":"oc-s-010","messageID":"msg-010","partID":"part-b","kind":"approval_step","text":"ok"}
{"sessionID":"oc-s-010","messageID":"msg-010","partID":"part-c","kind":42}
"#;

    let parsed = parse_part_records_jsonl(raw, None).expect("kind-variance rows should parse");

    assert_eq!(parsed.parts.len(), 3);
    assert!(parsed.warnings.is_empty());

    assert_eq!(parsed.parts[0].part_id, "part-a");
    assert!(parsed.parts[0].is_step_event);
    assert_eq!(parsed.parts[0].text, None);

    assert_eq!(parsed.parts[1].part_id, "part-b");
    assert!(parsed.parts[1].is_step_event);
    assert_eq!(parsed.parts[1].text.as_deref(), Some("ok"));

    assert_eq!(parsed.parts[2].part_id, "part-c");
    assert_eq!(parsed.parts[2].kind, "unknown");
    assert!(!parsed.parts[2].is_step_event);
}
