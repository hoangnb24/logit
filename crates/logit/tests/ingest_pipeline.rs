use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use logit::ingest::{
    INGEST_REPORT_SCHEMA_VERSION, IngestRefreshPlan, IngestRunStatus, ingest_report_artifact_path,
    run_refresh, write_ingest_report_artifact,
};
use logit::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, SchemaVersion, TimestampQuality,
};
use logit::sqlite::{
    EVENTS_TABLE, INGEST_RUNS_TABLE, INGEST_WATERMARKS_TABLE, open_sqlite_connection,
};

fn sample_event(
    event_id: &str,
    sequence_global: u64,
    source_kind: AgentSource,
    source_path: &str,
    timestamp_unix_ms: u64,
) -> AgentLogEvent {
    AgentLogEvent {
        schema_version: SchemaVersion::AgentLogV1,
        event_id: event_id.to_string(),
        run_id: "run-1".to_string(),
        sequence_global,
        sequence_source: Some(sequence_global),
        source_kind,
        source_path: source_path.to_string(),
        source_record_locator: format!("line:{sequence_global}"),
        source_record_hash: Some(format!("source-{event_id}")),
        adapter_name: source_kind,
        adapter_version: Some("v1".to_string()),
        record_format: RecordFormat::Message,
        event_type: EventType::Prompt,
        role: ActorRole::User,
        timestamp_utc: "2026-02-25T00:00:00Z".to_string(),
        timestamp_unix_ms,
        timestamp_quality: TimestampQuality::Exact,
        session_id: Some("session-1".to_string()),
        conversation_id: Some("conversation-1".to_string()),
        turn_id: Some(format!("turn-{sequence_global}")),
        parent_event_id: None,
        actor_id: Some("actor-1".to_string()),
        actor_name: Some("Agent".to_string()),
        provider: Some("openai".to_string()),
        model: Some("gpt-5".to_string()),
        content_text: Some(format!("hello {sequence_global}")),
        content_excerpt: Some(format!("hello {sequence_global}")),
        content_mime: Some("text/plain".to_string()),
        tool_name: None,
        tool_call_id: None,
        tool_arguments_json: None,
        tool_result_text: None,
        input_tokens: Some(1),
        output_tokens: Some(2),
        total_tokens: Some(3),
        cost_usd: Some(0.1),
        tags: vec!["tag".to_string()],
        flags: vec!["flag".to_string()],
        pii_redacted: Some(false),
        warnings: vec![],
        errors: vec![],
        raw_hash: format!("raw-{event_id}"),
        canonical_hash: format!("canonical-{event_id}"),
        metadata: BTreeMap::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn sample_tool_event(
    event_id: &str,
    sequence_global: u64,
    source_kind: AgentSource,
    source_path: &str,
    timestamp_unix_ms: u64,
    record_format: RecordFormat,
    event_type: EventType,
    role: ActorRole,
    tool_name: &str,
    tool_call_id: &str,
) -> AgentLogEvent {
    let mut event = sample_event(
        event_id,
        sequence_global,
        source_kind,
        source_path,
        timestamp_unix_ms,
    );
    event.record_format = record_format;
    event.event_type = event_type;
    event.role = role;
    event.tool_name = Some(tool_name.to_string());
    event.tool_call_id = Some(tool_call_id.to_string());
    event
}

fn temp_paths(label: &str) -> (PathBuf, PathBuf, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let base = std::env::temp_dir().join(format!("logit-{label}-{nanos}"));
    let events_path = base.join("events.jsonl");
    let sqlite_path = base.join("mart.sqlite");
    (base, events_path, sqlite_path)
}

fn write_events_jsonl(path: &PathBuf, events: &[AgentLogEvent]) {
    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).expect("event should serialize");
        body.push_str(&line);
        body.push('\n');
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("fixture dir should be creatable");
    }
    fs::write(path, body).expect("events jsonl should be writable");
}

