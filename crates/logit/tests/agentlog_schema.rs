use logit::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, SCHEMA_VERSION, SchemaVersion,
    TimestampQuality,
};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn schema_marks_core_fields_as_required() {
    let schema = logit::models::json_schema();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .expect("schema must include required list");

    for field in [
        "schema_version",
        "event_id",
        "run_id",
        "sequence_global",
        "source_kind",
        "source_path",
        "source_record_locator",
        "adapter_name",
        "record_format",
        "event_type",
        "role",
        "timestamp_utc",
        "timestamp_unix_ms",
        "timestamp_quality",
        "raw_hash",
        "canonical_hash",
    ] {
        assert!(required.iter().any(|value| value.as_str() == Some(field)));
    }
}

#[test]
fn serialization_omits_optional_fields_when_empty() {
    let record = AgentLogEvent {
        schema_version: SchemaVersion::AgentLogV1,
        event_id: "evt-1".to_string(),
        run_id: "run-1".to_string(),
        sequence_global: 0,
        sequence_source: None,
        source_kind: AgentSource::Codex,
        source_path: "/tmp/source.jsonl".to_string(),
        source_record_locator: "line:1".to_string(),
        source_record_hash: None,
        adapter_name: AgentSource::Codex,
        adapter_version: None,
        record_format: RecordFormat::Message,
        event_type: EventType::Prompt,
        role: ActorRole::User,
        timestamp_utc: "2026-02-25T00:00:00Z".to_string(),
        timestamp_unix_ms: 1_740_441_600_000,
        timestamp_quality: TimestampQuality::Exact,
        session_id: None,
        conversation_id: None,
        turn_id: None,
        parent_event_id: None,
        actor_id: None,
        actor_name: None,
        provider: None,
        model: None,
        content_text: None,
        content_excerpt: None,
        content_mime: None,
        tool_name: None,
        tool_call_id: None,
        tool_arguments_json: None,
        tool_result_text: None,
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        cost_usd: None,
        tags: Vec::new(),
        flags: Vec::new(),
        pii_redacted: None,
        warnings: Vec::new(),
        errors: Vec::new(),
        raw_hash: "raw-hash".to_string(),
        canonical_hash: "canonical-hash".to_string(),
        metadata: Default::default(),
    };

    let value = serde_json::to_value(record).expect("record serialization should succeed");
    let object = value
        .as_object()
        .expect("serialized event should be a json object");

    assert_eq!(
        object.get("schema_version").and_then(Value::as_str),
        Some(SCHEMA_VERSION)
    );
    assert!(!object.contains_key("sequence_source"));
    assert!(!object.contains_key("content_text"));
    assert!(!object.contains_key("tags"));
    assert!(!object.contains_key("metadata"));
}

#[test]
fn normalize_writes_schema_artifact_json() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("logit-agentlog-schema-{nanos}.json"));

    logit::normalize::write_schema_artifact(&path).expect("schema artifact write should succeed");

    let content = std::fs::read_to_string(&path).expect("schema artifact should be readable");
    let parsed: Value =
        serde_json::from_str(&content).expect("schema artifact should be valid json");
    assert!(parsed.get("properties").is_some());

    let _ = std::fs::remove_file(path);
}
