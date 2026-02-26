use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use logit::cli::commands::inspect::{inspect_target, render_text_report};

fn unique_temp_file(prefix: &str, extension: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}.{extension}"))
}

#[test]
fn inspect_reports_normalized_jsonl_summary() {
    let file = unique_temp_file("logit-inspect-normalized", "jsonl");
    std::fs::write(
        &file,
        r#"{"schema_version":"agentlog.v1","event_id":"evt-1","adapter_name":"codex","event_type":"prompt"}
{"schema_version":"agentlog.v1","event_id":"evt-2","adapter_name":"claude","event_type":"response"}
"#,
    )
    .expect("fixture write should succeed");

    let report = inspect_target(&file).expect("inspect should succeed");
    assert_eq!(report.classification, "jsonl");
    assert_eq!(
        report
            .line_counts
            .as_ref()
            .expect("line counts should exist")
            .json_rows,
        2
    );

    let normalized = report
        .normalized_event_summary
        .as_ref()
        .expect("normalized summary should exist");
    assert_eq!(normalized.normalized_rows, 2);
    assert_eq!(normalized.adapter_counts.get("claude"), Some(&1));
    assert_eq!(normalized.adapter_counts.get("codex"), Some(&1));
    assert_eq!(normalized.event_type_counts.get("prompt"), Some(&1));
    assert_eq!(normalized.event_type_counts.get("response"), Some(&1));

    let text = render_text_report(&report);
    assert!(text.contains("classification: jsonl"));
    assert!(text.contains("normalized_event_summary.normalized_rows: 2"));
}

#[test]
fn inspect_tracks_invalid_jsonl_rows_as_warnings() {
    let file = unique_temp_file("logit-inspect-invalid", "jsonl");
    std::fs::write(
        &file,
        r#"{"event_type":"prompt"}
not-json
{"event_type":"response"}
"#,
    )
    .expect("fixture write should succeed");

    let report = inspect_target(&file).expect("inspect should succeed");
    let line_counts = report.line_counts.expect("line counts should exist");
    assert_eq!(line_counts.total_lines, 3);
    assert_eq!(line_counts.non_empty_lines, 3);
    assert_eq!(line_counts.json_rows, 2);
    assert_eq!(line_counts.invalid_json_rows, 1);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("line 2: invalid JSON"))
    );
}

#[test]
fn inspect_json_document_reports_root_shape() {
    let file = unique_temp_file("logit-inspect-json", "json");
    std::fs::write(
        &file,
        r#"{"messages":[{"role":"user"}],"conversation_id":"gemini-c-1"}"#,
    )
    .expect("fixture write should succeed");

    let report = inspect_target(&file).expect("inspect should succeed");
    assert_eq!(report.classification, "json");
    let json_document = report
        .json_document
        .expect("json document summary should exist");
    assert_eq!(json_document.root_type, "object");
    assert_eq!(
        json_document.object_keys,
        vec!["conversation_id".to_string(), "messages".to_string()]
    );
    assert!(json_document.array_length.is_none());
}

#[test]
fn inspect_returns_error_for_missing_target() {
    let missing_file = unique_temp_file("logit-inspect-missing", "jsonl");
    let error = inspect_target(&missing_file).expect_err("missing file should fail");
    assert!(error.to_string().contains("does not exist"));
}