#[test]
fn refresh_pipeline_writes_events_run_stats_and_watermarks() {
    let (source_root, events_path, sqlite_path) = temp_paths("ingest-pipeline");
    let events = vec![
        sample_event(
            "evt-1",
            1,
            AgentSource::Codex,
            "/tmp/codex/events.jsonl",
            1_771_977_600_001,
        ),
        sample_event(
            "evt-2",
            2,
            AgentSource::Codex,
            "/tmp/codex/events.jsonl",
            1_771_977_600_002,
        ),
        sample_event(
            "evt-3",
            3,
            AgentSource::Claude,
            "/tmp/claude/session.jsonl",
            1_771_977_600_003,
        ),
    ];
    write_events_jsonl(&events_path, &events);

    let plan = IngestRefreshPlan {
        events_jsonl_path: events_path,
        sqlite_path: sqlite_path.clone(),
        source_root,
        fail_fast: false,
    };
    let report = run_refresh(&plan).expect("ingest refresh should succeed");

    assert_eq!(report.status, IngestRunStatus::Success);
    assert_eq!(report.events_read, 3);
    assert_eq!(report.events_written, 3);
    assert_eq!(report.events_skipped, 0);
    assert_eq!(report.warnings_count, 0);
    assert_eq!(report.errors_count, 0);
    assert_eq!(report.watermarks_upserted, 2);
    assert_eq!(report.watermark_staleness_state, "fresh");

    let connection = open_sqlite_connection(&sqlite_path).expect("db should reopen");
    let events_count: i64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {EVENTS_TABLE}"), [], |row| {
            row.get(0)
        })
        .expect("events count query should succeed");
    assert_eq!(events_count, 3);

    let runs_count: i64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {INGEST_RUNS_TABLE}"),
            [],
            |row| row.get(0),
        )
        .expect("ingest run count query should succeed");
    assert_eq!(runs_count, 1);

    let status: String = connection
        .query_row(
            &format!("SELECT status FROM {INGEST_RUNS_TABLE} WHERE ingest_run_id = ?1 LIMIT 1"),
            [&report.ingest_run_id],
            |row| row.get(0),
        )
        .expect("ingest run status query should succeed");
    assert_eq!(status, "success");

    let watermarks_count: i64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {INGEST_WATERMARKS_TABLE}"),
            [],
            |row| row.get(0),
        )
        .expect("watermark count query should succeed");
    assert_eq!(watermarks_count, 2);
}

#[test]
fn refresh_pipeline_tolerates_invalid_rows_when_fail_fast_is_false() {
    let (source_root, events_path, sqlite_path) = temp_paths("ingest-pipeline-warnings");
    let event = sample_event(
        "evt-1",
        1,
        AgentSource::Codex,
        "/tmp/codex/events.jsonl",
        1_771_977_600_001,
    );
    if let Some(parent) = events_path.parent() {
        fs::create_dir_all(parent).expect("fixture dir should be creatable");
    }
    let valid = serde_json::to_string(&event).expect("event should serialize");
    let body = format!("{valid}\n{{invalid-json-row\n");
    fs::write(&events_path, body).expect("events jsonl should be writable");

    let report = run_refresh(&IngestRefreshPlan {
        events_jsonl_path: events_path,
        sqlite_path: sqlite_path.clone(),
        source_root,
        fail_fast: false,
    })
    .expect("ingest refresh should succeed with warning mode");

    assert_eq!(report.status, IngestRunStatus::Success);
    assert_eq!(report.events_read, 1);
    assert_eq!(report.events_written, 1);
    assert_eq!(report.events_skipped, 1);
    assert_eq!(report.warnings_count, 1);
    assert_eq!(report.errors_count, 0);

    let connection = open_sqlite_connection(&sqlite_path).expect("db should reopen");
    let events_count: i64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {EVENTS_TABLE}"), [], |row| {
            row.get(0)
        })
        .expect("events count query should succeed");
    assert_eq!(events_count, 1);
}

#[test]
fn refresh_pipeline_is_idempotent_for_repeat_artifact_replay() {
    let (source_root, events_path, sqlite_path) = temp_paths("ingest-idempotent-replay");
    let event = sample_event(
        "evt-repeat-1",
        1,
        AgentSource::Codex,
        "/tmp/codex/events.jsonl",
        1_771_977_600_001,
    );
    write_events_jsonl(&events_path, &[event]);

    let plan = IngestRefreshPlan {
        events_jsonl_path: events_path,
        sqlite_path: sqlite_path.clone(),
        source_root,
        fail_fast: false,
    };

    let first_report = run_refresh(&plan).expect("first ingest refresh should succeed");
    let second_report = run_refresh(&plan).expect("replayed ingest refresh should succeed");

    assert_eq!(first_report.status, IngestRunStatus::Success);
    assert_eq!(second_report.status, IngestRunStatus::Success);
    assert_eq!(first_report.events_written, 1);
    assert_eq!(second_report.events_written, 1);

    let connection = open_sqlite_connection(&sqlite_path).expect("db should reopen");
    let events_count: i64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {EVENTS_TABLE}"), [], |row| {
            row.get(0)
        })
        .expect("events count query should succeed");
    assert_eq!(
        events_count, 1,
        "repeat refresh replay should remain idempotent by event identity"
    );

    let runs_count: i64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {INGEST_RUNS_TABLE}"),
            [],
            |row| row.get(0),
        )
        .expect("ingest run count query should succeed");
    assert_eq!(runs_count, 2);
}

