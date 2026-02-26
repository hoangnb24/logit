use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use logit::cli::commands::validate::{ValidateArgs, run as run_validate};
use logit::config::RuntimePaths;
use logit::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, SchemaVersion, TimestampQuality,
};
use logit::validate::build_artifact_layout;
use serde_json::Value;

fn sample_event(event_id: &str) -> AgentLogEvent {
    AgentLogEvent {
        schema_version: SchemaVersion::AgentLogV1,
        event_id: event_id.to_string(),
        run_id: "run-1".to_string(),
        sequence_global: 0,
        sequence_source: Some(0),
        source_kind: AgentSource::Codex,
        source_path: "/tmp/events.jsonl".to_string(),
        source_record_locator: format!("line:{event_id}"),
        source_record_hash: Some(format!("source-{event_id}")),
        adapter_name: AgentSource::Codex,
        adapter_version: Some("v1".to_string()),
        record_format: RecordFormat::Message,
        event_type: EventType::Prompt,
        role: ActorRole::User,
        timestamp_utc: "2026-02-25T00:00:00Z".to_string(),
        timestamp_unix_ms: 1_771_977_600_000,
        timestamp_quality: TimestampQuality::Exact,
        session_id: Some("session-1".to_string()),
        conversation_id: Some("conversation-1".to_string()),
        turn_id: Some("turn-1".to_string()),
        parent_event_id: None,
        actor_id: Some("actor-1".to_string()),
        actor_name: Some("Agent".to_string()),
        provider: Some("openai".to_string()),
        model: Some("gpt-5".to_string()),
        content_text: Some("hello world".to_string()),
        content_excerpt: Some("hello world".to_string()),
        content_mime: Some("text/plain".to_string()),
        tool_name: None,
        tool_call_id: None,
        tool_arguments_json: None,
        tool_result_text: None,
        input_tokens: Some(1),
        output_tokens: Some(2),
        total_tokens: Some(3),
        cost_usd: Some(0.1),
        tags: Vec::new(),
        flags: Vec::new(),
        pii_redacted: Some(false),
        warnings: Vec::new(),
        errors: Vec::new(),
        raw_hash: format!("raw-{event_id}"),
        canonical_hash: format!("canonical-{event_id}"),
        metadata: BTreeMap::new(),
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

#[test]
fn validate_artifact_layout_uses_contract_filename() {
    let layout = build_artifact_layout(Path::new("/tmp/logit-out"));
    assert_eq!(
        layout.report_json,
        Path::new("/tmp/logit-out/validate/report.json")
    );
}

#[test]
fn validate_command_emits_machine_readable_report_artifact() {
    let out_dir = unique_temp_dir("logit-validate-pass");
    let input_path = out_dir.join("events.jsonl");
    let event_line = serde_json::to_string(&sample_event("1")).expect("event should serialize");
    std::fs::create_dir_all(&out_dir).expect("temp out dir should be creatable");
    std::fs::write(&input_path, format!("{event_line}\n")).expect("input should be writable");

    let runtime_paths = RuntimePaths {
        home_dir: PathBuf::from("/tmp/logit-home"),
        cwd: PathBuf::from("/tmp/logit-cwd"),
        out_dir: out_dir.clone(),
    };
    let args = ValidateArgs {
        input: input_path,
        strict: false,
    };

    run_validate(&args, &runtime_paths).expect("validate command should succeed");

    let report_path = build_artifact_layout(&out_dir).report_json;
    assert!(
        report_path.exists(),
        "validate report artifact should exist"
    );

    let report: Value = serde_json::from_str(
        &std::fs::read_to_string(&report_path).expect("report artifact should be readable"),
    )
    .expect("report artifact should be valid json");

    assert_eq!(report.get("status").and_then(Value::as_str), Some("pass"));
    assert_eq!(
        report.get("interpreted_exit_code").and_then(Value::as_i64),
        Some(0)
    );
    assert_eq!(report.get("errors").and_then(Value::as_u64), Some(0));
    assert_eq!(report.get("warnings").and_then(Value::as_u64), Some(0));
    assert_eq!(
        report
            .pointer("/quality_scorecard/overall_score")
            .and_then(Value::as_u64),
        Some(100)
    );
    assert_eq!(
        report
            .pointer("/quality_scorecard/coverage_score")
            .and_then(Value::as_u64),
        Some(100)
    );
    assert_eq!(
        report
            .pointer("/quality_scorecard/parse_success_score")
            .and_then(Value::as_u64),
        Some(100)
    );
    assert_eq!(
        report
            .pointer("/quality_scorecard/content_completeness_score")
            .and_then(Value::as_u64),
        Some(100)
    );
    assert_eq!(
        report
            .pointer("/quality_scorecard/timestamp_quality_score")
            .and_then(Value::as_u64),
        Some(100)
    );
    assert_eq!(
        report
            .pointer("/per_agent_summary/codex/records_validated")
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[test]
fn validate_command_writes_report_before_returning_validation_error() {
    let out_dir = unique_temp_dir("logit-validate-fail");
    let input_path = out_dir.join("events.jsonl");
    let event_line = serde_json::to_string(&sample_event("1")).expect("event should serialize");
    std::fs::create_dir_all(&out_dir).expect("temp out dir should be creatable");
    std::fs::write(&input_path, format!("{event_line}\nnot-json\n"))
        .expect("input should be writable");

    let runtime_paths = RuntimePaths {
        home_dir: PathBuf::from("/tmp/logit-home"),
        cwd: PathBuf::from("/tmp/logit-cwd"),
        out_dir: out_dir.clone(),
    };
    let args = ValidateArgs {
        input: input_path,
        strict: true,
    };

    let err = run_validate(&args, &runtime_paths).expect_err("validate should fail");
    assert!(
        err.to_string().contains("validation failed"),
        "unexpected error: {err}"
    );

    let report_path = build_artifact_layout(&out_dir).report_json;
    assert!(
        report_path.exists(),
        "validate report artifact should exist even on validation failure"
    );

    let report: Value = serde_json::from_str(
        &std::fs::read_to_string(&report_path).expect("report artifact should be readable"),
    )
    .expect("report artifact should be valid json");

    assert_eq!(report.get("status").and_then(Value::as_str), Some("fail"));
    assert_eq!(
        report.get("interpreted_exit_code").and_then(Value::as_i64),
        Some(2)
    );
    assert!(
        report.get("errors").and_then(Value::as_u64).unwrap_or(0) >= 1,
        "expected at least one validation error"
    );
    assert!(
        report
            .pointer("/per_agent_summary/unknown/errors")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 1,
        "invalid line should count as unknown-agent error"
    );
    assert_eq!(
        report
            .pointer("/quality_scorecard/overall_score")
            .and_then(Value::as_u64),
        Some(75)
    );
    assert_eq!(
        report
            .pointer("/quality_scorecard/coverage_score")
            .and_then(Value::as_u64),
        Some(50)
    );
    assert_eq!(
        report
            .pointer("/quality_scorecard/parse_success_score")
            .and_then(Value::as_u64),
        Some(50)
    );
    assert_eq!(
        report
            .pointer("/quality_scorecard/weakest_dimensions/0")
            .and_then(Value::as_str),
        Some("coverage")
    );
    assert_eq!(
        report
            .pointer("/quality_scorecard/weakest_dimensions/1")
            .and_then(Value::as_str),
        Some("parse_success")
    );
}
