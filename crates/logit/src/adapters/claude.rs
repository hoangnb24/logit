use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, TimestampQuality,
};
use crate::utils::content;
use crate::utils::hash::hash64;
use crate::utils::time::{format_unix_ms, normalize_timestamp_exact};

pub const DEFAULT_PATHS: &[&str] = &["~/.claude/projects", "~/.claude/statsig", "~/.claude.json"];

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeSessionParseResult {
    pub events: Vec<AgentLogEvent>,
    pub warnings: Vec<String>,
}

pub fn parse_project_session_file(path: &Path, run_id: &str) -> Result<ClaudeSessionParseResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read claude project session file: {path:?}"))?;
    let source_path = path.to_string_lossy().to_string();
    let file_name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    if file_name.contains("mcp_cache") || matches!(extension.as_str(), "log" | "txt") {
        return Ok(parse_mcp_cache_debug_log(&content, run_id, &source_path));
    }

    if file_name.contains("history") {
        return Ok(parse_history_jsonl(&content, run_id, &source_path));
    }

    Ok(parse_project_session_jsonl(&content, run_id, &source_path))
}

#[must_use]
pub fn parse_history_jsonl(
    input: &str,
    run_id: &str,
    source_path: &str,
) -> ClaudeSessionParseResult {
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed = match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => value,
            Err(error) => {
                warnings.push(format!(
                    "line {line_number}: invalid JSON payload ({error})"
                ));
                continue;
            }
        };

        let Some(object) = parsed.as_object() else {
            warnings.push(format!("line {line_number}: JSON root must be an object"));
            continue;
        };

        let role_value = extract_string(object.get("role"))
            .or_else(|| extract_string(object.get("kind")))
            .unwrap_or_else(|| infer_history_role(object).to_string());
        let (record_format, canonical_event_type, role) =
            classify_history_role(&role_value, line_number, &mut warnings);

        let event_id = extract_string(object.get("event_id"))
            .or_else(|| extract_string(object.get("entry_id")))
            .or_else(|| extract_string(object.get("id")))
            .unwrap_or_else(|| format!("claude-history-line-{line_number:06}"));
        let project_id = extract_string(object.get("project_id"));
        let session_id = extract_string(object.get("session_id"));

        let content_text = object
            .get("prompt")
            .and_then(content::extract_text)
            .or_else(|| object.get("response").and_then(content::extract_text))
            .or_else(|| extract_conversation_content_text(object));
        if content_text.is_none() {
            warnings.push(format!(
                "line {line_number}: missing conversational content in history row"
            ));
        }
        let content_excerpt = content_text
            .as_deref()
            .and_then(|text| content::derive_excerpt(text, content::DEFAULT_EXCERPT_MAX_CHARS));

        let (timestamp_unix_ms, timestamp_utc, timestamp_quality) = map_timestamp(
            object.get("created_at").or_else(|| object.get("timestamp")),
            line_number,
            &mut warnings,
        );

        let raw_hash = format!("{:016x}", hash64(&trimmed));
        let canonical_hash = format!(
            "{:016x}",
            hash64(&(
                project_id.clone(),
                session_id.clone(),
                event_id.clone(),
                role_value.clone(),
                content_text.clone(),
                line_number
            ))
        );

        let mut metadata = BTreeMap::new();
        metadata.insert("source_line".to_string(), serde_json::json!(line_number));
        metadata.insert(
            "claude_record".to_string(),
            serde_json::json!("history_auxiliary"),
        );
        metadata.insert("history_role".to_string(), serde_json::json!(role_value));
        if let Some(project_id) = &project_id {
            metadata.insert("project_id".to_string(), serde_json::json!(project_id));
        }

        events.push(AgentLogEvent {
            schema_version: crate::models::SchemaVersion::AgentLogV1,
            event_id,
            run_id: run_id.to_string(),
            sequence_global: events.len() as u64,
            sequence_source: Some(index as u64),
            source_kind: AgentSource::Claude,
            source_path: source_path.to_string(),
            source_record_locator: format!("line:{line_number}"),
            source_record_hash: None,
            adapter_name: AgentSource::Claude,
            adapter_version: Some("v1".to_string()),
            record_format,
            event_type: canonical_event_type,
            role,
            timestamp_utc,
            timestamp_unix_ms,
            timestamp_quality,
            session_id,
            conversation_id: project_id,
            turn_id: None,
            parent_event_id: None,
            actor_id: None,
            actor_name: None,
            provider: None,
            model: None,
            content_text,
            content_excerpt,
            content_mime: Some("text/plain".to_string()),
            tool_name: None,
            tool_call_id: None,
            tool_arguments_json: None,
            tool_result_text: None,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cost_usd: None,
            tags: vec![
                "claude".to_string(),
                "history_auxiliary".to_string(),
                "auxiliary".to_string(),
            ],
            flags: vec!["auxiliary".to_string()],
            pii_redacted: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            raw_hash,
            canonical_hash,
            metadata,
        });
    }

    ClaudeSessionParseResult { events, warnings }
}

