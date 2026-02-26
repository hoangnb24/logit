use logit::adapters::claude::{
    parse_history_jsonl, parse_mcp_cache_debug_log, parse_project_session_file,
    parse_project_session_jsonl,
};
use logit::models::{ActorRole, EventType, RecordFormat, TimestampQuality};
use serde_json::json;

#[test]
fn parses_claude_project_session_fixture_to_canonical_events() {
    let fixture = fixture_path("project_session.jsonl");
    let input = std::fs::read_to_string(&fixture).expect("fixture readable");
    let result =
        parse_project_session_jsonl(&input, "run-test", fixture.to_string_lossy().as_ref());

    assert!(result.warnings.is_empty());
    assert_eq!(result.events.len(), 3);

    assert_eq!(result.events[0].event_type, EventType::Prompt);
    assert_eq!(result.events[0].role, ActorRole::User);
    assert_eq!(result.events[0].record_format, RecordFormat::Message);
    assert_eq!(
        result.events[0].conversation_id.as_deref(),
        Some("claude-p-001")
    );

    assert_eq!(result.events[1].event_type, EventType::Response);
    assert_eq!(result.events[1].role, ActorRole::Assistant);

    assert_eq!(result.events[2].event_type, EventType::StatusUpdate);
    assert_eq!(result.events[2].role, ActorRole::Runtime);
    assert_eq!(result.events[2].record_format, RecordFormat::System);
}

#[test]
fn handles_malformed_and_unknown_kinds_without_crashing() {
    let fixture = fixture_path("project_session_malformed.jsonl");
    let input = std::fs::read_to_string(&fixture).expect("fixture readable");
    let result =
        parse_project_session_jsonl(&input, "run-test", fixture.to_string_lossy().as_ref());

    assert_eq!(result.events.len(), 3);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("missing `created_at`"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("unknown `kind` value `unknown_kind`"))
    );
    assert_eq!(
        result.events[0].timestamp_quality,
        TimestampQuality::Fallback
    );
    assert_eq!(
        result.events[1].timestamp_quality,
        TimestampQuality::Fallback
    );
    assert_eq!(result.events[2].record_format, RecordFormat::Diagnostic);
}

#[test]
fn parses_claude_project_session_file_from_disk() {
    let path = fixture_path("project_session.jsonl");
    let result = parse_project_session_file(&path, "run-test").expect("fixture should parse");
    assert_eq!(result.events.len(), 3);
}

#[test]
fn maps_system_kind_to_system_notice() {
    let input = r#"{"project_id":"claude-p-x","session_id":"claude-s-x","kind":"system","created_at":"2026-02-02T09:00:10Z","text":"system note"}"#;
    let result = parse_project_session_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].event_type, EventType::SystemNotice);
    assert_eq!(result.events[0].role, ActorRole::System);
    assert_eq!(result.events[0].record_format, RecordFormat::System);
}

#[test]
fn parses_modern_type_rows_and_tool_blocks() {
    let input = concat!(
        "{\"type\":\"assistant\",\"sessionId\":\"claude-s-modern\",\"uuid\":\"evt-tool-use\",\"timestamp\":\"2026-02-02T09:00:10Z\",\"message\":{\"role\":\"assistant\",\"model\":\"claude-opus-4-5-thinking\",\"content\":[{\"type\":\"tool_use\",\"id\":\"Read-1\",\"name\":\"Read\",\"input\":{\"file_path\":\"/tmp/a.txt\"}}]}}\n",
        "{\"type\":\"user\",\"sessionId\":\"claude-s-modern\",\"uuid\":\"evt-tool-result\",\"parentUuid\":\"evt-tool-use\",\"timestamp\":\"2026-02-02T09:00:11Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"Read-1\",\"content\":\"ok\",\"is_error\":false}]}}\n",
        "{\"type\":\"assistant\",\"sessionId\":\"claude-s-modern\",\"uuid\":\"evt-text\",\"timestamp\":\"2026-02-02T09:00:12Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Done.\"}]}}\n",
    );
    let result = parse_project_session_jsonl(input, "run-test", "inline-modern");

    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );
    assert_eq!(result.events.len(), 3);

    assert_eq!(result.events[0].record_format, RecordFormat::ToolCall);
    assert_eq!(result.events[0].event_type, EventType::ToolInvocation);
    assert_eq!(result.events[0].role, ActorRole::Tool);
    assert_eq!(
        result.events[0].session_id.as_deref(),
        Some("claude-s-modern")
    );
    assert_eq!(result.events[0].tool_name.as_deref(), Some("Read"));
    assert_eq!(result.events[0].tool_call_id.as_deref(), Some("Read-1"));
    assert!(
        result.events[0]
            .tool_arguments_json
            .as_deref()
            .is_some_and(|json| json.contains("file_path"))
    );

    assert_eq!(result.events[1].record_format, RecordFormat::ToolResult);
    assert_eq!(result.events[1].event_type, EventType::ToolOutput);
    assert_eq!(result.events[1].role, ActorRole::Tool);
    assert_eq!(result.events[1].tool_call_id.as_deref(), Some("Read-1"));
    assert_eq!(result.events[1].tool_result_text.as_deref(), Some("ok"));
    assert_eq!(
        result.events[1].parent_event_id.as_deref(),
        Some("evt-tool-use")
    );

    assert_eq!(result.events[2].record_format, RecordFormat::Message);
    assert_eq!(result.events[2].event_type, EventType::Response);
    assert_eq!(result.events[2].role, ActorRole::Assistant);
    assert_eq!(result.events[2].content_text.as_deref(), Some("Done."));
}

