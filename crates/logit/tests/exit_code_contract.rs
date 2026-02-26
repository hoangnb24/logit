use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

const EXIT_SUCCESS: i32 = 0;
const EXIT_RUNTIME_FAILURE: i32 = 1;
const EXIT_VALIDATION_FAILURE: i32 = 2;
const EXIT_USAGE_ERROR: i32 = 64;

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
fn missing_required_args_exits_with_usage_code() {
    let status = Command::new(env!("CARGO_BIN_EXE_logit"))
        .arg("validate")
        .status()
        .expect("command should execute");

    assert_eq!(status.code(), Some(EXIT_USAGE_ERROR));
}

#[test]
fn runtime_path_resolution_failures_exit_with_runtime_code() {
    let temp = unique_temp_dir("logit-exit-runtime");
    std::fs::create_dir_all(&temp).expect("temp dir should be creatable");
    let input = temp.join("events.jsonl");
    write_valid_events_jsonl(&input);

    let status = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir", "relative", "validate"])
        .arg(&input)
        .status()
        .expect("command should execute");

    assert_eq!(status.code(), Some(EXIT_RUNTIME_FAILURE));
}

#[test]
fn validation_failures_exit_with_validation_code() {
    let temp = unique_temp_dir("logit-exit-validation-fail");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");

    let input = temp.join("invalid-events.jsonl");
    std::fs::write(&input, "not-json\n").expect("invalid input should be writable");

    let status = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .arg("validate")
        .arg(&input)
        .status()
        .expect("command should execute");

    assert_eq!(status.code(), Some(EXIT_VALIDATION_FAILURE));
}

#[test]
fn successful_validate_exits_zero() {
    let temp = unique_temp_dir("logit-exit-success");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");

    let input = temp.join("events.jsonl");
    write_valid_events_jsonl(&input);

    let status = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .arg("validate")
        .arg(&input)
        .status()
        .expect("command should execute");

    assert_eq!(status.code(), Some(EXIT_SUCCESS));
}

#[test]
fn ingest_refresh_missing_events_exits_runtime_failure_code() {
    let temp = unique_temp_dir("logit-exit-ingest-runtime-fail");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");

    let status = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .args(["ingest", "refresh", "--fail-fast"])
        .status()
        .expect("command should execute");

    assert_eq!(status.code(), Some(EXIT_RUNTIME_FAILURE));
}

#[test]
fn ingest_refresh_with_valid_events_exits_zero() {
    let temp = unique_temp_dir("logit-exit-ingest-success");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");
    write_valid_events_jsonl(&out_dir.join("events.jsonl"));

    let status = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .args(["ingest", "refresh"])
        .status()
        .expect("command should execute");

    assert_eq!(status.code(), Some(EXIT_SUCCESS));
}

#[test]
fn query_sql_read_only_success_exits_zero() {
    let temp = unique_temp_dir("logit-exit-query-runtime-fail");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");

    let status = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .args(["query", "sql", "select 1"])
        .status()
        .expect("command should execute");

    assert_eq!(status.code(), Some(EXIT_SUCCESS));
}

#[test]
fn query_benchmark_with_valid_corpus_exits_zero() {
    let temp = unique_temp_dir("logit-exit-query-benchmark-success");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");
    write_valid_events_jsonl(&out_dir.join("events.jsonl"));

    let ingest_status = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .args(["ingest", "refresh"])
        .status()
        .expect("ingest refresh should execute");
    assert_eq!(
        ingest_status.code(),
        Some(EXIT_SUCCESS),
        "ingest refresh should succeed before benchmark"
    );

    let corpus_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/benchmarks/answerability_question_corpus_v1.json");
    let status = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .args(["query", "benchmark", "--corpus"])
        .arg(&corpus_path)
        .status()
        .expect("query benchmark should execute");

    assert_eq!(status.code(), Some(EXIT_SUCCESS));
}

#[test]
fn query_benchmark_with_missing_corpus_exits_runtime_failure() {
    let temp = unique_temp_dir("logit-exit-query-benchmark-fail");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");

    let status = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .args([
            "query",
            "benchmark",
            "--corpus",
            "/definitely/missing/answerability_question_corpus_v1.json",
        ])
        .status()
        .expect("query benchmark should execute");

    assert_eq!(status.code(), Some(EXIT_RUNTIME_FAILURE));
}