#[must_use]
pub fn parse_mcp_cache_debug_log(
    input: &str,
    run_id: &str,
    source_path: &str,
) -> ClaudeSessionParseResult {
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        let first_token = parts.next();
        let (timestamp_token, level, component) = match first_token {
            Some(token) if looks_like_timestamp_token(token) => (
                Some(token.to_string()),
                parts.next().unwrap_or("UNKNOWN").to_string(),
                parts.next().unwrap_or("claude.mcp").to_string(),
            ),
            Some(token) => {
                warnings.push(format!(
                    "line {line_number}: missing timestamp prefix in MCP cache debug log; using fallback timestamp"
                ));
                (
                    None,
                    token.to_string(),
                    parts.next().unwrap_or("claude.mcp").to_string(),
                )
            }
            None => continue,
        };
        let message = parts.collect::<Vec<_>>().join(" ");

        let (timestamp_unix_ms, timestamp_utc, timestamp_quality) = match timestamp_token {
            Some(token) => {
                let timestamp_value = Value::String(token);
                map_timestamp(Some(&timestamp_value), line_number, &mut warnings)
            }
            None => map_timestamp(None, line_number, &mut warnings),
        };

        let raw_hash = format!("{:016x}", hash64(&trimmed));
        let canonical_hash = format!(
            "{:016x}",
            hash64(&(
                level.as_str(),
                component.as_str(),
                message.as_str(),
                line_number
            ))
        );

        let mut metadata = BTreeMap::new();
        metadata.insert("source_line".to_string(), serde_json::json!(line_number));
        metadata.insert(
            "claude_record".to_string(),
            serde_json::json!("mcp_cache_debug"),
        );
        metadata.insert("log_level".to_string(), serde_json::json!(level));
        metadata.insert("log_component".to_string(), serde_json::json!(component));
        if let Some(action) = message.split_whitespace().next() {
            metadata.insert("mcp_action".to_string(), serde_json::json!(action));
        }
        for token in message.split_whitespace() {
            if let Some((key, value)) = token.split_once('=') {
                let metadata_key = format!("log_kv_{key}");
                metadata.insert(metadata_key, serde_json::json!(value));
            }
        }

        let content_text = if message.is_empty() {
            None
        } else {
            Some(message)
        };
        let content_excerpt = content_text
            .as_deref()
            .and_then(|text| content::derive_excerpt(text, content::DEFAULT_EXCERPT_MAX_CHARS));

        let event_type = if level.eq_ignore_ascii_case("error") {
            EventType::Error
        } else {
            EventType::DebugLog
        };

        events.push(AgentLogEvent {
            schema_version: crate::models::SchemaVersion::AgentLogV1,
            event_id: format!("claude-mcp-log-line-{line_number:06}"),
            run_id: run_id.to_string(),
            sequence_global: events.len() as u64,
            sequence_source: Some(index as u64),
            source_kind: AgentSource::Claude,
            source_path: source_path.to_string(),
            source_record_locator: format!("line:{line_number}"),
            source_record_hash: None,
            adapter_name: AgentSource::Claude,
            adapter_version: Some("v1".to_string()),
            record_format: RecordFormat::Diagnostic,
            event_type,
            role: ActorRole::Runtime,
            timestamp_utc,
            timestamp_unix_ms,
            timestamp_quality,
            session_id: None,
            conversation_id: None,
            turn_id: None,
            parent_event_id: None,
            actor_id: None,
            actor_name: None,
            provider: None,
            model: None,
            content_text,
            content_excerpt,
            content_mime: Some("text/plain".to_string()),
            tool_name: None,
            tool_call_id: None,
            tool_arguments_json: None,
            tool_result_text: None,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cost_usd: None,
            tags: vec![
                "claude".to_string(),
                "mcp_cache_debug".to_string(),
                "auxiliary".to_string(),
            ],
            flags: vec!["diagnostic".to_string(), "non_conversational".to_string()],
            pii_redacted: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            raw_hash,
            canonical_hash,
            metadata,
        });
    }

    ClaudeSessionParseResult { events, warnings }
}