#[test]
fn parses_file_history_snapshot_rows_as_artifact_references() {
    let input = r#"{"type":"file-history-snapshot","messageId":"msg-1","sessionId":"claude-s-x","timestamp":"2026-02-02T09:00:10Z","snapshot":{"timestamp":"2026-02-02T09:00:10Z"}}"#;
    let result = parse_project_session_jsonl(input, "run-test", "inline-snapshot");

    assert!(result.warnings.is_empty());
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].event_id, "msg-1");
    assert_eq!(result.events[0].event_type, EventType::ArtifactReference);
    assert_eq!(result.events[0].record_format, RecordFormat::System);
    assert_eq!(result.events[0].role, ActorRole::Runtime);
    assert_eq!(result.events[0].session_id.as_deref(), Some("claude-s-x"));
    assert_eq!(result.events[0].timestamp_quality, TimestampQuality::Exact);
}

#[test]
fn maps_core_project_session_kinds_to_canonical_families() {
    let input = concat!(
        "{\"kind\":\"user\",\"created_at\":\"2026-02-02T09:00:10Z\",\"text\":\"u\"}\n",
        "{\"kind\":\"assistant\",\"created_at\":\"2026-02-02T09:00:11Z\",\"text\":\"a\"}\n",
        "{\"kind\":\"system\",\"created_at\":\"2026-02-02T09:00:12Z\",\"text\":\"s\"}\n",
        "{\"kind\":\"progress\",\"created_at\":\"2026-02-02T09:00:13Z\",\"text\":\"p\"}\n",
    );
    let result = parse_project_session_jsonl(input, "run-test", "inline");

    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );
    assert_eq!(result.events.len(), 4);

    assert_eq!(result.events[0].record_format, RecordFormat::Message);
    assert_eq!(result.events[0].event_type, EventType::Prompt);
    assert_eq!(result.events[0].role, ActorRole::User);

    assert_eq!(result.events[1].record_format, RecordFormat::Message);
    assert_eq!(result.events[1].event_type, EventType::Response);
    assert_eq!(result.events[1].role, ActorRole::Assistant);

    assert_eq!(result.events[2].record_format, RecordFormat::System);
    assert_eq!(result.events[2].event_type, EventType::SystemNotice);
    assert_eq!(result.events[2].role, ActorRole::System);

    assert_eq!(result.events[3].record_format, RecordFormat::System);
    assert_eq!(result.events[3].event_type, EventType::StatusUpdate);
    assert_eq!(result.events[3].role, ActorRole::Runtime);
}

