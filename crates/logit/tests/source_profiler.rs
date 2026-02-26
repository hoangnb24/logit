use logit::snapshot::profiler::{profile_json_records, profile_jsonl};
use serde_json::json;

#[test]
fn profiles_nested_key_paths_and_value_types() {
    let records = vec![
        json!({
            "event_type": "user_prompt",
            "payload": {"text": "hello", "tokens": 12},
            "tags": ["a", "b"]
        }),
        json!({
            "event_type": "assistant_response",
            "payload": {"text": "world", "tokens": "n/a"},
            "tags": []
        }),
    ];

    let profile = profile_json_records(&records);
    assert_eq!(profile.total_records, 2);

    let payload_text = profile
        .key_stats
        .get("payload.text")
        .expect("payload.text stats should exist");
    assert_eq!(payload_text.occurrences, 2);
    assert_eq!(payload_text.value_types.get("string"), Some(&2));

    let payload_tokens = profile
        .key_stats
        .get("payload.tokens")
        .expect("payload.tokens stats should exist");
    assert_eq!(payload_tokens.occurrences, 2);
    assert_eq!(payload_tokens.value_types.get("number"), Some(&1));
    assert_eq!(payload_tokens.value_types.get("string"), Some(&1));

    let tags = profile
        .key_stats
        .get("tags")
        .expect("tags stats should exist");
    assert_eq!(tags.value_types.get("array"), Some(&2));

    let tag_items = profile
        .key_stats
        .get("tags[]")
        .expect("tags[] stats should exist");
    assert_eq!(tag_items.occurrences, 2);
    assert_eq!(tag_items.value_types.get("string"), Some(&2));

    assert_eq!(
        profile.event_kind_frequency.get("user_prompt"),
        Some(&1usize)
    );
    assert_eq!(
        profile.event_kind_frequency.get("assistant_response"),
        Some(&1usize)
    );
}

#[test]
fn jsonl_profile_skips_invalid_lines_with_warnings() {
    let input = r#"{"event_type":"user_prompt","payload":{"text":"a"}}
not-json
{"kind":"progress","payload":{"text":"b"}}
"#;

    let result = profile_jsonl(input);
    assert_eq!(result.profile.total_records, 2);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("invalid JSON"));
    assert_eq!(
        result.profile.event_kind_frequency.get("user_prompt"),
        Some(&1usize)
    );
    assert_eq!(
        result.profile.event_kind_frequency.get("progress"),
        Some(&1)
    );
}

#[test]
fn profiling_output_is_deterministic_for_key_order_variance() {
    let a = serde_json::from_str(r#"{"z":1,"a":{"k":"v"}}"#).expect("json should parse");
    let b = serde_json::from_str(r#"{"a":{"k":"v"},"z":1}"#).expect("json should parse");

    let profile_a = profile_json_records(&[a]);
    let profile_b = profile_json_records(&[b]);

    assert_eq!(profile_a.key_stats, profile_b.key_stats);
}