#[must_use]
pub fn parse_project_session_jsonl(
    input: &str,
    run_id: &str,
    source_path: &str,
) -> ClaudeSessionParseResult {
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed = match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => value,
            Err(error) => {
                warnings.push(format!(
                    "line {line_number}: invalid JSON payload ({error})"
                ));
                continue;
            }
        };

        let Some(object) = parsed.as_object() else {
            warnings.push(format!("line {line_number}: JSON root must be an object"));
            continue;
        };

        let parent_session_id = extract_string(object.get("parent_session_id"));
        let subagent_session_id = extract_string(object.get("subagent_session_id"));
        if parent_session_id.is_some() || subagent_session_id.is_some() {
            let source_role =
                extract_string(object.get("role")).unwrap_or_else(|| "unknown".to_string());
            let (record_format, canonical_event_type, role) =
                classify_subagent_role(&source_role, line_number, &mut warnings);
            if subagent_session_id.is_none() {
                warnings.push(format!(
                    "line {line_number}: missing `subagent_session_id`; preserving event with null session_id"
                ));
            }
            if parent_session_id.is_none() {
                warnings.push(format!(
                    "line {line_number}: missing `parent_session_id`; preserving event with null conversation_id"
                ));
            }

            let event_id = extract_string(object.get("event_id"))
                .or_else(|| extract_string(object.get("trace_id")))
                .unwrap_or_else(|| format!("claude-subagent-line-{line_number:06}"));
            let content_text = extract_conversation_content_text(object);
            let content_excerpt = content_text
                .as_deref()
                .and_then(|text| content::derive_excerpt(text, content::DEFAULT_EXCERPT_MAX_CHARS));
            let (timestamp_unix_ms, timestamp_utc, timestamp_quality) =
                map_timestamp(object.get("created_at"), line_number, &mut warnings);
            let raw_hash = format!("{:016x}", hash64(&trimmed));
            let canonical_hash = format!(
                "{:016x}",
                hash64(&(
                    parent_session_id.clone(),
                    subagent_session_id.clone(),
                    event_id.clone(),
                    source_role.clone(),
                    content_text.clone(),
                    line_number
                ))
            );

            let mut metadata = BTreeMap::new();
            metadata.insert("source_line".to_string(), serde_json::json!(line_number));
            metadata.insert(
                "claude_record".to_string(),
                serde_json::json!("subagent_trace"),
            );
            metadata.insert("delegated_activity".to_string(), serde_json::json!(true));
            metadata.insert("subagent_role".to_string(), serde_json::json!(source_role));
            if let Some(subagent_session_id) = &subagent_session_id {
                metadata.insert(
                    "subagent_session_id".to_string(),
                    serde_json::json!(subagent_session_id),
                );
            }
            if let Some(parent_session_id) = &parent_session_id {
                metadata.insert(
                    "parent_session_id".to_string(),
                    serde_json::json!(parent_session_id),
                );
            }

            events.push(AgentLogEvent {
                schema_version: crate::models::SchemaVersion::AgentLogV1,
                event_id,
                run_id: run_id.to_string(),
                sequence_global: events.len() as u64,
                sequence_source: Some(index as u64),
                source_kind: AgentSource::Claude,
                source_path: source_path.to_string(),
                source_record_locator: format!("line:{line_number}"),
                source_record_hash: None,
                adapter_name: AgentSource::Claude,
                adapter_version: Some("v1".to_string()),
                record_format,
                event_type: canonical_event_type,
                role,
                timestamp_utc,
                timestamp_unix_ms,
                timestamp_quality,
                session_id: subagent_session_id,
                conversation_id: parent_session_id,
                turn_id: None,
                parent_event_id: None,
                actor_id: None,
                actor_name: None,
                provider: None,
                model: None,
                content_text,
                content_excerpt,
                content_mime: Some("text/plain".to_string()),
                tool_name: None,
                tool_call_id: None,
                tool_arguments_json: None,
                tool_result_text: None,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                cost_usd: None,
                tags: vec![
                    "claude".to_string(),
                    "subagent_trace".to_string(),
                    "delegated".to_string(),
                ],
                flags: vec!["subagent".to_string(), "delegated".to_string()],
                pii_redacted: None,
                warnings: Vec::new(),
                errors: Vec::new(),
                raw_hash,
                canonical_hash,
                metadata,
            });

            continue;
        }

        let source_kind =
            extract_string(object.get("kind")).unwrap_or_else(|| "unknown".to_string());
        let (record_format, canonical_event_type, role) =
            classify_session_kind(&source_kind, line_number, &mut warnings);

        let event_id = extract_string(object.get("event_id"))
            .unwrap_or_else(|| format!("claude-line-{line_number:06}"));
        let project_id = extract_string(object.get("project_id"));
        let session_id = extract_string(object.get("session_id"));

        let content_text = extract_conversation_content_text(object);
        let content_excerpt = content_text
            .as_deref()
            .and_then(|text| content::derive_excerpt(text, content::DEFAULT_EXCERPT_MAX_CHARS));

        let (timestamp_unix_ms, timestamp_utc, timestamp_quality) =
            map_timestamp(object.get("created_at"), line_number, &mut warnings);

        let raw_hash = format!("{:016x}", hash64(&trimmed));
        let canonical_hash = format!(
            "{:016x}",
            hash64(&(
                project_id.clone(),
                session_id.clone(),
                event_id.clone(),
                source_kind.clone(),
                content_text.clone(),
                line_number
            ))
        );

        let mut metadata = BTreeMap::new();
        metadata.insert("source_line".to_string(), serde_json::json!(line_number));
        metadata.insert("claude_kind".to_string(), serde_json::json!(source_kind));
        if let Some(project_id) = &project_id {
            metadata.insert("project_id".to_string(), serde_json::json!(project_id));
        }

        events.push(AgentLogEvent {
            schema_version: crate::models::SchemaVersion::AgentLogV1,
            event_id,
            run_id: run_id.to_string(),
            sequence_global: events.len() as u64,
            sequence_source: Some(index as u64),
            source_kind: AgentSource::Claude,
            source_path: source_path.to_string(),
            source_record_locator: format!("line:{line_number}"),
            source_record_hash: None,
            adapter_name: AgentSource::Claude,
            adapter_version: Some("v1".to_string()),
            record_format,
            event_type: canonical_event_type,
            role,
            timestamp_utc,
            timestamp_unix_ms,
            timestamp_quality,
            session_id,
            conversation_id: project_id,
            turn_id: None,
            parent_event_id: None,
            actor_id: None,
            actor_name: None,
            provider: None,
            model: None,
            content_text,
            content_excerpt,
            content_mime: Some("text/plain".to_string()),
            tool_name: None,
            tool_call_id: None,
            tool_arguments_json: None,
            tool_result_text: None,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cost_usd: None,
            tags: vec!["claude".to_string(), "project_session".to_string()],
            flags: Vec::new(),
            pii_redacted: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            raw_hash,
            canonical_hash,
            metadata,
        });
    }

    ClaudeSessionParseResult { events, warnings }
}