#[test]
fn maps_subagent_and_history_core_roles_to_canonical_families() {
    let subagent_input = concat!(
        "{\"parent_session_id\":\"claude-s-1\",\"subagent_session_id\":\"claude-sub-1\",\"created_at\":\"2026-02-02T09:00:10Z\",\"role\":\"user\",\"text\":\"u\"}\n",
        "{\"parent_session_id\":\"claude-s-1\",\"subagent_session_id\":\"claude-sub-1\",\"created_at\":\"2026-02-02T09:00:11Z\",\"role\":\"assistant\",\"text\":\"a\"}\n",
        "{\"parent_session_id\":\"claude-s-1\",\"subagent_session_id\":\"claude-sub-1\",\"created_at\":\"2026-02-02T09:00:12Z\",\"role\":\"system\",\"text\":\"s\"}\n",
        "{\"parent_session_id\":\"claude-s-1\",\"subagent_session_id\":\"claude-sub-1\",\"created_at\":\"2026-02-02T09:00:13Z\",\"role\":\"progress\",\"text\":\"p\"}\n",
    );
    let subagent_result = parse_project_session_jsonl(subagent_input, "run-test", "inline");
    assert!(
        subagent_result.warnings.is_empty(),
        "unexpected subagent warnings: {:?}",
        subagent_result.warnings
    );
    assert_eq!(subagent_result.events.len(), 4);
    assert_eq!(subagent_result.events[0].event_type, EventType::Prompt);
    assert_eq!(subagent_result.events[0].role, ActorRole::User);
    assert_eq!(subagent_result.events[1].event_type, EventType::Response);
    assert_eq!(subagent_result.events[1].role, ActorRole::Assistant);
    assert_eq!(
        subagent_result.events[2].record_format,
        RecordFormat::System
    );
    assert_eq!(
        subagent_result.events[2].event_type,
        EventType::SystemNotice
    );
    assert_eq!(subagent_result.events[2].role, ActorRole::System);
    assert_eq!(
        subagent_result.events[3].record_format,
        RecordFormat::System
    );
    assert_eq!(
        subagent_result.events[3].event_type,
        EventType::StatusUpdate
    );
    assert_eq!(subagent_result.events[3].role, ActorRole::Runtime);

    let history_input = concat!(
        "{\"timestamp\":\"2026-02-02T09:01:10Z\",\"role\":\"user\",\"text\":\"u\"}\n",
        "{\"timestamp\":\"2026-02-02T09:01:11Z\",\"role\":\"assistant\",\"text\":\"a\"}\n",
        "{\"timestamp\":\"2026-02-02T09:01:12Z\",\"role\":\"system\",\"text\":\"s\"}\n",
        "{\"timestamp\":\"2026-02-02T09:01:13Z\",\"role\":\"progress\",\"text\":\"p\"}\n",
    );
    let history_result = parse_history_jsonl(history_input, "run-test", "inline-history");
    assert!(
        history_result.warnings.is_empty(),
        "unexpected history warnings: {:?}",
        history_result.warnings
    );
    assert_eq!(history_result.events.len(), 4);
    assert_eq!(
        history_result.events[0].record_format,
        RecordFormat::Message
    );
    assert_eq!(history_result.events[0].event_type, EventType::Prompt);
    assert_eq!(history_result.events[0].role, ActorRole::User);
    assert_eq!(
        history_result.events[1].record_format,
        RecordFormat::Message
    );
    assert_eq!(history_result.events[1].event_type, EventType::Response);
    assert_eq!(history_result.events[1].role, ActorRole::Assistant);
    assert_eq!(history_result.events[2].record_format, RecordFormat::System);
    assert_eq!(history_result.events[2].event_type, EventType::SystemNotice);
    assert_eq!(history_result.events[2].role, ActorRole::System);
    assert_eq!(history_result.events[3].record_format, RecordFormat::System);
    assert_eq!(history_result.events[3].event_type, EventType::StatusUpdate);
    assert_eq!(history_result.events[3].role, ActorRole::Runtime);
}

#[test]
fn prefers_message_content_then_message_text_then_message_serialization() {
    let input = concat!(
        "{\"project_id\":\"claude-p-x\",\"session_id\":\"claude-s-x\",\"kind\":\"assistant\",\"created_at\":\"2026-02-02T09:00:10Z\",\"text\":\"top text\",\"message\":{\"content\":\"from message content\",\"text\":\"from message text\"}}\n",
        "{\"project_id\":\"claude-p-x\",\"session_id\":\"claude-s-x\",\"kind\":\"assistant\",\"created_at\":\"2026-02-02T09:00:11Z\",\"text\":\"top text\",\"message\":{\"text\":\"from message text\"}}\n",
        "{\"project_id\":\"claude-p-x\",\"session_id\":\"claude-s-x\",\"kind\":\"assistant\",\"created_at\":\"2026-02-02T09:00:12Z\",\"message\":{\"summary\":\"from message fallback\"}}\n",
        "{\"parent_session_id\":\"claude-s-x\",\"subagent_session_id\":\"claude-sub-x\",\"role\":\"assistant\",\"created_at\":\"2026-02-02T09:00:13Z\",\"text\":\"subagent top\",\"message\":{\"content\":\"subagent message content\",\"text\":\"subagent message text\"}}\n",
    );
    let result = parse_project_session_jsonl(input, "run-test", "inline");

    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );
    assert_eq!(result.events.len(), 4);
    assert_eq!(
        result.events[0].content_text.as_deref(),
        Some("from message content")
    );
    assert_eq!(
        result.events[1].content_text.as_deref(),
        Some("from message text")
    );
    assert_eq!(
        result.events[2].content_text.as_deref(),
        Some("from message fallback")
    );
    assert_eq!(
        result.events[3].content_text.as_deref(),
        Some("subagent message content")
    );
}

