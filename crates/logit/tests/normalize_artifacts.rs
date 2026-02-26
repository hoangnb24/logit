use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use logit::cli::commands::normalize::{NormalizeArgs, run as run_normalize};
use logit::config::RuntimePaths;
use logit::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, SchemaVersion, TimestampQuality,
};
use logit::normalize::{
    DedupeStats, build_artifact_layout, build_normalize_stats, write_events_artifact,
};
use serde_json::Value;

fn fixture_event(event_id: &str) -> AgentLogEvent {
    AgentLogEvent {
        schema_version: SchemaVersion::AgentLogV1,
        event_id: event_id.to_string(),
        run_id: "run-1".to_string(),
        sequence_global: 0,
        sequence_source: Some(1),
        source_kind: AgentSource::Codex,
        source_path: "/tmp/source.jsonl".to_string(),
        source_record_locator: "line:1".to_string(),
        source_record_hash: None,
        adapter_name: AgentSource::Codex,
        adapter_version: Some("v1".to_string()),
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
        content_text: Some("hello".to_string()),
        content_excerpt: Some("hello".to_string()),
        content_mime: Some("text/plain".to_string()),
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
fn artifact_layout_uses_contract_filenames() {
    let layout = build_artifact_layout(Path::new("/tmp/logit-out/normalize"));

    assert_eq!(
        layout.events_jsonl,
        Path::new("/tmp/logit-out/normalize/events.jsonl")
    );
    assert_eq!(
        layout.schema_json,
        Path::new("/tmp/logit-out/normalize/agentlog.v1.schema.json")
    );
    assert_eq!(
        layout.stats_json,
        Path::new("/tmp/logit-out/normalize/stats.json")
    );
}

#[test]
fn events_writer_emits_jsonl_rows_in_input_order() {
    let output_dir = unique_temp_dir("logit-normalize-events");
    let events_path = output_dir.join("events.jsonl");

    let first = fixture_event("evt-1");
    let mut second = fixture_event("evt-2");
    second.event_type = EventType::Response;
    second.role = ActorRole::Assistant;

    write_events_artifact(&events_path, &[first, second]).expect("events artifact write succeeds");

    let content =
        std::fs::read_to_string(&events_path).expect("events artifact should be readable");
    assert!(content.ends_with('\n'));

    let rows = content
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("jsonl row should parse"))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].get("event_id").and_then(Value::as_str),
        Some("evt-1")
    );
    assert_eq!(
        rows[1].get("event_id").and_then(Value::as_str),
        Some("evt-2")
    );
}

#[test]
fn normalize_stats_aggregate_counts_and_breakdowns() {
    let mut first = fixture_event("evt-1");
    first.warnings = vec!["warn-a".to_string(), "warn-b".to_string()];

    let mut second = fixture_event("evt-2");
    second.adapter_name = AgentSource::Claude;
    second.source_kind = AgentSource::Claude;
    second.record_format = RecordFormat::Diagnostic;
    second.event_type = EventType::StatusUpdate;
    second.timestamp_quality = TimestampQuality::Fallback;
    second.errors = vec!["err-a".to_string()];

    let stats = build_normalize_stats(
        &[first, second],
        DedupeStats {
            input_records: 3,
            unique_records: 2,
            duplicate_records: 1,
        },
    );

    assert_eq!(stats.schema_version, "agentlog.v1");
    assert_eq!(stats.counts.input_records, 3);
    assert_eq!(stats.counts.records_emitted, 2);
    assert_eq!(stats.counts.duplicates_removed, 1);
    assert_eq!(stats.counts.warnings, 2);
    assert_eq!(stats.counts.errors, 1);
    assert_eq!(stats.adapter_contributions.get("codex"), Some(&1));
    assert_eq!(stats.adapter_contributions.get("claude"), Some(&1));
    assert_eq!(stats.record_format_counts.get("message"), Some(&1));
    assert_eq!(stats.record_format_counts.get("diagnostic"), Some(&1));
    assert_eq!(stats.event_type_counts.get("prompt"), Some(&1));
    assert_eq!(stats.event_type_counts.get("status_update"), Some(&1));
    assert_eq!(stats.timestamp_quality_counts.get("exact"), Some(&1));
    assert_eq!(stats.timestamp_quality_counts.get("fallback"), Some(&1));
    assert_eq!(stats.timestamp_quality_counts.get("derived"), Some(&0));
}

#[test]
fn normalize_command_emits_all_normalize_artifacts() {
    let out_dir = unique_temp_dir("logit-normalize-run");
    let runtime_paths = RuntimePaths {
        home_dir: PathBuf::from("/tmp/logit-home"),
        cwd: PathBuf::from("/tmp/logit-cwd"),
        out_dir: out_dir.clone(),
    };
    let args = NormalizeArgs {
        source_root: Some(PathBuf::from("/tmp/source-root")),
        fail_fast: true,
    };

    run_normalize(&args, &runtime_paths).expect("normalize command should succeed");

    let layout = build_artifact_layout(&out_dir);
    assert!(layout.events_jsonl.exists(), "events artifact should exist");
    assert!(layout.schema_json.exists(), "schema artifact should exist");
    assert!(layout.stats_json.exists(), "stats artifact should exist");

    let events = std::fs::read_to_string(&layout.events_jsonl).expect("events artifact readable");
    assert!(
        events.is_empty(),
        "placeholder normalize run should emit zero rows"
    );

    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&layout.schema_json).expect("schema artifact readable"),
    )
    .expect("schema artifact should be valid json");
    assert!(schema.get("properties").is_some());

    let stats: Value = serde_json::from_str(
        &std::fs::read_to_string(&layout.stats_json).expect("stats artifact readable"),
    )
    .expect("stats artifact should be valid json");
    assert_eq!(
        stats
            .pointer("/counts/records_emitted")
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        stats.pointer("/counts/warnings").and_then(Value::as_u64),
        Some(0)
    );
    assert!(
        stats
            .get("adapter_contributions")
            .and_then(Value::as_object)
            .is_some()
    );
}