fn classify_session_kind(
    source_kind: &str,
    line_number: usize,
    warnings: &mut Vec<String>,
) -> (RecordFormat, EventType, ActorRole) {
    match source_kind {
        "user" => (RecordFormat::Message, EventType::Prompt, ActorRole::User),
        "assistant" => (
            RecordFormat::Message,
            EventType::Response,
            ActorRole::Assistant,
        ),
        "progress" => (
            RecordFormat::System,
            EventType::StatusUpdate,
            ActorRole::Runtime,
        ),
        "system" => (
            RecordFormat::System,
            EventType::SystemNotice,
            ActorRole::System,
        ),
        _ => {
            warnings.push(format!(
                "line {line_number}: unknown `kind` value `{source_kind}`; mapped to diagnostic event"
            ));
            (
                RecordFormat::Diagnostic,
                EventType::DebugLog,
                ActorRole::Runtime,
            )
        }
    }
}

fn classify_subagent_role(
    source_role: &str,
    line_number: usize,
    warnings: &mut Vec<String>,
) -> (RecordFormat, EventType, ActorRole) {
    match source_role {
        "user" => (RecordFormat::Message, EventType::Prompt, ActorRole::User),
        "assistant" => (
            RecordFormat::Message,
            EventType::Response,
            ActorRole::Assistant,
        ),
        "system" => (
            RecordFormat::System,
            EventType::SystemNotice,
            ActorRole::System,
        ),
        "runtime" | "progress" => (
            RecordFormat::System,
            EventType::StatusUpdate,
            ActorRole::Runtime,
        ),
        _ => {
            warnings.push(format!(
                "line {line_number}: unknown subagent `role` value `{source_role}`; mapped to diagnostic event"
            ));
            (
                RecordFormat::Diagnostic,
                EventType::DebugLog,
                ActorRole::Runtime,
            )
        }
    }
}

