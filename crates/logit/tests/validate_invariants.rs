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
fn baseline_mode_treats_missing_content_as_warning() {
    let mut event = sample_event("1");
    event.content_text = None;
    event.content_excerpt = None;

    let input = format!(
        "{}\n",
        serde_json::to_string(&event).expect("event should serialize")
    );
    let report = validate_jsonl_against_generated_schema(&input, ValidationMode::Baseline);

    assert_eq!(report.errors, 0);
    assert!(report.warnings >= 1);
    assert!(report.issues.iter().any(|issue| issue.kind
        == ValidationIssueKind::InvariantViolation
        && issue.severity == ValidationIssueSeverity::Warning
        && issue.detail.contains("content_text")));
}

#[test]
fn strict_mode_escalates_missing_content_to_error() {
    let mut event = sample_event("1");
    event.content_text = None;
    event.content_excerpt = None;

    let input = format!(
        "{}\n",
        serde_json::to_string(&event).expect("event should serialize")
    );
    let report = validate_jsonl_against_generated_schema(&input, ValidationMode::Strict);

    assert!(report.errors >= 1);
    assert!(report.issues.iter().any(|issue| issue.kind
        == ValidationIssueKind::InvariantViolation
        && issue.severity == ValidationIssueSeverity::Error
        && issue.detail.contains("content_text")));
}

#[test]
fn timestamp_mismatch_is_reported_as_error() {
    let mut event = sample_event("1");
    event.timestamp_unix_ms += 1;

    let input = format!(
        "{}\n",
        serde_json::to_string(&event).expect("event should serialize")
    );
    let report = validate_jsonl_against_generated_schema(&input, ValidationMode::Baseline);

    assert!(report.errors >= 1);
    assert!(report.issues.iter().any(|issue| issue.kind
        == ValidationIssueKind::InvariantViolation
        && issue.severity == ValidationIssueSeverity::Error
        && issue.detail.contains("timestamp mismatch")));
}

#[test]
fn empty_hashes_are_rejected() {
    let mut event = sample_event("1");
    event.raw_hash = "  ".to_string();
    event.canonical_hash.clear();

    let input = format!(
        "{}\n",
        serde_json::to_string(&event).expect("event should serialize")
    );
    let report = validate_jsonl_against_generated_schema(&input, ValidationMode::Baseline);

    assert!(report.errors >= 2);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.detail == "raw_hash must be non-empty")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.detail == "canonical_hash must be non-empty")
    );
}

#[test]
fn invariant_catalog_executor_emits_combined_findings_deterministically() {
    let mut event = sample_event("1");
    event.timestamp_unix_ms += 1;
    event.raw_hash.clear();
    event.canonical_hash = " ".to_string();
    event.content_text = None;
    event.content_excerpt = None;

    let input = format!(
        "{}\n",
        serde_json::to_string(&event).expect("event should serialize")
    );
    let report = validate_jsonl_against_generated_schema(&input, ValidationMode::Strict);

    let details = report
        .issues
        .iter()
        .map(|issue| issue.detail.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        details,
        vec![
            "canonical_hash must be non-empty",
            "content_text is empty for user/assistant message record",
            "content_text null-rate 1.00 exceeds 0.20 threshold",
            "raw_hash must be non-empty",
            "timestamp mismatch: timestamp_utc=1771977600000, timestamp_unix_ms=1771977600001",
        ]
    );
}

#[test]
fn quality_scorecard_reports_dimension_breakdown_and_weakest_axes() {
    let mut first = sample_event("1");
    first.timestamp_quality = TimestampQuality::Exact;

    let mut second = sample_event("2");
    second.timestamp_quality = TimestampQuality::Fallback;
    second.content_text = None;
    second.content_excerpt = None;

    let input = format!(
        "{}\n{}\n",
        serde_json::to_string(&first).expect("event should serialize"),
        serde_json::to_string(&second).expect("event should serialize")
    );
    let report = validate_jsonl_against_generated_schema(&input, ValidationMode::Baseline);

    assert_eq!(report.quality_scorecard.coverage_score, 100);
    assert_eq!(report.quality_scorecard.parse_success_score, 100);
    assert_eq!(report.quality_scorecard.content_completeness_score, 50);
    assert_eq!(report.quality_scorecard.timestamp_quality_score, 65);
    assert_eq!(report.quality_scorecard.overall_score, 79);
    assert_eq!(
        report.quality_scorecard.weakest_dimensions,
        vec![
            "content_completeness".to_string(),
            "timestamp_quality".to_string()
        ]
    );
}
