use logit::adapters::opencode::{
    build_message_key_index, join_message_metadata_with_parts, parse_part_records_jsonl,
    parse_session_metadata_jsonl,
};

#[test]
fn joins_messages_with_matching_parts_from_fixtures() {
    let message_raw = include_str!("../../../fixtures/opencode/session_messages.jsonl");
    let part_raw = include_str!("../../../fixtures/opencode/session_parts.jsonl");

    let metadata = parse_session_metadata_jsonl(message_raw).expect("message fixture should parse");
    let message_index = build_message_key_index(&metadata.messages);
    let parts = parse_part_records_jsonl(part_raw, Some(&message_index)).expect("parts parse");
    let joined = join_message_metadata_with_parts(&metadata.messages, &parts.parts);

    assert_eq!(joined.joined_messages.len(), 2);
    assert!(joined.messages_without_parts.is_empty());
    assert!(joined.orphan_parts.is_empty());
    assert!(joined.warnings.is_empty());
    assert_eq!(joined.joined_messages[0].message.message_id, "msg-001");
    assert_eq!(joined.joined_messages[0].parts.len(), 1);
    assert_eq!(joined.joined_messages[0].parts[0].part_id, "part-001");
    assert_eq!(joined.joined_messages[1].message.message_id, "msg-002");
    assert_eq!(joined.joined_messages[1].parts[0].part_id, "part-002");
}

#[test]
fn reports_orphan_parts_and_messages_without_parts() {
    let message_raw = include_str!("../../../fixtures/opencode/session_messages.jsonl");
    let metadata = parse_session_metadata_jsonl(message_raw).expect("message fixture should parse");
    let message_index = build_message_key_index(&metadata.messages);
    let part_raw = r#"
{"sessionID":"oc-s-001","messageID":"msg-001","partID":"part-001","kind":"input_text","text":"hello"}
{"sessionID":"oc-s-001","messageID":"msg-404","partID":"part-orphan","kind":"output_text","text":"missing target"}
"#;
    let parts = parse_part_records_jsonl(part_raw, Some(&message_index)).expect("parts parse");
    let joined = join_message_metadata_with_parts(&metadata.messages, &parts.parts);

    assert_eq!(joined.joined_messages.len(), 2);
    assert_eq!(joined.messages_without_parts.len(), 1);
    assert_eq!(joined.messages_without_parts[0].message_id, "msg-002");
    assert_eq!(joined.orphan_parts.len(), 1);
    assert_eq!(joined.orphan_parts[0].message_id, "msg-404");
    assert!(
        joined
            .warnings
            .iter()
            .any(|warning| warning.contains("orphan part"))
    );
    assert!(
        joined
            .warnings
            .iter()
            .any(|warning| warning.contains("has no part records"))
    );
}

#[test]
fn join_output_is_deterministic_when_inputs_are_unsorted() {
    let message_raw = include_str!("../../../fixtures/opencode/session_messages.jsonl");
    let part_raw = include_str!("../../../fixtures/opencode/session_parts.jsonl");
    let metadata = parse_session_metadata_jsonl(message_raw).expect("message fixture should parse");
    let message_index = build_message_key_index(&metadata.messages);
    let parsed_parts =
        parse_part_records_jsonl(part_raw, Some(&message_index)).expect("parts parse");

    let mut unsorted_messages = metadata.messages.clone();
    unsorted_messages.reverse();
    let mut unsorted_parts = parsed_parts.parts.clone();
    unsorted_parts.reverse();

    let joined = join_message_metadata_with_parts(&unsorted_messages, &unsorted_parts);
    let message_ids = joined
        .joined_messages
        .iter()
        .map(|item| item.message.message_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(message_ids, vec!["msg-001", "msg-002"]);
    assert_eq!(joined.joined_messages[0].parts[0].part_id, "part-001");
    assert_eq!(joined.joined_messages[1].parts[0].part_id, "part-002");
}

#[test]
fn joins_multiple_parts_for_same_message_in_stable_order() {
    let message_raw = include_str!("../../../fixtures/opencode/session_messages.jsonl");
    let metadata = parse_session_metadata_jsonl(message_raw).expect("message fixture should parse");
    let message_index = build_message_key_index(&metadata.messages);
    let part_raw = r#"
{"sessionID":"oc-s-001","messageID":"msg-001","partID":"part-003","kind":"output_text","text":"tail"}
{"sessionID":"oc-s-001","messageID":"msg-001","partID":"part-001","kind":"input_text","text":"head"}
{"sessionID":"oc-s-001","messageID":"msg-002","partID":"part-010","kind":"output_text","text":"other message"}
"#;
    let parsed_parts =
        parse_part_records_jsonl(part_raw, Some(&message_index)).expect("parts parse");

    let joined = join_message_metadata_with_parts(&metadata.messages, &parsed_parts.parts);
    let first_message_parts = joined
        .joined_messages
        .iter()
        .find(|entry| entry.message.message_id == "msg-001")
        .expect("msg-001 should be joined");
    let part_ids = first_message_parts
        .parts
        .iter()
        .map(|part| part.part_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(part_ids, vec!["part-001", "part-003"]);
}
