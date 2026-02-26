use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

fn write_valid_events_jsonl(path: &std::path::Path) {
    let event = json!({
        "schema_version": "agentlog.v1",
        "event_id": "evt-1",
        "run_id": "run-1",
        "sequence_global": 0,
        "source_kind": "codex",
        "source_path": "/tmp/events.jsonl",
        "source_record_locator": "line:1",
        "adapter_name": "codex",
        "record_format": "message",
        "event_type": "prompt",
        "role": "user",
        "content_text": "hello world",
        "content_excerpt": "hello world",
        "timestamp_utc": "2026-02-25T00:00:00Z",
        "timestamp_unix_ms": 1771977600000u64,
        "timestamp_quality": "exact",
        "raw_hash": "raw-1",
        "canonical_hash": "canonical-1"
    });
    let line = serde_json::to_string(&event).expect("event should serialize");
    std::fs::write(path, format!("{line}\n")).expect("events file should be writable");
}

#[test]
fn normalize_prints_stage_progress_and_summary() {
    let temp = unique_temp_dir("logit-progress-normalize");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");

    let output = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .args(["normalize", "--source-root"])
        .arg(&home_dir)
        .output()
        .expect("normalize command should execute");

    assert!(output.status.success(), "normalize should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("logit: starting `normalize`"));
    assert!(stdout.contains("normalize: start"));
    assert!(stdout.contains("normalize: stage orchestrate"));
    assert!(stdout.contains("normalize: checkpoint orchestrate_complete"));
    assert!(stdout.contains("normalize: adapter_health"));
    assert!(stdout.contains("normalize: stage write_normalize_artifacts"));
    assert!(stdout.contains("normalize: checkpoint events_written"));
    assert!(stdout.contains("normalize: checkpoint schema_written"));
    assert!(stdout.contains("normalize: checkpoint stats_written"));
    assert!(stdout.contains("normalize: stage write_discovery_artifacts"));
    assert!(stdout.contains("normalize: checkpoint discovery_written"));
    assert!(stdout.contains("normalize: complete"));
    assert!(stdout.contains("normalize: artifacts"));
    assert!(stdout.contains("normalize: next"));
    assert!(stdout.contains("logit: completed `normalize`"));
}

#[test]
fn validate_prints_success_summary() {
    let temp = unique_temp_dir("logit-progress-validate-ok");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");
    let input = temp.join("events.jsonl");
    write_valid_events_jsonl(&input);

    let output = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .arg("validate")
        .arg(&input)
        .output()
        .expect("validate command should execute");

    assert!(output.status.success(), "validate should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("logit: starting `validate`"));
    assert!(stdout.contains("validate: start"));
    assert!(stdout.contains("validate: report status=pass"));
    assert!(stdout.contains("validate: complete"));
    assert!(stdout.contains("logit: completed `validate`"));
}

#[test]
fn validate_prints_failure_summary_on_stderr() {
    let temp = unique_temp_dir("logit-progress-validate-fail");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");
    let input = temp.join("events.jsonl");
    std::fs::write(&input, "not-json\n").expect("invalid input should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .arg("validate")
        .arg(&input)
        .output()
        .expect("validate command should execute");

    assert_eq!(
        output.status.code(),
        Some(2),
        "validate should fail with code 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("validate: failed"));
    assert!(stderr.contains("logit: failed `validate`"));
}
