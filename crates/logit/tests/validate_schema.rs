use std::collections::BTreeMap;

use logit::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, SchemaVersion, TimestampQuality,
};
use logit::validate::{
    ValidationIssueKind, ValidationIssueSeverity, ValidationMode,
    validate_jsonl_against_generated_schema,
};

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

#[test]
fn validates_well_formed_agentlog_jsonl_rows() {
    let line1 = serde_json::to_string(&sample_event("1")).expect("event serializes");
    let line2 = serde_json::to_string(&sample_event("2")).expect("event serializes");
    let input = format!("{line1}\n{line2}\n");

    let report = validate_jsonl_against_generated_schema(&input, ValidationMode::Baseline);
    assert_eq!(report.schema_version, "agentlog.v1");
    assert_eq!(report.total_records, 2);
    assert_eq!(report.records_validated, 2);
    assert_eq!(report.errors, 0);
    assert!(report.issues.is_empty());
}

#[test]
fn reports_invalid_json_line_numbers() {
    let line1 = serde_json::to_string(&sample_event("1")).expect("event serializes");
    let input = format!("{line1}\nnot-json\n");

    let report = validate_jsonl_against_generated_schema(&input, ValidationMode::Strict);
    assert_eq!(report.total_records, 2);
    assert_eq!(report.records_validated, 1);
    assert_eq!(report.errors, 1);
    assert_eq!(report.issues[0].line, 2);
    assert_eq!(report.issues[0].kind, ValidationIssueKind::InvalidJson);
    assert_eq!(report.issues[0].severity, ValidationIssueSeverity::Error);
    assert!(report.issues[0].detail.contains("invalid JSON"));
}

#[test]
fn reports_schema_violations_with_line_numbers() {
    let input = r#"{"schema_version":"agentlog.v1"}"#;

    let report = validate_jsonl_against_generated_schema(input, ValidationMode::Baseline);
    assert_eq!(report.total_records, 1);
    assert_eq!(report.records_validated, 0);
    assert_eq!(report.errors, 1);
    assert_eq!(report.issues[0].line, 1);
    assert_eq!(report.issues[0].kind, ValidationIssueKind::SchemaViolation);
    assert_eq!(report.issues[0].severity, ValidationIssueSeverity::Error);
    assert!(report.issues[0].detail.contains("missing required fields"));
}
