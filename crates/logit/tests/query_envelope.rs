use logit::models::{QUERY_ENVELOPE_SCHEMA_VERSION, QueryEnvelope, QueryEnvelopeCommandFailure};
use serde_json::json;

#[test]
fn ok_envelope_tracks_contract_fields() {
    let envelope = QueryEnvelope::ok(
        "query.sql",
        json!([
            {"adapter":"codex","events":42},
            {"adapter":"claude","events":7}
        ]),
    )
    .with_meta("row_count", json!(2))
    .with_warning("result_truncated", "truncated to row_cap")
    .with_warning_details(json!({"row_cap": 1000}));

    assert!(envelope.ok);
    assert_eq!(envelope.command, "query.sql");
    assert!(envelope.generated_at_utc.ends_with('Z'));
    assert!(envelope.data.is_some());
    assert_eq!(
        envelope.meta.get("schema_version"),
        Some(&json!(QUERY_ENVELOPE_SCHEMA_VERSION))
    );
    assert_eq!(envelope.meta.get("row_count"), Some(&json!(2)));
    assert_eq!(envelope.warnings.len(), 1);
    assert_eq!(envelope.warnings[0].code, "result_truncated");
    assert_eq!(envelope.warnings[0].message, "truncated to row_cap");
    assert_eq!(
        envelope.warnings[0].details.as_ref(),
        Some(&json!({"row_cap": 1000}))
    );
    assert!(envelope.error.is_none());
}

#[test]
fn ok_envelope_serializes_required_top_level_fields() {
    let envelope = QueryEnvelope::ok("query.schema", json!({"tables":[]}));
    let encoded = serde_json::to_value(&envelope).expect("envelope should serialize");

    let object = encoded
        .as_object()
        .expect("query envelope JSON should be object");
    assert_eq!(object.get("ok"), Some(&json!(true)));
    assert_eq!(object.get("command"), Some(&json!("query.schema")));
    assert!(object.contains_key("generated_at_utc"));
    assert!(object.contains_key("data"));
    assert!(object.contains_key("meta"));
    assert!(object.contains_key("warnings"));
    assert!(!object.contains_key("error"));
}

#[test]
fn error_envelope_sets_status_and_error_payload() {
    let envelope = QueryEnvelope::error("query.sql", "sql_guardrail_violation", "query rejected");
    assert!(!envelope.ok);
    assert_eq!(envelope.command, "query.sql");
    assert!(envelope.data.is_none());
    assert!(envelope.warnings.is_empty());

    let error = envelope.error.expect("error payload should be present");
    assert_eq!(error.code, "sql_guardrail_violation");
    assert_eq!(error.message, "query rejected");
    assert!(error.details.is_none());
}

#[test]
fn error_envelope_supports_structured_details() {
    let envelope = QueryEnvelope::error("query.sql", "sql_guardrail_violation", "query rejected")
        .with_error_details(json!({"violation":{"reason":"multi_statement"}}));

    let encoded = serde_json::to_value(&envelope).expect("envelope should serialize");
    assert_eq!(
        encoded
            .pointer("/error/details/violation/reason")
            .and_then(|value| value.as_str()),
        Some("multi_statement")
    );
}

#[test]
fn command_failure_display_is_json_envelope() {
    let envelope = QueryEnvelope::error("query.sql", "runtime_failure", "command failed");
    let failure = QueryEnvelopeCommandFailure::new(envelope);
    let rendered = failure.to_string();
    let parsed: serde_json::Value =
        serde_json::from_str(&rendered).expect("display output should be JSON envelope");
    assert_eq!(
        parsed.get("ok").and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        parsed
            .pointer("/error/code")
            .and_then(|value| value.as_str()),
        Some("runtime_failure")
    );
}
