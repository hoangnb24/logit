use std::time::{SystemTime, UNIX_EPOCH};

use logit::discovery::SourceFormatHint;
use logit::discovery::classifier::{
    SourceClassification, classify_bytes, classify_file, classify_from_hint,
};

#[test]
fn maps_format_hints_to_file_classification_when_possible() {
    assert_eq!(
        classify_from_hint(SourceFormatHint::Json),
        Some(SourceClassification::Json)
    );
    assert_eq!(
        classify_from_hint(SourceFormatHint::Jsonl),
        Some(SourceClassification::Jsonl)
    );
    assert_eq!(
        classify_from_hint(SourceFormatHint::TextLog),
        Some(SourceClassification::TextLog)
    );
    assert_eq!(classify_from_hint(SourceFormatHint::Directory), None);
}

#[test]
fn classifies_by_extension_before_content_heuristics() {
    let json_path = std::path::Path::new("/tmp/event.json");
    let jsonl_path = std::path::Path::new("/tmp/event.jsonl");

    assert_eq!(
        classify_bytes(json_path, b"not-valid-json"),
        SourceClassification::Json
    );
    assert_eq!(
        classify_bytes(jsonl_path, b"{\"a\":1}"),
        SourceClassification::Jsonl
    );
}

#[test]
fn classifies_jsonl_by_multiline_content_without_extension() {
    let path = std::path::Path::new("/tmp/source");
    let bytes = br#"{"kind":"a"}
{"kind":"b"}"#;

    assert_eq!(classify_bytes(path, bytes), SourceClassification::Jsonl);
}

#[test]
fn classifies_json_document_without_extension() {
    let path = std::path::Path::new("/tmp/source");
    let bytes = br#"{"session_id":"abc","messages":[1,2,3]}"#;

    assert_eq!(classify_bytes(path, bytes), SourceClassification::Json);
}

#[test]
fn classifies_text_log_when_not_json_or_jsonl() {
    let path = std::path::Path::new("/tmp/source.log");
    let bytes = b"INFO started\nWARN retrying";

    assert_eq!(classify_bytes(path, bytes), SourceClassification::TextLog);
}

#[test]
fn classifies_binary_for_nul_or_invalid_utf8() {
    let path = std::path::Path::new("/tmp/source.bin");

    assert_eq!(
        classify_bytes(path, b"abc\0def"),
        SourceClassification::Binary
    );
    assert_eq!(
        classify_bytes(path, &[0xff, 0xfe, 0xfd]),
        SourceClassification::Binary
    );
}

#[test]
fn classifies_file_from_disk() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("logit-classify-{nanos}.jsonl"));
    std::fs::write(&path, b"{\"a\":1}\n{\"b\":2}\n").expect("temporary classification file write");

    let classification = classify_file(&path).expect("classification should succeed");
    assert_eq!(classification, SourceClassification::Jsonl);

    let _ = std::fs::remove_file(path);
}
