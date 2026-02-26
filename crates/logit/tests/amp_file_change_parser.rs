use logit::adapters::amp::{DEFAULT_CHANGE_BLOB_LIMIT_BYTES, parse_file_change_artifact};

#[test]
fn parses_attachment_blob_limit_fixture_into_telemetry() {
    let raw = include_str!("../../../fixtures/amp/blob_limits.json");
    let parsed = parse_file_change_artifact(raw).expect("fixture should parse");

    assert_eq!(parsed.thread_id.as_deref(), Some("amp-t-limits"));
    assert_eq!(parsed.blob_limit_bytes, 1_024);
    assert_eq!(parsed.attachments.len(), 2);
    assert_eq!(parsed.file_changes.len(), 0);
    assert_eq!(parsed.over_limit_attachments, 2);
    assert_eq!(parsed.truncated_blobs, 0);
    assert!(parsed.warnings.is_empty());

    assert_eq!(parsed.attachments[0].attachment_id, "a-001");
    assert_eq!(parsed.attachments[0].size_bytes, Some(1_024));
    assert!(parsed.attachments[0].at_or_over_limit);
    assert_eq!(parsed.attachments[1].attachment_id, "a-002");
    assert_eq!(parsed.attachments[1].size_bytes, Some(1_025));
    assert!(parsed.attachments[1].at_or_over_limit);
}

#[test]
fn parses_file_change_rows_and_truncates_large_blobs() {
    let raw = r#"{
  "thread_id": "amp-fc-001",
  "blob_limit_bytes": 10,
  "file_changes": [
    {
      "path": "src/lib.rs",
      "operation": "edit",
      "tool": "apply_patch",
      "before": "0123456789abcdef",
      "after": "short"
    },
    {
      "file": "README.md",
      "action": "create",
      "tool_name": "write_file",
      "new_content": {"text": "This output is intentionally long for truncation."}
    }
  ]
}"#;

    let parsed = parse_file_change_artifact(raw).expect("inline file-change artifact should parse");

    assert_eq!(parsed.thread_id.as_deref(), Some("amp-fc-001"));
    assert_eq!(parsed.blob_limit_bytes, 10);
    assert_eq!(parsed.file_changes.len(), 2);
    assert_eq!(parsed.paths_seen, vec!["README.md", "src/lib.rs"]);
    assert_eq!(parsed.tools_seen, vec!["apply_patch", "write_file"]);
    assert_eq!(parsed.truncated_blobs, 2);
    assert!(parsed.warnings.is_empty());

    assert_eq!(
        parsed.file_changes[0].before_preview.as_deref(),
        Some("0123456...")
    );
    assert!(parsed.file_changes[0].before_truncated);
    assert_eq!(
        parsed.file_changes[0].after_preview.as_deref(),
        Some("short")
    );
    assert!(!parsed.file_changes[0].after_truncated);
    assert_eq!(parsed.file_changes[1].path, "README.md");
    assert!(parsed.file_changes[1].after_truncated);
}

#[test]
fn handles_malformed_file_change_shapes_without_crashing() {
    let raw = r#"{
  "blob_limit_bytes": "invalid",
  "attachments": {"attachment_id": "a-001"},
  "changes": [
    null,
    {
      "operation": "edit",
      "before": "small value"
    }
  ]
}"#;

    let parsed = parse_file_change_artifact(raw).expect("malformed shape should still parse");

    assert_eq!(parsed.blob_limit_bytes, DEFAULT_CHANGE_BLOB_LIMIT_BYTES);
    assert_eq!(parsed.attachments.len(), 0);
    assert_eq!(parsed.file_changes.len(), 1);
    assert_eq!(parsed.file_changes[0].path, "unknown");
    assert_eq!(
        parsed.file_changes[0].before_preview.as_deref(),
        Some("small value")
    );
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid `blob_limit_bytes`"))
    );
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("`attachments` is not an array"))
    );
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("file_change[0] is not an object"))
    );
}

#[test]
fn supports_alias_fields_and_string_blob_limit_values() {
    let raw = r#"{
  "thread_id": "amp-fc-alias",
  "blob_limit_bytes": "8",
  "attachments": [{"id":"att-1","size":"9","status":"too_large"}],
  "changes": [
    {
      "filename": "src/main.rs",
      "op": "update",
      "source_tool": "rewrite",
      "old_content": "abcdefghij",
      "new": "xyz"
    }
  ]
}"#;

    let parsed = parse_file_change_artifact(raw).expect("alias payload should parse");
    assert_eq!(parsed.thread_id.as_deref(), Some("amp-fc-alias"));
    assert_eq!(parsed.blob_limit_bytes, 8);
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(parsed.attachments[0].attachment_id, "att-1");
    assert_eq!(parsed.attachments[0].size_bytes, Some(9));
    assert!(parsed.attachments[0].at_or_over_limit);
    assert_eq!(parsed.file_changes.len(), 1);
    assert_eq!(parsed.file_changes[0].path, "src/main.rs");
    assert_eq!(parsed.file_changes[0].operation, "update");
    assert_eq!(parsed.file_changes[0].tool_name.as_deref(), Some("rewrite"));
    assert!(parsed.file_changes[0].before_truncated);
    assert!(!parsed.file_changes[0].after_truncated);
    assert!(parsed.warnings.is_empty());
}