#[test]
fn history_parser_prefers_message_content_fallback_chain_after_prompt_response() {
    let input = r#"{"timestamp":"2026-02-02T09:01:10Z","role":"assistant","text":"history top","message":{"content":"history message content","text":"history message text"}}"#;
    let result = parse_history_jsonl(input, "run-test", "inline-history");

    assert!(result.warnings.is_empty());
    assert_eq!(result.events.len(), 1);
    assert_eq!(
        result.events[0].content_text.as_deref(),
        Some("history message content")
    );
}

#[test]
fn parses_subagent_trace_fixture_with_delegated_tags() {
    let fixture = fixture_path("subagent_trace.jsonl");
    let input = std::fs::read_to_string(&fixture).expect("fixture readable");
    let result =
        parse_project_session_jsonl(&input, "run-test", fixture.to_string_lossy().as_ref());

    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );
    assert_eq!(result.events.len(), 2);

    let first = &result.events[0];
    assert_eq!(first.event_type, EventType::Response);
    assert_eq!(first.role, ActorRole::Assistant);
    assert_eq!(first.record_format, RecordFormat::Message);
    assert_eq!(first.session_id.as_deref(), Some("claude-sub-01"));
    assert_eq!(first.conversation_id.as_deref(), Some("claude-s-001"));
    assert!(first.tags.iter().any(|tag| tag == "subagent_trace"));
    assert!(first.tags.iter().any(|tag| tag == "delegated"));
    assert!(first.flags.iter().any(|flag| flag == "subagent"));
    assert!(first.flags.iter().any(|flag| flag == "delegated"));
    assert_eq!(
        first.metadata.get("claude_record"),
        Some(&json!("subagent_trace"))
    );
    assert_eq!(first.metadata.get("delegated_activity"), Some(&json!(true)));
    assert_eq!(
        first.metadata.get("parent_session_id"),
        Some(&json!("claude-s-001"))
    );
    assert_eq!(
        first.metadata.get("subagent_session_id"),
        Some(&json!("claude-sub-01"))
    );
}

#[test]
fn handles_malformed_subagent_trace_records_without_crashing() {
    let input = concat!(
        "{\"parent_session_id\":\"claude-s-1\",\"subagent_session_id\":\"claude-sub-1\",\"created_at\":\"bad-ts\",\"role\":\"observer\",\"text\":\"trace\"}\n",
        "not-json\n",
        "{\"parent_session_id\":\"claude-s-1\",\"created_at\":\"2026-02-02T09:00:14Z\",\"role\":\"assistant\",\"text\":\"missing subagent id\"}\n",
    );
    let result = parse_project_session_jsonl(input, "run-test", "inline");

    assert_eq!(result.events.len(), 2);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("unknown subagent `role` value `observer`"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid `created_at` value"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid JSON payload"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("missing `subagent_session_id`"))
    );
    assert_eq!(result.events[0].record_format, RecordFormat::Diagnostic);
    assert_eq!(
        result.events[0].timestamp_quality,
        TimestampQuality::Fallback
    );
    assert_eq!(result.events[1].session_id, None);
    assert_eq!(
        result.events[1].conversation_id.as_deref(),
        Some("claude-s-1")
    );
}

