use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use logit::cli::commands::normalize::{NormalizeArgs, run as run_normalize};
use logit::cli::commands::snapshot::{SnapshotArgs, run as run_snapshot};
use logit::cli::commands::validate::{ValidateArgs, run as run_validate};
use logit::config::RuntimePaths;
use logit::discovery::build_artifact_layout as build_discovery_artifact_layout;
use logit::normalize::build_artifact_layout as build_normalize_artifact_layout;
use logit::snapshot::{
    build_artifact_layout as build_snapshot_artifact_layout, verify_snapshot_artifacts_parseable,
};
use logit::validate::build_artifact_layout as build_validate_artifact_layout;
use serde_json::Value;

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

fn write_file(path: &Path, content: &str) {
    let parent = path.parent().expect("test path should have parent");
    std::fs::create_dir_all(parent).expect("test parent directory should be creatable");
    std::fs::write(path, content).expect("test file should be writable");
}

fn seed_workflow_sources(source_root: &Path, home_dir: &Path) {
    write_file(
        &source_root.join(".codex/sessions/rollout_primary.jsonl"),
        include_str!("../../../fixtures/codex/rollout_primary.jsonl"),
    );
    write_file(
        &source_root.join(".claude/projects/project_session.jsonl"),
        include_str!("../../../fixtures/claude/project_session.jsonl"),
    );
    write_file(
        &home_dir.join(".zsh_history"),
        r#": 1740467001:0;claude --resume
: 1740467002:0;codex --full-auto
: 1740467003:0;codex --full-auto
"#,
    );
}

#[test]
fn end_to_end_snapshot_normalize_validate_workflow_emits_consistent_artifacts() {
    let source_root = unique_temp_dir("logit-workflow-source");
    let home_dir = unique_temp_dir("logit-workflow-home");
    let out_dir = unique_temp_dir("logit-workflow-out");
    seed_workflow_sources(&source_root, &home_dir);

    let runtime_paths = RuntimePaths {
        home_dir: home_dir.clone(),
        cwd: PathBuf::from("/tmp/logit-cwd"),
        out_dir: out_dir.clone(),
    };

    let snapshot_args = SnapshotArgs {
        source_root: Some(source_root.clone()),
        sample_size: 2,
    };
    run_snapshot(&snapshot_args, &runtime_paths).expect("snapshot command should succeed");

    let normalize_args = NormalizeArgs {
        source_root: Some(source_root),
        fail_fast: false,
    };
    run_normalize(&normalize_args, &runtime_paths).expect("normalize command should succeed");

    let normalize_layout = build_normalize_artifact_layout(&out_dir);
    let validate_args = ValidateArgs {
        input: normalize_layout.events_jsonl.clone(),
        strict: false,
    };
    run_validate(&validate_args, &runtime_paths).expect("validate command should succeed");

    let snapshot_layout = build_snapshot_artifact_layout(&out_dir);
    let discovery_layout = build_discovery_artifact_layout(&out_dir);
    let validate_layout = build_validate_artifact_layout(&out_dir);

    assert!(snapshot_layout.index_json.exists());
    assert!(snapshot_layout.samples_jsonl.exists());
    assert!(snapshot_layout.schema_profile_json.exists());
    assert!(normalize_layout.events_jsonl.exists());
    assert!(normalize_layout.schema_json.exists());
    assert!(normalize_layout.stats_json.exists());
    assert!(discovery_layout.sources_json.exists());
    assert!(discovery_layout.zsh_history_usage_json.exists());
    assert!(validate_layout.report_json.exists());

    verify_snapshot_artifacts_parseable(&snapshot_layout)
        .expect("snapshot artifacts should be parseable and internally consistent");

    let events_text = std::fs::read_to_string(&normalize_layout.events_jsonl)
        .expect("normalize events artifact should be readable");
    let event_rows = events_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("event row should be valid JSON"))
        .collect::<Vec<_>>();
    assert!(!event_rows.is_empty(), "expected normalized events");

    let stats: Value = serde_json::from_str(
        &std::fs::read_to_string(&normalize_layout.stats_json).expect("stats artifact readable"),
    )
    .expect("stats artifact should parse");
    assert_eq!(
        stats
            .pointer("/counts/records_emitted")
            .and_then(Value::as_u64),
        Some(event_rows.len() as u64)
    );

    let schema_doc: Value = serde_json::from_str(
        &std::fs::read_to_string(&normalize_layout.schema_json).expect("schema artifact readable"),
    )
    .expect("schema artifact should parse");
    assert!(
        schema_doc.get("$schema").is_some() || schema_doc.get("schema").is_some(),
        "expected JSON schema document shape"
    );

    let discovery_sources: Value = serde_json::from_str(
        &std::fs::read_to_string(&discovery_layout.sources_json)
            .expect("discovery sources artifact readable"),
    )
    .expect("discovery sources artifact should parse");
    assert!(
        discovery_sources
            .get("total_sources")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
    );

    let validate_report: Value = serde_json::from_str(
        &std::fs::read_to_string(&validate_layout.report_json).expect("validate report readable"),
    )
    .expect("validate report should parse");
    assert_eq!(
        validate_report
            .get("interpreted_exit_code")
            .and_then(Value::as_i64),
        Some(0)
    );
    assert_eq!(
        validate_report.get("errors").and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        validate_report.get("total_records").and_then(Value::as_u64),
        Some(event_rows.len() as u64)
    );
    assert_eq!(
        validate_report
            .get("records_validated")
            .and_then(Value::as_u64),
        Some(event_rows.len() as u64)
    );
}
