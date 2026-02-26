use logit::adapters::amp::parse_thread_envelope;

#[test]
fn extracts_canonical_text_excerpt_and_parts_from_amp_fixture() {
    let raw = include_str!("../../../fixtures/amp/thread_payloads.json");
    let parsed = parse_thread_envelope(raw).expect("fixture should parse");

    assert!(parsed.warnings.is_empty());
    assert_eq!(parsed.messages.len(), 2);

    let user_message = &parsed.messages[0];
    assert_eq!(user_message.message_id, "m-001");
    assert_eq!(
        user_message.content_text.as_deref(),
        Some("Summarize this file.")
    );
    assert_eq!(
        user_message.content_excerpt.as_deref(),
        Some("Summarize this file.")
    );
    assert_eq!(user_message.content_parts.len(), 1);
    assert_eq!(user_message.content_parts[0].path, "0");
    assert_eq!(user_message.content_parts[0].kind, "text");
    assert_eq!(
        user_message.content_parts[0].text.as_deref(),
        Some("Summarize this file.")
    );

    let assistant_message = &parsed.messages[1];
    assert_eq!(
        assistant_message.content_text.as_deref(),
        Some("Summary completed.")
    );
    assert_eq!(assistant_message.content_parts.len(), 1);
    assert_eq!(assistant_message.content_parts[0].kind, "text");
}

#[test]
fn surfaces_no_content_parts_for_malformed_fixture_shapes() {
    let raw = include_str!("../../../fixtures/amp/thread_payloads_malformed.json");
    let parsed = parse_thread_envelope(raw).expect("malformed fixture should still parse");

    assert_eq!(parsed.messages.len(), 2);
    assert!(parsed.messages[0].content_text.is_none());
    assert!(parsed.messages[0].content_excerpt.is_none());
    assert!(parsed.messages[0].content_parts.is_empty());
    assert!(parsed.messages[1].content_parts.is_empty());
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("`parts` is not an array"))
    );
}

#[test]
fn flattens_nested_typed_content_arrays_without_text_duplication() {
    let raw = r#"{
  "thread_id": "amp-t-nested",
  "messages": [
    {
      "id": "m-nested",
      "role": "assistant",
      "parts": [
        {
          "type": "container",
          "content": [
            {"type": "text", "text": "First nested line."},
            {"type": "text", "text": "Second nested line."}
          ]
        },
        {"type": "tool_call", "name": "grep", "content": {"path": "src/main.rs"}}
      ]
    }
  ]
}"#;

    let parsed = parse_thread_envelope(raw).expect("nested typed payload should parse");
    let message = &parsed.messages[0];

    assert!(parsed.warnings.is_empty());
    assert_eq!(message.part_count, 2);
    assert_eq!(
        message.part_kinds,
        vec!["container", "text", "text", "tool_call"]
    );
    assert_eq!(
        message.content_text.as_deref(),
        Some("First nested line.\nSecond nested line.")
    );
    assert_eq!(message.content_parts.len(), 4);
    assert_eq!(message.content_parts[0].path, "0");
    assert_eq!(message.content_parts[1].path, "0.0");
    assert_eq!(message.content_parts[2].path, "0.1");
    assert_eq!(message.content_parts[3].path, "1");
    assert!(message.content_parts[0].text.is_none());
    assert_eq!(
        message.content_parts[1].text.as_deref(),
        Some("First nested line.")
    );
    assert_eq!(
        message.content_parts[2].text.as_deref(),
        Some("Second nested line.")
    );
    assert!(message.content_parts[3].text.is_none());
}

#[test]
fn concatenates_text_bearing_parts_in_path_order_for_full_text_and_excerpt() {
    let raw = r#"{
  "thread_id": "amp-t-ordered",
  "messages": [
    {
      "id": "m-ordered",
      "role": "assistant",
      "parts": [
        {"type": "text", "text": "First line."},
        {"type": "tool_result", "output": "Second line."},
        {
          "type": "container",
          "content": [
            {"type": "text", "text": "Third line."},
            {"type": "tool_call", "content": {"cmd": "ls"}}
          ]
        },
        {"type": "text", "text": "Fourth line."}
      ]
    }
  ]
}"#;

    let parsed = parse_thread_envelope(raw).expect("ordered typed payload should parse");
    let message = &parsed.messages[0];

    assert!(parsed.warnings.is_empty());
    assert_eq!(message.part_count, 4);
    assert_eq!(
        message.part_kinds,
        vec![
            "text",
            "tool_result",
            "container",
            "text",
            "tool_call",
            "text"
        ]
    );
    assert_eq!(
        message.content_text.as_deref(),
        Some("First line.\nSecond line.\nThird line.\nFourth line.")
    );
    assert_eq!(
        message.content_excerpt.as_deref(),
        Some("First line. Second line. Third line. Fourth line.")
    );
    assert_eq!(message.content_parts.len(), 6);
    assert_eq!(message.content_parts[0].path, "0");
    assert_eq!(message.content_parts[1].path, "1");
    assert_eq!(message.content_parts[2].path, "2");
    assert_eq!(message.content_parts[3].path, "2.0");
    assert_eq!(message.content_parts[4].path, "2.1");
    assert_eq!(message.content_parts[5].path, "3");
}
