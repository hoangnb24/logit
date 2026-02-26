use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

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

#[test]
fn ingest_failure_prints_start_and_failure_progress() {
    let temp = unique_temp_dir("logit-progress-ingest-fail");
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
        .args(["ingest", "refresh"])
        .output()
        .expect("ingest command should execute");

    assert_eq!(
        output.status.code(),
        Some(1),
        "ingest refresh should return runtime failure when events input is missing"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("ingest failure should emit JSON envelope");
    assert_eq!(envelope.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        envelope.get("command").and_then(Value::as_str),
        Some("ingest.refresh")
    );
    assert!(
        envelope
            .get("generated_at_utc")
            .and_then(Value::as_str)
            .is_some()
    );
    assert!(envelope.get("meta").and_then(Value::as_object).is_some());
    assert!(envelope.get("warnings").and_then(Value::as_array).is_some());
    assert_eq!(
        envelope.pointer("/error/code").and_then(Value::as_str),
        Some("ingest_events_missing")
    );
    assert!(
        envelope
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("ingest refresh failed"))
    );
    assert!(
        stderr.trim().is_empty(),
        "ingest JSON-only error should not use stderr"
    );
}

#[test]
fn ingest_success_emits_machine_readable_json_report() {
    let temp = unique_temp_dir("logit-progress-ingest-success");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");
    write_valid_events_jsonl(&out_dir.join("events.jsonl"));

    let output = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .args(["ingest", "refresh"])
        .output()
        .expect("ingest command should execute");

    assert_eq!(
        output.status.code(),
        Some(0),
        "ingest refresh should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("ingest success should emit JSON envelope");
    assert_eq!(envelope.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        envelope.get("command").and_then(Value::as_str),
        Some("ingest.refresh")
    );
    assert_eq!(
        envelope
            .pointer("/data/schema_version")
            .and_then(Value::as_str),
        Some("logit.ingest-report.v1")
    );
    assert!(
        envelope
            .pointer("/meta/artifact_path")
            .and_then(Value::as_str)
            .is_some()
    );
    assert!(
        stderr.trim().is_empty(),
        "ingest JSON-only success should not use stderr"
    );
}

#[test]
fn query_success_emits_runtime_metadata_envelope() {
    let temp = unique_temp_dir("logit-progress-query-success");
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
        .args(["query", "sql", "select 1 as value"])
        .output()
        .expect("query command should execute");

    assert_eq!(
        output.status.code(),
        Some(0),
        "query sql should succeed for a valid read-only statement"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("query success should emit JSON envelope");
    assert_eq!(envelope.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        envelope.get("command").and_then(Value::as_str),
        Some("query.sql")
    );
    assert!(
        envelope
            .get("generated_at_utc")
            .and_then(Value::as_str)
            .is_some()
    );
    assert_eq!(
        envelope
            .pointer("/data/rows/0/value")
            .and_then(Value::as_i64),
        Some(1)
    );
    assert_eq!(
        envelope.pointer("/meta/row_count").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        envelope.pointer("/meta/truncated").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        envelope.pointer("/meta/row_cap").and_then(Value::as_u64),
        Some(1000)
    );
    assert!(
        envelope
            .pointer("/meta/duration_ms")
            .and_then(Value::as_u64)
            .is_some()
    );
    assert_eq!(
        envelope
            .pointer("/meta/params_count")
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        envelope
            .pointer("/meta/diagnostics/statement_kind")
            .and_then(Value::as_str),
        Some("select")
    );
    assert!(
        envelope
            .pointer("/meta/diagnostics/latency_bucket")
            .and_then(Value::as_str)
            .is_some()
    );
    assert_eq!(
        envelope
            .pointer("/meta/diagnostics/truncation_reason")
            .and_then(Value::as_str),
        Some("none")
    );
    assert_eq!(
        envelope
            .pointer("/meta/diagnostics/returned_rows")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert!(envelope.get("meta").and_then(Value::as_object).is_some());
    assert!(envelope.get("warnings").and_then(Value::as_array).is_some());
    assert!(
        stderr.trim().is_empty(),
        "query JSON-only success should not use stderr"
    );
}

#[test]
fn query_params_and_row_cap_emit_bound_and_truncation_metadata() {
    let temp = unique_temp_dir("logit-progress-query-params-row-cap");
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
        .args([
            "query",
            "sql",
            "with nums(n) as (values (?1), (?2), (?3)) select n from nums order by n",
            "--params",
            "[3,1,2]",
            "--row-cap",
            "2",
        ])
        .output()
        .expect("query command should execute");

    assert_eq!(output.status.code(), Some(0), "query sql should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("query success should emit JSON envelope");
    assert_eq!(
        envelope.pointer("/data/rows/0/n").and_then(Value::as_i64),
        Some(1)
    );
    assert_eq!(
        envelope.pointer("/data/rows/1/n").and_then(Value::as_i64),
        Some(2)
    );
    assert_eq!(
        envelope.pointer("/meta/row_count").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        envelope.pointer("/meta/truncated").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        envelope.pointer("/meta/row_cap").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        envelope
            .pointer("/meta/params_count")
            .and_then(Value::as_u64),
        Some(3)
    );
    assert_eq!(
        envelope
            .pointer("/meta/diagnostics/statement_kind")
            .and_then(Value::as_str),
        Some("with_select")
    );
    assert_eq!(
        envelope
            .pointer("/meta/diagnostics/has_order_by")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        envelope
            .pointer("/meta/diagnostics/truncation_reason")
            .and_then(Value::as_str),
        Some("row_cap_reached")
    );
    assert_eq!(
        envelope
            .pointer("/meta/diagnostics/returned_rows")
            .and_then(Value::as_u64),
        Some(2)
    );
}

#[test]
fn query_multi_statement_is_rejected_with_guardrail_error_envelope() {
    let temp = unique_temp_dir("logit-progress-query-guardrail");
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
        .args(["query", "sql", "select 1; select 2"])
        .output()
        .expect("query command should execute");

    assert_eq!(
        output.status.code(),
        Some(1),
        "guardrail violation should fail with runtime failure exit code"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("guardrail rejection should emit JSON envelope");

    assert_eq!(envelope.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        envelope.get("command").and_then(Value::as_str),
        Some("query.sql")
    );
    assert_eq!(
        envelope.pointer("/error/code").and_then(Value::as_str),
        Some("sql_guardrail_violation")
    );
    assert!(
        envelope
            .pointer("/error/details/violation/reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason == "multi_statement")
    );
    assert!(
        stderr.trim().is_empty(),
        "query JSON-only error should not use stderr"
    );
}

#[test]
fn query_schema_emits_machine_readable_table_and_view_metadata() {
    let temp = unique_temp_dir("logit-progress-query-schema");
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
        .args(["query", "schema"])
        .output()
        .expect("query schema command should execute");

    assert_eq!(output.status.code(), Some(0), "query schema should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("query schema success should emit JSON envelope");
    assert_eq!(envelope.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        envelope.get("command").and_then(Value::as_str),
        Some("query.schema")
    );

    let tables = envelope
        .pointer("/data/tables")
        .and_then(Value::as_array)
        .expect("query schema should expose `tables` array");
    let views = envelope
        .pointer("/data/views")
        .and_then(Value::as_array)
        .expect("query schema should expose `views` array");
    assert!(
        !tables.is_empty(),
        "tables metadata should include canonical schema tables"
    );
    assert!(
        !views.is_empty(),
        "views metadata should include semantic analytics views"
    );
    assert!(
        tables.iter().any(|table| {
            table.pointer("/name").and_then(Value::as_str) == Some("agentlog_events")
                && table.pointer("/kind").and_then(Value::as_str) == Some("table")
                && table.pointer("/internal").and_then(Value::as_bool) == Some(false)
                && table
                    .pointer("/columns")
                    .and_then(Value::as_array)
                    .is_some_and(|columns| {
                        columns.iter().any(|column| {
                            column.pointer("/name").and_then(Value::as_str) == Some("event_id")
                        })
                    })
        }),
        "agentlog_events table metadata should include event_id column"
    );
    assert!(
        views.iter().any(|view| {
            view.pointer("/name").and_then(Value::as_str) == Some("v_tool_calls")
                && view.pointer("/kind").and_then(Value::as_str) == Some("view")
        }),
        "v_tool_calls view metadata should be present"
    );

    assert_eq!(
        envelope
            .pointer("/meta/table_count")
            .and_then(Value::as_u64),
        Some(tables.len() as u64)
    );
    assert_eq!(
        envelope.pointer("/meta/view_count").and_then(Value::as_u64),
        Some(views.len() as u64)
    );
    assert_eq!(
        envelope
            .pointer("/meta/object_count")
            .and_then(Value::as_u64),
        Some((tables.len() + views.len()) as u64)
    );
    assert_eq!(
        envelope
            .pointer("/meta/include_internal")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(
        stderr.trim().is_empty(),
        "query schema JSON-only success should not use stderr"
    );
}

#[test]
fn query_schema_include_internal_surfaces_schema_meta_table() {
    let temp = unique_temp_dir("logit-progress-query-schema-internal");
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
        .args(["query", "schema", "--include-internal"])
        .output()
        .expect("query schema --include-internal command should execute");

    assert_eq!(output.status.code(), Some(0), "query schema should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("query schema success should emit JSON envelope");
    assert_eq!(
        envelope
            .pointer("/meta/include_internal")
            .and_then(Value::as_bool),
        Some(true)
    );
    let tables = envelope
        .pointer("/data/tables")
        .and_then(Value::as_array)
        .expect("query schema should expose `tables` array");
    assert!(
        tables.iter().any(|table| {
            table.pointer("/name").and_then(Value::as_str) == Some("agentlog_schema_meta")
                && table.pointer("/internal").and_then(Value::as_bool) == Some(true)
        }),
        "include-internal schema introspection should include agentlog_schema_meta"
    );
}

#[test]
fn query_catalog_emits_domain_concepts_for_agent_planning() {
    let temp = unique_temp_dir("logit-progress-query-catalog");
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
        .args(["query", "catalog"])
        .output()
        .expect("query catalog command should execute");

    assert_eq!(
        output.status.code(),
        Some(0),
        "query catalog should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("query catalog success should emit JSON envelope");
    assert_eq!(envelope.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        envelope.get("command").and_then(Value::as_str),
        Some("query.catalog")
    );
    assert_eq!(
        envelope
            .pointer("/data/schema_version")
            .and_then(Value::as_str),
        Some("logit.semantic-catalog.v1")
    );
    assert_eq!(
        envelope.pointer("/meta/verbose").and_then(Value::as_bool),
        Some(false)
    );

    let concepts = envelope
        .pointer("/data/concepts")
        .and_then(Value::as_array)
        .expect("query catalog should expose concepts array");
    assert!(
        concepts.iter().any(|concept| {
            concept.pointer("/concept_id").and_then(Value::as_str) == Some("tool_calls")
                && concept.pointer("/primary_relation").and_then(Value::as_str)
                    == Some("v_tool_calls")
        }),
        "catalog should include tool_calls concept mapped to v_tool_calls"
    );
    assert!(
        concepts.iter().any(
            |concept| concept.pointer("/concept_id").and_then(Value::as_str) == Some("sessions")
        ),
        "catalog should include sessions concept"
    );
    assert!(
        concepts.iter().any(
            |concept| concept.pointer("/concept_id").and_then(Value::as_str) == Some("adapters")
        ),
        "catalog should include adapters concept"
    );
    assert!(
        concepts.iter().any(
            |concept| concept.pointer("/concept_id").and_then(Value::as_str) == Some("quality")
        ),
        "catalog should include quality concept"
    );
    assert_eq!(
        envelope
            .pointer("/meta/concept_count")
            .and_then(Value::as_u64),
        Some(concepts.len() as u64)
    );
}

#[test]
fn query_catalog_verbose_includes_field_catalog_details() {
    let temp = unique_temp_dir("logit-progress-query-catalog-verbose");
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
        .args(["query", "catalog", "--verbose"])
        .output()
        .expect("query catalog --verbose command should execute");

    assert_eq!(
        output.status.code(),
        Some(0),
        "query catalog should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("query catalog success should emit JSON envelope");
    assert_eq!(
        envelope.pointer("/meta/verbose").and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        envelope
            .pointer("/data/concepts")
            .and_then(Value::as_array)
            .is_some_and(|concepts| {
                concepts.iter().any(|concept| {
                    concept.pointer("/concept_id").and_then(Value::as_str) == Some("tool_calls")
                        && concept
                            .pointer("/field_catalog")
                            .and_then(Value::as_array)
                            .is_some_and(|fields| !fields.is_empty())
                })
            }),
        "verbose catalog should include field_catalog entries for tool_calls"
    );
}

#[test]
fn query_benchmark_emits_per_question_results_and_artifact() {
    let temp = unique_temp_dir("logit-progress-query-benchmark");
    let home_dir = temp.join("home");
    let cwd = temp.join("cwd");
    let out_dir = temp.join("out");
    std::fs::create_dir_all(&home_dir).expect("home dir should be creatable");
    std::fs::create_dir_all(&cwd).expect("cwd dir should be creatable");
    std::fs::create_dir_all(&out_dir).expect("out dir should be creatable");
    write_valid_events_jsonl(&out_dir.join("events.jsonl"));

    let ingest_output = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .args(["ingest", "refresh"])
        .output()
        .expect("ingest refresh should execute before benchmark");
    assert_eq!(
        ingest_output.status.code(),
        Some(0),
        "ingest refresh should succeed before benchmark execution"
    );

    let corpus_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/benchmarks/answerability_question_corpus_v1.json");
    let output = Command::new(env!("CARGO_BIN_EXE_logit"))
        .args(["--home-dir"])
        .arg(&home_dir)
        .args(["--cwd"])
        .arg(&cwd)
        .args(["--out-dir"])
        .arg(&out_dir)
        .args(["query", "benchmark", "--corpus"])
        .arg(&corpus_path)
        .output()
        .expect("query benchmark command should execute");

    assert_eq!(
        output.status.code(),
        Some(0),
        "query benchmark should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("query benchmark success should emit JSON envelope");
    assert_eq!(envelope.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        envelope.get("command").and_then(Value::as_str),
        Some("query.benchmark")
    );
    assert_eq!(
        envelope
            .pointer("/data/schema_version")
            .and_then(Value::as_str),
        Some("logit.answerability-benchmark-report.v1")
    );
    assert_eq!(
        envelope.pointer("/data/corpus_id").and_then(Value::as_str),
        Some("canonical-user-question-corpus-v1")
    );
    assert_eq!(
        envelope
            .pointer("/data/preflight/semantic_concept_count")
            .and_then(Value::as_u64),
        Some(4)
    );
    let questions = envelope
        .pointer("/data/questions")
        .and_then(Value::as_array)
        .expect("benchmark report should contain per-question results");
    assert_eq!(
        questions.len(),
        16,
        "canonical corpus v1 currently has 16 questions"
    );
    assert!(
        questions.iter().all(|question| {
            question.pointer("/id").and_then(Value::as_str).is_some()
                && question.pointer("/query_interface").and_then(Value::as_str) == Some("query.sql")
                && question
                    .pointer("/column_names")
                    .and_then(Value::as_array)
                    .is_some()
        }),
        "benchmark should emit machine-readable per-question execution results"
    );
    assert_eq!(
        envelope
            .pointer("/data/summary/total_questions")
            .and_then(Value::as_u64),
        Some(16)
    );
    assert_eq!(
        envelope
            .pointer("/data/release_gate/passed")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        envelope
            .pointer("/data/release_gate/minimum_total_score_pct")
            .and_then(Value::as_f64),
        Some(95.0)
    );
    assert_eq!(
        envelope
            .pointer("/data/release_gate/maximum_failed_questions")
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        envelope
            .pointer("/meta/release_gate_passed")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        envelope
            .pointer("/meta/question_count")
            .and_then(Value::as_u64),
        Some(16)
    );

    let artifact_path = envelope
        .pointer("/meta/artifact_path")
        .and_then(Value::as_str)
        .expect("benchmark success should report artifact path");
    assert!(
        std::path::Path::new(artifact_path).exists(),
        "benchmark artifact path should be written"
    );
    let artifact_value: Value = serde_json::from_str(
        &std::fs::read_to_string(artifact_path).expect("artifact should be readable"),
    )
    .expect("artifact should be valid JSON");
    assert_eq!(
        artifact_value
            .pointer("/schema_version")
            .and_then(Value::as_str),
        Some("logit.answerability-benchmark-report.v1")
    );
    assert_eq!(
        artifact_value
            .pointer("/release_gate/passed")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        artifact_value
            .pointer("/release_gate/failed_checks")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    assert!(
        stderr.trim().is_empty(),
        "query benchmark JSON-only success should not use stderr"
    );
}

#[test]
fn query_benchmark_invalid_corpus_path_emits_error_envelope() {
    let temp = unique_temp_dir("logit-progress-query-benchmark-invalid-corpus");
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
        .args([
            "query",
            "benchmark",
            "--corpus",
            "/definitely/missing/answerability_question_corpus_v1.json",
        ])
        .output()
        .expect("query benchmark command should execute");

    assert_eq!(
        output.status.code(),
        Some(1),
        "invalid corpus path should fail"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let envelope: Value =
        serde_json::from_str(&stdout).expect("benchmark failure should emit JSON envelope");
    assert_eq!(envelope.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        envelope.get("command").and_then(Value::as_str),
        Some("query.benchmark")
    );
    assert_eq!(
        envelope.pointer("/error/code").and_then(Value::as_str),
        Some("query_benchmark_corpus_invalid")
    );
    assert!(
        stderr.trim().is_empty(),
        "query benchmark JSON-only failure should not use stderr"
    );
}