fn classify_history_role(
    source_role: &str,
    line_number: usize,
    warnings: &mut Vec<String>,
) -> (RecordFormat, EventType, ActorRole) {
    match source_role {
        "user" | "prompt" => (RecordFormat::Message, EventType::Prompt, ActorRole::User),
        "assistant" | "response" => (
            RecordFormat::Message,
            EventType::Response,
            ActorRole::Assistant,
        ),
        "system" => (
            RecordFormat::System,
            EventType::SystemNotice,
            ActorRole::System,
        ),
        "runtime" | "progress" => (
            RecordFormat::System,
            EventType::StatusUpdate,
            ActorRole::Runtime,
        ),
        _ => {
            warnings.push(format!(
                "line {line_number}: unknown history `role` value `{source_role}`; mapped to diagnostic event"
            ));
            (
                RecordFormat::Diagnostic,
                EventType::DebugLog,
                ActorRole::Runtime,
            )
        }
    }
}

fn infer_history_role(object: &Map<String, Value>) -> &'static str {
    let has_prompt = object
        .get("prompt")
        .and_then(content::extract_text)
        .is_some();
    let has_response = object
        .get("response")
        .and_then(content::extract_text)
        .is_some();

    if has_response && !has_prompt {
        "assistant"
    } else {
        "user"
    }
}

fn looks_like_timestamp_token(token: &str) -> bool {
    token.contains('T')
        && (token.ends_with('Z') || token.contains('+') || token.rfind('-').is_some())
}

fn extract_conversation_content_text(object: &Map<String, Value>) -> Option<String> {
    extract_message_field_text(object.get("message"))
        .or_else(|| object.get("text").and_then(content::extract_text))
        .or_else(|| object.get("content").and_then(content::extract_text))
        .or_else(|| object.get("input").and_then(content::extract_text))
}

fn extract_message_field_text(message_value: Option<&Value>) -> Option<String> {
    let message = message_value?;
    let message_object = message.as_object();

    message_object
        .and_then(|object| object.get("content"))
        .and_then(content::extract_text)
        .or_else(|| {
            message_object
                .and_then(|object| object.get("text"))
                .and_then(content::extract_text)
        })
        .or_else(|| content::extract_text(message))
}

fn map_timestamp(
    created_at: Option<&Value>,
    line_number: usize,
    warnings: &mut Vec<String>,
) -> (u64, String, TimestampQuality) {
    if let Some(raw) = extract_string(created_at) {
        match normalize_timestamp_exact(&raw) {
            Ok(normalized) => {
                return (
                    normalized.timestamp_unix_ms,
                    normalized.timestamp_utc(),
                    normalized.timestamp_quality,
                );
            }
            Err(error) => {
                warnings.push(format!(
                    "line {line_number}: invalid `created_at` value ({error}); using fallback timestamp"
                ));
            }
        }
    } else {
        warnings.push(format!(
            "line {line_number}: missing `created_at`; using fallback timestamp"
        ));
    }

    let fallback_unix_ms = line_number as u64;
    (
        fallback_unix_ms,
        format_unix_ms(fallback_unix_ms),
        TimestampQuality::Fallback,
    )
}

fn extract_string(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}
