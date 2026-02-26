use logit::models::AgentSource;
use logit::snapshot::samples::{
    SampleCandidate, extract_representative_samples, redact_and_truncate_samples,
};
use logit::utils::redaction::REDACTION_TOKEN;
use serde_json::{Value, json};

fn fixture_sample(record: Value) -> SampleCandidate {
    SampleCandidate {
        source_kind: AgentSource::Codex,
        source_path: "/tmp/codex/history.jsonl".to_string(),
        source_record_locator: "line:1".to_string(),
        record,
    }
}

#[test]
fn redacts_sensitive_snapshot_text_and_adds_metadata_flags() {
    let candidates = vec![fixture_sample(json!({
        "event_type": "prompt",
        "message": "Contact me at alice@example.com or +1 (555) 123-4567",
        "auth": "Bearer abcdefghijklmnop123456",
        "url": "https://example.test/callback?access_token=secret-token-value&x=1",
        "config": "password = hunter2"
    }))];

    let samples = extract_representative_samples(&candidates, 1);
    let redacted = redact_and_truncate_samples(&samples, 240);

    assert_eq!(redacted.len(), 1);
    let record = redacted[0]
        .record
        .as_object()
        .expect("record should stay object");
    let rendered = serde_json::to_string(record).expect("record should serialize");
    assert!(rendered.contains(REDACTION_TOKEN));
    assert!(!rendered.contains("alice@example.com"));
    assert!(!rendered.contains("hunter2"));
    assert_eq!(
        record.get("pii_redacted").and_then(Value::as_bool),
        Some(true)
    );
    let classes = record
        .get("redaction_classes")
        .and_then(Value::as_array)
        .expect("redaction classes should be present");
    assert!(classes.iter().any(|value| value.as_str() == Some("email")));
    assert!(
        classes
            .iter()
            .any(|value| value.as_str() == Some("secret_assignment"))
    );
    assert!(
        classes
            .iter()
            .any(|value| value.as_str() == Some("bearer_token"))
    );
    assert!(
        classes
            .iter()
            .any(|value| value.as_str() == Some("url_query_token"))
    );
    assert!(classes.iter().any(|value| value.as_str() == Some("phone")));
}

#[test]
fn redacts_private_key_blocks_and_truncates_long_text() {
    let private_key = "-----BEGIN PRIVATE KEY-----\nabc123\n-----END PRIVATE KEY-----";
    let long_text = format!("{private_key} {}", "x".repeat(120));
    let candidates = vec![fixture_sample(json!({
        "kind": "debug",
        "payload": long_text
    }))];

    let samples = extract_representative_samples(&candidates, 1);
    let redacted = redact_and_truncate_samples(&samples, 32);
    let record = redacted[0]
        .record
        .as_object()
        .expect("record should stay object");

    let payload = record
        .get("payload")
        .and_then(Value::as_str)
        .expect("payload should remain string");
    assert!(payload.contains(REDACTION_TOKEN));
    assert!(!payload.contains("BEGIN PRIVATE KEY"));
    assert!(payload.ends_with("..."));
    assert_eq!(
        record.get("snapshot_truncated").and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn snapshot_redaction_is_deterministic_for_identical_samples() {
    let candidates = vec![fixture_sample(json!({
        "event_type": "prompt",
        "content": "email bob@example.com token=abc123456789"
    }))];

    let samples = extract_representative_samples(&candidates, 1);
    let first = redact_and_truncate_samples(&samples, 64);
    let second = redact_and_truncate_samples(&samples, 64);
    assert_eq!(first, second);
    assert_eq!(first[0].sample_rank, 0);
    assert_eq!(first[0].event_kind.as_deref(), Some("prompt"));
}

#[test]
fn redacts_binary_like_text_and_tracks_binary_blob_class() {
    let binary_like = "\0\u{0001}\u{0002}\u{0003}\u{0004}\u{0005}\u{0006}\u{0007}abcdefghijklmnop";
    let candidates = vec![fixture_sample(json!({
        "event_type": "prompt",
        "payload": binary_like
    }))];

    let samples = extract_representative_samples(&candidates, 1);
    let redacted = redact_and_truncate_samples(&samples, 240);
    let record = redacted[0]
        .record
        .as_object()
        .expect("record should stay object");
    let payload = record
        .get("payload")
        .and_then(Value::as_str)
        .expect("payload should remain string");

    assert_eq!(payload, REDACTION_TOKEN);
    let classes = record
        .get("redaction_classes")
        .and_then(Value::as_array)
        .expect("redaction classes should be present");
    assert!(
        classes
            .iter()
            .any(|value| value.as_str() == Some("binary_blob"))
    );
}

#[test]
fn sanitizes_oversized_sample_records_to_bounded_preview_object() {
    let large_fields = (0..120)
        .map(|index| (format!("k{index:03}"), Value::String("x".repeat(80))))
        .collect::<serde_json::Map<String, Value>>();
    let candidates = vec![fixture_sample(Value::Object(large_fields))];

    let samples = extract_representative_samples(&candidates, 1);
    let redacted = redact_and_truncate_samples(&samples, 10_000);
    let record = redacted[0]
        .record
        .as_object()
        .expect("record should stay object");

    assert_eq!(
        record.get("snapshot_sanitized").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        record.get("snapshot_truncated").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        record.get("sanitization_reason").and_then(Value::as_str),
        Some("max_record_chars_exceeded")
    );
    let original_chars = record
        .get("original_char_count")
        .and_then(Value::as_u64)
        .expect("original char count should be present");
    assert!(original_chars > 4096);
    let preview = record
        .get("sanitized_preview")
        .and_then(Value::as_str)
        .expect("sanitized preview should be present");
    assert!(preview.ends_with("..."));
}
