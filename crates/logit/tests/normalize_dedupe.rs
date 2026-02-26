use std::collections::BTreeMap;

use logit::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, SchemaVersion, TimestampQuality,
};
use logit::normalize::dedupe_and_sort_events;
use serde_json::Value;

fn fixture_event(event_id: &str) -> AgentLogEvent {
    AgentLogEvent {
        schema_version: SchemaVersion::AgentLogV1,
        event_id: event_id.to_string(),
        run_id: "run-1".to_string(),
        sequence_global: 99,
        sequence_source: Some(1),
        source_kind: AgentSource::Codex,
        source_path: "/tmp/source.jsonl".to_string(),
        source_record_locator: "line:1".to_string(),
        source_record_hash: Some("source-hash".to_string()),
        adapter_name: AgentSource::Codex,
        adapter_version: Some("v1".to_string()),
        record_format: RecordFormat::Message,
        event_type: EventType::Prompt,
        role: ActorRole::User,
        timestamp_utc: "2026-02-25T00:00:00Z".to_string(),
        timestamp_unix_ms: 1_740_441_600_000,
        timestamp_quality: TimestampQuality::Exact,
        session_id: Some("session-1".to_string()),
        conversation_id: Some("conversation-1".to_string()),
        turn_id: Some("turn-1".to_string()),
        parent_event_id: None,
        actor_id: None,
        actor_name: None,
        provider: None,
        model: None,
        content_text: Some("Hello world".to_string()),
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
        raw_hash: format!("raw-{event_id}"),
        canonical_hash: "canonical-1".to_string(),
        metadata: BTreeMap::new(),
    }
}

#[test]
fn dedupes_by_canonical_hash_and_tracks_metadata() {
    let mut first = fixture_event("evt-a");
    first.timestamp_quality = TimestampQuality::Fallback;
    first.source_record_locator = "line:1".to_string();

    let mut second = fixture_event("evt-b");
    second.timestamp_quality = TimestampQuality::Exact;
    second.source_record_locator = "line:2".to_string();

    let (events, stats) = dedupe_and_sort_events(vec![first, second]);

    assert_eq!(stats.input_records, 2);
    assert_eq!(stats.unique_records, 1);
    assert_eq!(stats.duplicate_records, 1);
    assert_eq!(events.len(), 1);

    let event = &events[0];
    assert_eq!(event.event_id, "evt-b");
    assert_eq!(event.sequence_global, 0);
    assert_eq!(
        event
            .metadata
            .get("dedupe_strategy")
            .and_then(Value::as_str)
            .expect("dedupe strategy should be present"),
        "canonical_hash"
    );
    assert_eq!(
        event
            .metadata
            .get("dedupe_count")
            .and_then(Value::as_u64)
            .expect("dedupe_count should be present"),
        2
    );
    assert_eq!(
        event
            .metadata
            .get("provenance_entries")
            .and_then(Value::as_array)
            .expect("provenance entries should be present")
            .len(),
        2
    );
}

#[test]
fn falls_back_to_source_locator_key_when_canonical_hash_missing() {
    let mut first = fixture_event("evt-x");
    first.canonical_hash.clear();
    first.conversation_id = None;
    first.turn_id = None;
    first.content_text = None;
    first.source_record_locator = "line:9".to_string();

    let mut second = fixture_event("evt-y");
    second.canonical_hash.clear();
    second.conversation_id = None;
    second.turn_id = None;
    second.content_text = None;
    second.source_record_locator = "line:9".to_string();
    second.raw_hash = "raw-same".to_string();
    first.raw_hash = "raw-same".to_string();

    let (events, stats) = dedupe_and_sort_events(vec![first, second]);
    assert_eq!(events.len(), 1);
    assert_eq!(stats.duplicate_records, 1);
}

#[test]
fn sorts_by_timestamp_then_quality_then_tiebreakers() {
    let mut fallback = fixture_event("evt-c");
    fallback.timestamp_unix_ms = 10;
    fallback.timestamp_quality = TimestampQuality::Fallback;
    fallback.canonical_hash = "c-hash".to_string();
    fallback.source_record_locator = "line:3".to_string();

    let mut exact = fixture_event("evt-a");
    exact.timestamp_unix_ms = 10;
    exact.timestamp_quality = TimestampQuality::Exact;
    exact.canonical_hash = "a-hash".to_string();
    exact.source_record_locator = "line:1".to_string();

    let mut later = fixture_event("evt-b");
    later.timestamp_unix_ms = 11;
    later.timestamp_quality = TimestampQuality::Exact;
    later.canonical_hash = "b-hash".to_string();
    later.source_record_locator = "line:2".to_string();

    let (events, _) = dedupe_and_sort_events(vec![fallback, later, exact]);
    let ids = events
        .iter()
        .map(|event| event.event_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["evt-a", "evt-c", "evt-b"]);
    assert_eq!(events[0].sequence_global, 0);
    assert_eq!(events[1].sequence_global, 1);
    assert_eq!(events[2].sequence_global, 2);
}