#[test]
fn parses_history_jsonl_as_auxiliary_events() {
    let input = concat!(
        "{\"entry_id\":\"hist-1\",\"project_id\":\"claude-p-1\",\"session_id\":\"claude-s-1\",\"created_at\":\"2026-02-02T09:01:00Z\",\"prompt\":\"Summarize risk.\"}\n",
        "{\"id\":\"hist-2\",\"project_id\":\"claude-p-1\",\"session_id\":\"claude-s-1\",\"timestamp\":\"2026-02-02T09:01:05Z\",\"role\":\"assistant\",\"response\":\"Risk is low.\"}\n",
        "not-json\n",
    );
    let result = parse_history_jsonl(input, "run-test", "inline-history");

    assert_eq!(result.events.len(), 2);
    assert_eq!(result.events[0].event_type, EventType::Prompt);
    assert_eq!(result.events[0].role, ActorRole::User);
    assert_eq!(result.events[1].event_type, EventType::Response);
    assert_eq!(result.events[1].role, ActorRole::Assistant);
    assert!(
        result.events[0]
            .tags
            .iter()
            .any(|tag| tag == "history_auxiliary")
    );
    assert_eq!(
        result.events[0].metadata.get("claude_record"),
        Some(&json!("history_auxiliary"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid JSON payload"))
    );
}

#[test]
fn infers_assistant_role_for_history_rows_with_response_only() {
    let input = r#"{"id":"hist-rsp","timestamp":"2026-02-02T09:01:05Z","response":"Done."}"#;
    let result = parse_history_jsonl(input, "run-test", "inline-history");

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].event_type, EventType::Response);
    assert_eq!(result.events[0].role, ActorRole::Assistant);
    assert!(result.warnings.is_empty());
}

#[test]
fn parses_mcp_cache_debug_log_as_diagnostic_events() {
    let fixture = fixture_path("mcp_cache_debug.log");
    let input = std::fs::read_to_string(&fixture).expect("fixture readable");
    let result = parse_mcp_cache_debug_log(&input, "run-test", fixture.to_string_lossy().as_ref());

    assert!(result.warnings.is_empty());
    assert_eq!(result.events.len(), 2);
    let first = &result.events[0];
    assert_eq!(first.record_format, RecordFormat::Diagnostic);
    assert_eq!(first.event_type, EventType::DebugLog);
    assert_eq!(first.role, ActorRole::Runtime);
    assert!(first.tags.iter().any(|tag| tag == "mcp_cache_debug"));
    assert!(first.flags.iter().any(|flag| flag == "non_conversational"));
    assert_eq!(
        first.metadata.get("claude_record"),
        Some(&json!("mcp_cache_debug"))
    );
    assert_eq!(first.metadata.get("log_level"), Some(&json!("DEBUG")));
    assert_eq!(
        first.metadata.get("log_component"),
        Some(&json!("claude.mcp"))
    );
}

#[test]
fn handles_mcp_cache_lines_without_timestamp_prefix() {
    let input = "DEBUG claude.mcp cache_lookup key=resource://thread/br-29y hit=true\n";
    let result = parse_mcp_cache_debug_log(input, "run-test", "inline-mcp");

    assert_eq!(result.events.len(), 1);
    assert_eq!(
        result.events[0].timestamp_quality,
        TimestampQuality::Fallback
    );
    assert_eq!(
        result.events[0].metadata.get("log_level"),
        Some(&json!("DEBUG"))
    );
    assert_eq!(
        result.events[0].metadata.get("log_component"),
        Some(&json!("claude.mcp"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("missing timestamp prefix"))
    );
}

#[test]
fn dispatches_history_and_mcp_cache_files_in_parse_project_session_file() {
    let temp = unique_temp_dir("logit-claude-dispatch");
    std::fs::create_dir_all(&temp).expect("temp dir should be creatable");

    let history_path = temp.join("history.jsonl");
    std::fs::write(
        &history_path,
        "{\"entry_id\":\"hist-1\",\"created_at\":\"2026-02-02T09:01:00Z\",\"prompt\":\"hello\"}\n",
    )
    .expect("history file should be writable");

    let mcp_path = temp.join("mcp_cache_debug.log");
    std::fs::write(
        &mcp_path,
        "2026-02-02T09:00:11Z DEBUG claude.mcp cache_lookup key=resource://thread/br-29y hit=true\n",
    )
    .expect("mcp log file should be writable");

    let history_result = parse_project_session_file(&history_path, "run-test")
        .expect("history dispatch should parse");
    let mcp_result =
        parse_project_session_file(&mcp_path, "run-test").expect("mcp dispatch should parse");

    assert_eq!(history_result.events.len(), 1);
    assert_eq!(
        history_result.events[0].metadata.get("claude_record"),
        Some(&json!("history_auxiliary"))
    );
    assert_eq!(mcp_result.events.len(), 1);
    assert_eq!(
        mcp_result.events[0].metadata.get("claude_record"),
        Some(&json!("mcp_cache_debug"))
    );
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/claude")
        .join(name)
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}
