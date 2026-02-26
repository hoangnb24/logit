use logit::utils::content::{
    DEFAULT_EXCERPT_MAX_CHARS, derive_excerpt, extract_text, extract_text_and_excerpt,
};
use serde_json::json;

#[test]
fn extracts_text_from_priority_keys() {
    let value = json!({
        "id": "msg-1",
        "role": "assistant",
        "text": "  hello from text field  ",
        "metadata": {"ignored": true}
    });

    let extracted = extract_text(&value).expect("text should be extracted");
    assert_eq!(extracted, "hello from text field");
}

#[test]
fn extracts_and_joins_text_from_content_array() {
    let value = json!({
        "content": [
            {"type": "text", "text": "first line"},
            {"type": "text", "text": "second line"},
            {"type": "meta", "name": "skip-me"}
        ]
    });

    let extracted = extract_text(&value).expect("content array text should be extracted");
    assert_eq!(extracted, "first line\nsecond line");
}

#[test]
fn falls_back_to_sorted_non_priority_keys() {
    let value = serde_json::from_str::<serde_json::Value>(
        r#"{"zeta":"last","alpha":"first","role":"assistant"}"#,
    )
    .expect("json must parse");

    let extracted = extract_text(&value).expect("fallback extraction should succeed");
    assert_eq!(extracted, "first\nlast");
}

#[test]
fn returns_none_for_metadata_only_object() {
    let value = json!({
        "id": "evt-1",
        "type": "status",
        "role": "system",
        "timestamp": "2026-02-25T00:00:00Z"
    });

    assert!(extract_text(&value).is_none());
}

#[test]
fn excerpt_is_deterministic_and_truncated() {
    let excerpt = derive_excerpt("  one\t two\nthree   four  ", 9)
        .expect("excerpt should be generated for non-empty text");
    assert_eq!(excerpt, "one two t...");
}

#[test]
fn excerpt_returns_none_for_empty_or_zero_limit() {
    assert!(derive_excerpt("   ", DEFAULT_EXCERPT_MAX_CHARS).is_none());
    assert!(derive_excerpt("non-empty", 0).is_none());
}

#[test]
fn extract_text_and_excerpt_return_consistent_results() {
    let value = json!({
        "content": [
            {"text": "first paragraph"},
            {"text": "second paragraph"}
        ]
    });

    let extracted = extract_text_and_excerpt(&value, 12);
    assert_eq!(
        extracted.content_text.as_deref(),
        Some("first paragraph\nsecond paragraph")
    );
    assert_eq!(
        extracted.content_excerpt.as_deref(),
        Some("first paragr...")
    );
}