#[test]
fn writes_machine_readable_ingest_report_artifact() {
    let (source_root, events_path, sqlite_path) = temp_paths("ingest-report-artifact");
    let event = sample_event(
        "evt-1",
        1,
        AgentSource::Codex,
        "/tmp/codex/events.jsonl",
        1_771_977_600_001,
    );
    write_events_jsonl(&events_path, &[event]);

    let report = run_refresh(&IngestRefreshPlan {
        events_jsonl_path: events_path,
        sqlite_path,
        source_root: source_root.clone(),
        fail_fast: false,
    })
    .expect("ingest refresh should succeed");

    let artifact_path = ingest_report_artifact_path(&source_root);
    write_ingest_report_artifact(&artifact_path, &report).expect("report artifact should write");

    let content =
        fs::read_to_string(&artifact_path).expect("ingest report artifact should be readable");
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("ingest report artifact should parse");

    assert_eq!(
        parsed.get("schema_version"),
        Some(&serde_json::json!(INGEST_REPORT_SCHEMA_VERSION))
    );
    assert_eq!(parsed.get("status"), Some(&serde_json::json!("success")));
    assert_eq!(
        parsed.pointer("/counts/inserted"),
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        parsed.pointer("/counts/skipped"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        parsed.pointer("/watermarks/staleness_state"),
        Some(&serde_json::json!("fresh"))
    );
}

#[test]
fn refresh_pipeline_marks_missing_sources_stale_in_followup_refresh() {
    let (source_root, events_path, sqlite_path) = temp_paths("ingest-pipeline-staleness");
    let first_batch = vec![
        sample_event(
            "evt-1",
            1,
            AgentSource::Codex,
            "/tmp/codex/events.jsonl",
            1_771_977_600_001,
        ),
        sample_event(
            "evt-2",
            2,
            AgentSource::Claude,
            "/tmp/claude/session.jsonl",
            1_771_977_600_002,
        ),
    ];
    write_events_jsonl(&events_path, &first_batch);

    let first_report = run_refresh(&IngestRefreshPlan {
        events_jsonl_path: events_path.clone(),
        sqlite_path: sqlite_path.clone(),
        source_root: source_root.clone(),
        fail_fast: false,
    })
    .expect("first ingest refresh should succeed");
    assert_eq!(first_report.watermarks_upserted, 2);
    assert_eq!(first_report.watermark_staleness_state, "fresh");

    let second_batch = vec![sample_event(
        "evt-3",
        3,
        AgentSource::Codex,
        "/tmp/codex/events.jsonl",
        1_771_977_600_010,
    )];
    write_events_jsonl(&events_path, &second_batch);

    let second_report = run_refresh(&IngestRefreshPlan {
        events_jsonl_path: events_path,
        sqlite_path: sqlite_path.clone(),
        source_root,
        fail_fast: false,
    })
    .expect("follow-up ingest refresh should succeed");
    assert_eq!(second_report.watermarks_upserted, 1);
    assert_eq!(second_report.watermark_staleness_state, "stale");

    let connection = open_sqlite_connection(&sqlite_path).expect("db should reopen");
    let mut statement = connection
        .prepare(&format!(
            "SELECT source_key, staleness_state, metadata_json
             FROM {INGEST_WATERMARKS_TABLE}
             ORDER BY source_key"
        ))
        .expect("watermark query should prepare");
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("watermark query should execute");
    let watermark_rows: Vec<(String, String, String)> = rows
        .collect::<Result<_, _>>()
        .expect("watermark rows should collect");
    assert_eq!(watermark_rows.len(), 2);

    let by_source: BTreeMap<String, (String, serde_json::Value)> = watermark_rows
        .into_iter()
        .map(|(source_key, staleness_state, metadata_json)| {
            let parsed =
                serde_json::from_str(&metadata_json).expect("watermark metadata_json should parse");
            (source_key, (staleness_state, parsed))
        })
        .collect();

    let codex = by_source
        .get("codex|/tmp/codex/events.jsonl")
        .expect("codex source watermark should exist");
    assert_eq!(codex.0, "fresh");
    assert_eq!(
        codex
            .1
            .pointer("/incremental_decision")
            .and_then(serde_json::Value::as_str),
        Some("process")
    );
    assert_eq!(
        codex
            .1
            .pointer("/decision_reason")
            .and_then(serde_json::Value::as_str),
        Some("advanced_timestamp")
    );
    assert_eq!(
        codex
            .1
            .pointer("/pre_refresh_staleness_state")
            .and_then(serde_json::Value::as_str),
        Some("stale")
    );

    let claude = by_source
        .get("claude|/tmp/claude/session.jsonl")
        .expect("claude source watermark should exist");
    assert_eq!(claude.0, "stale");
    assert_eq!(
        claude
            .1
            .pointer("/observed_in_refresh")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        claude
            .1
            .pointer("/decision_reason")
            .and_then(serde_json::Value::as_str),
        Some("missing_in_latest_refresh")
    );
}

#[test]
fn refresh_pipeline_preserves_duration_rollups_in_semantic_views() {
    let (source_root, events_path, sqlite_path) = temp_paths("ingest-duration-rollups");
    let events = vec![
        sample_event(
            "evt-msg-1",
            1,
            AgentSource::Codex,
            "/tmp/codex/events.jsonl",
            1_771_977_600_000,
        ),
        sample_tool_event(
            "evt-tc-1",
            2,
            AgentSource::Codex,
            "/tmp/codex/events.jsonl",
            1_771_977_600_100,
            RecordFormat::ToolCall,
            EventType::ToolInvocation,
            ActorRole::Assistant,
            "shell",
            "tc-1",
        ),
        sample_tool_event(
            "evt-tr-1",
            3,
            AgentSource::Codex,
            "/tmp/codex/events.jsonl",
            1_771_977_600_250,
            RecordFormat::ToolResult,
            EventType::ToolOutput,
            ActorRole::Tool,
            "shell",
            "tc-1",
        ),
        sample_event(
            "evt-msg-2",
            4,
            AgentSource::Codex,
            "/tmp/codex/events.jsonl",
            1_771_977_600_400,
        ),
    ];
    write_events_jsonl(&events_path, &events);

    let plan = IngestRefreshPlan {
        events_jsonl_path: events_path,
        sqlite_path: sqlite_path.clone(),
        source_root,
        fail_fast: false,
    };

    let first_report = run_refresh(&plan).expect("first ingest refresh should succeed");
    let second_report = run_refresh(&plan).expect("repeat ingest refresh should succeed");
    assert_eq!(first_report.status, IngestRunStatus::Success);
    assert_eq!(second_report.status, IngestRunStatus::Success);

    let connection = open_sqlite_connection(&sqlite_path).expect("db should reopen");

    let tool_rollup = connection
        .query_row(
            "SELECT duration_ms, duration_source, duration_quality, pairing_status
             FROM v_tool_calls
             WHERE tool_call_id = ?1",
            ["tc-1"],
            |row| {
                Ok((
                    row.get::<usize, Option<i64>>(0)?,
                    row.get::<usize, Option<String>>(1)?,
                    row.get::<usize, Option<String>>(2)?,
                    row.get::<usize, String>(3)?,
                ))
            },
        )
        .expect("tool rollup should query");
    assert_eq!(tool_rollup.0, Some(150));
    assert_eq!(tool_rollup.1.as_deref(), Some("paired"));
    assert_eq!(tool_rollup.2.as_deref(), Some("medium"));
    assert_eq!(tool_rollup.3, "paired");

    let session_rollup = connection
        .query_row(
            "SELECT event_count, first_event_timestamp_unix_ms, last_event_timestamp_unix_ms, duration_ms,
                    tool_call_count, tool_result_count
             FROM v_sessions
             WHERE run_id = ?1 AND session_id = ?2",
            ["run-1", "session-1"],
            |row| {
                Ok((
                    row.get::<usize, i64>(0)?,
                    row.get::<usize, i64>(1)?,
                    row.get::<usize, i64>(2)?,
                    row.get::<usize, i64>(3)?,
                    row.get::<usize, i64>(4)?,
                    row.get::<usize, i64>(5)?,
                ))
            },
        )
        .expect("session rollup should query");
    assert_eq!(session_rollup.0, 4);
    assert_eq!(session_rollup.1, 1_771_977_600_000);
    assert_eq!(session_rollup.2, 1_771_977_600_400);
    assert_eq!(session_rollup.3, 400);
    assert_eq!(session_rollup.4, 1);
    assert_eq!(session_rollup.5, 1);
}
