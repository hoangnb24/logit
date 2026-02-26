use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, TimestampQuality,
};
use crate::utils::content;
use crate::utils::hash::hash64;
use crate::utils::time::{format_unix_ms, normalize_timestamp_exact};

pub const DEFAULT_PATHS: &[&str] = &[
    "~/.codex/sessions",
    "~/.codex/history.jsonl",
    "~/.codex/log",
];

#[derive(Debug, Clone, PartialEq)]
pub struct CodexRolloutParseResult {
    pub events: Vec<AgentLogEvent>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexHistoryParseResult {
    pub events: Vec<AgentLogEvent>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexDiagnosticLogParseResult {
    pub events: Vec<AgentLogEvent>,
    pub warnings: Vec<String>,
}

pub fn parse_rollout_file(path: &Path, run_id: &str) -> Result<CodexRolloutParseResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read codex rollout file: {path:?}"))?;
    Ok(parse_rollout_jsonl(
        &content,
        run_id,
        path.to_string_lossy().as_ref(),
    ))
}

pub fn parse_history_file(path: &Path, run_id: &str) -> Result<CodexHistoryParseResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read codex history file: {path:?}"))?;
    Ok(parse_history_jsonl(
        &content,
        run_id,
        path.to_string_lossy().as_ref(),
    ))
}

pub fn parse_diagnostic_log_file(
    path: &Path,
    run_id: &str,
) -> Result<CodexDiagnosticLogParseResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read codex diagnostic log file: {path:?}"))?;
    Ok(parse_diagnostic_log_text(
        &content,
        run_id,
        path.to_string_lossy().as_ref(),
    ))
}

#[must_use]
pub fn parse_rollout_jsonl(
    input: &str,
    run_id: &str,
    source_path: &str,
) -> CodexRolloutParseResult {
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

        let source_event_type =
            extract_string(object.get("event_type")).unwrap_or_else(|| "unknown".to_string());
        let (record_format, canonical_event_type, role) = classify_event_family(&source_event_type);
        if !is_known_rollout_event_type(&source_event_type) {
            warnings.push(format!(
                "line {line_number}: unrecognized `event_type` `{source_event_type}`; mapped as diagnostic runtime event"
            ));
        }

        let event_id = extract_string(object.get("event_id"))
            .unwrap_or_else(|| format!("codex-line-{line_number:06}"));
        let session_id = extract_string(object.get("session_id"));
        let (content_text, content_source) = extract_rollout_content(object);
        if matches!(record_format, RecordFormat::Message) && content_text.is_none() {
            warnings.push(format!(
                "line {line_number}: missing message content text; emitting empty content"
            ));
        }
        let content_excerpt = content_text
            .as_deref()
            .and_then(|text| content::derive_excerpt(text, content::DEFAULT_EXCERPT_MAX_CHARS));
        let tool_name = extract_string(object.get("tool_name"));
        let exit_code = extract_i64(object.get("exit_code"));

        let (timestamp_unix_ms, timestamp_utc, timestamp_quality) =
            map_timestamp(object.get("created_at"), line_number, &mut warnings);

        let raw_hash = format!("{:016x}", hash64(&trimmed));
        let canonical_hash = codex_conversation_hash(
            session_id.as_deref(),
            role,
            content_text.as_deref(),
            &timestamp_utc,
        )
        .unwrap_or_else(|| {
            format!(
                "{:016x}",
                hash64(&(
                    session_id.clone(),
                    event_id.clone(),
                    source_event_type.clone(),
                    content_text.clone(),
                    line_number
                ))
            )
        });

        let mut metadata = BTreeMap::new();
        metadata.insert("source_line".to_string(), serde_json::json!(line_number));
        metadata.insert(
            "codex_event_type".to_string(),
            serde_json::json!(source_event_type),
        );
        if let Some(event_msg_category) = classify_event_msg_category(&source_event_type) {
            metadata.insert(
                "codex_event_msg_category".to_string(),
                serde_json::json!(event_msg_category),
            );
            if let Some(suffix) = source_event_type.strip_prefix("event_msg.")
                && !suffix.trim().is_empty()
            {
                metadata.insert(
                    "codex_event_msg_name".to_string(),
                    serde_json::json!(suffix),
                );
            }
        }
        if let Some(content_source) = content_source {
            metadata.insert(
                "codex_content_source".to_string(),
                serde_json::json!(content_source),
            );
        }
        if let Some(code) = exit_code {
            metadata.insert("exit_code".to_string(), serde_json::json!(code));
        }

        events.push(AgentLogEvent {
            schema_version: crate::models::SchemaVersion::AgentLogV1,
            event_id,
            run_id: run_id.to_string(),
            sequence_global: events.len() as u64,
            sequence_source: Some(index as u64),
            source_kind: AgentSource::Codex,
            source_path: source_path.to_string(),
            source_record_locator: format!("line:{line_number}"),
            source_record_hash: None,
            adapter_name: AgentSource::Codex,
            adapter_version: Some("v1".to_string()),
            record_format,
            event_type: canonical_event_type,
            role,
            timestamp_utc,
            timestamp_unix_ms,
            timestamp_quality,
            session_id,
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
            tool_name,
            tool_call_id: None,
            tool_arguments_json: None,
            tool_result_text: None,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cost_usd: None,
            tags: vec!["codex".to_string(), "rollout".to_string()],
            flags: Vec::new(),
            pii_redacted: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            raw_hash,
            canonical_hash,
            metadata,
        });
    }

    CodexRolloutParseResult { events, warnings }
}

#[must_use]
pub fn parse_history_jsonl(
    input: &str,
    run_id: &str,
    source_path: &str,
) -> CodexHistoryParseResult {
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

        let role_hint = extract_string(object.get("role"));
        let (record_format, event_type, role) =
            classify_history_role(role_hint.as_deref(), line_number, &mut warnings);

        let event_id = extract_string(object.get("prompt_id"))
            .or_else(|| extract_string(object.get("event_id")))
            .unwrap_or_else(|| format!("codex-history-line-{line_number:06}"));
        let session_id = extract_string(object.get("session_id"));
        let content_text = object
            .get("content")
            .and_then(content::extract_text)
            .or_else(|| object.get("text").and_then(content::extract_text))
            .or_else(|| object.get("message").and_then(content::extract_text));
        if content_text.is_none() {
            warnings.push(format!(
                "line {line_number}: missing content text; emitting empty content"
            ));
        }
        let content_excerpt = content_text
            .as_deref()
            .and_then(|text| content::derive_excerpt(text, content::DEFAULT_EXCERPT_MAX_CHARS));

        let (timestamp_unix_ms, timestamp_utc, timestamp_quality) =
            map_timestamp(object.get("created_at"), line_number, &mut warnings);

        let raw_hash = format!("{:016x}", hash64(&trimmed));
        let canonical_hash = codex_conversation_hash(
            session_id.as_deref(),
            role,
            content_text.as_deref(),
            &timestamp_utc,
        )
        .unwrap_or_else(|| {
            format!(
                "{:016x}",
                hash64(&(
                    session_id.clone(),
                    event_id.clone(),
                    role_hint.clone(),
                    content_text.clone(),
                    line_number
                ))
            )
        });

        let mut metadata = BTreeMap::new();
        metadata.insert("source_line".to_string(), serde_json::json!(line_number));
        if let Some(source) = extract_string(object.get("source")) {
            metadata.insert(
                "codex_history_source".to_string(),
                serde_json::json!(source),
            );
        }
        if let Some(role_hint) = role_hint.clone() {
            metadata.insert(
                "codex_history_role".to_string(),
                serde_json::json!(role_hint),
            );
        }

        events.push(AgentLogEvent {
            schema_version: crate::models::SchemaVersion::AgentLogV1,
            event_id,
            run_id: run_id.to_string(),
            sequence_global: events.len() as u64,
            sequence_source: Some(index as u64),
            source_kind: AgentSource::Codex,
            source_path: source_path.to_string(),
            source_record_locator: format!("line:{line_number}"),
            source_record_hash: None,
            adapter_name: AgentSource::Codex,
            adapter_version: Some("v1".to_string()),
            record_format,
            event_type,
            role,
            timestamp_utc,
            timestamp_unix_ms,
            timestamp_quality,
            session_id,
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
            tags: vec!["codex".to_string(), "history_auxiliary".to_string()],
            flags: Vec::new(),
            pii_redacted: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            raw_hash,
            canonical_hash,
            metadata,
        });
    }

    CodexHistoryParseResult { events, warnings }
}

#[must_use]
pub fn parse_diagnostic_log_text(
    input: &str,
    run_id: &str,
    source_path: &str,
) -> CodexDiagnosticLogParseResult {
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed = match parse_diagnostic_line(trimmed) {
            Some(parsed) => parsed,
            None => {
                warnings.push(format!(
                    "line {line_number}: unrecognized codex log shape; expected `<timestamp> <level> <subsystem> <event> [key=value ...]`"
                ));
                continue;
            }
        };

        let timestamp_value = Value::String(parsed.timestamp.clone());
        let (timestamp_unix_ms, timestamp_utc, timestamp_quality) =
            map_timestamp(Some(&timestamp_value), line_number, &mut warnings);

        let content_text = Some(parsed.payload.clone());
        let content_excerpt = content_text
            .as_deref()
            .and_then(|text| content::derive_excerpt(text, content::DEFAULT_EXCERPT_MAX_CHARS));

        let raw_hash = format!("{:016x}", hash64(&trimmed));
        let canonical_hash = format!(
            "{:016x}",
            hash64(&(
                "codex.diagnostic.v1",
                parsed.timestamp.clone(),
                parsed.level.clone(),
                parsed.subsystem.clone(),
                parsed.payload.clone()
            ))
        );

        let event_type = classify_diagnostic_level(&parsed.level);
        if !is_known_diagnostic_level(&parsed.level) {
            warnings.push(format!(
                "line {line_number}: unrecognized diagnostic log level `{}`; mapped as debug_log",
                parsed.level
            ));
        }
        let source_tag = if parsed.subsystem.contains("desktop") {
            "desktop_diagnostic".to_string()
        } else if parsed.subsystem.contains("tui") {
            "tui_diagnostic".to_string()
        } else {
            "runtime_diagnostic".to_string()
        };
        let mut metadata = BTreeMap::new();
        metadata.insert("source_line".to_string(), serde_json::json!(line_number));
        metadata.insert(
            "codex_log_level".to_string(),
            serde_json::json!(parsed.level),
        );
        metadata.insert(
            "codex_log_subsystem".to_string(),
            serde_json::json!(parsed.subsystem.clone()),
        );
        metadata.insert(
            "codex_log_event".to_string(),
            serde_json::json!(parsed.event.clone()),
        );
        if !parsed.fields.is_empty() {
            metadata.insert(
                "codex_log_fields".to_string(),
                serde_json::to_value(&parsed.fields)
                    .expect("codex diagnostic fields should serialize"),
            );
        }

        events.push(AgentLogEvent {
            schema_version: crate::models::SchemaVersion::AgentLogV1,
            event_id: format!("codex-log-line-{line_number:06}"),
            run_id: run_id.to_string(),
            sequence_global: events.len() as u64,
            sequence_source: Some(index as u64),
            source_kind: AgentSource::Codex,
            source_path: source_path.to_string(),
            source_record_locator: format!("line:{line_number}"),
            source_record_hash: None,
            adapter_name: AgentSource::Codex,
            adapter_version: Some("v1".to_string()),
            record_format: RecordFormat::Diagnostic,
            event_type,
            role: ActorRole::Runtime,
            timestamp_utc,
            timestamp_unix_ms,
            timestamp_quality,
            session_id: parsed
                .fields
                .get("session")
                .cloned()
                .or_else(|| parsed.fields.get("session_id").cloned()),
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
                "codex".to_string(),
                "diagnostic_log".to_string(),
                source_tag,
            ],
            flags: Vec::new(),
            pii_redacted: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            raw_hash,
            canonical_hash,
            metadata,
        });
    }

    CodexDiagnosticLogParseResult { events, warnings }
}

fn classify_event_family(source_event_type: &str) -> (RecordFormat, EventType, ActorRole) {
    match source_event_type {
        "user_prompt" => (RecordFormat::Message, EventType::Prompt, ActorRole::User),
        "assistant_response" => (
            RecordFormat::Message,
            EventType::Response,
            ActorRole::Assistant,
        ),
        "tool_result" => (
            RecordFormat::ToolResult,
            EventType::ToolOutput,
            ActorRole::Tool,
        ),
        event if event.starts_with("event_msg") => match classify_event_msg_category(event) {
            Some("meta") => (
                RecordFormat::System,
                EventType::SystemNotice,
                ActorRole::Runtime,
            ),
            Some("progress") | Some("generic") => (
                RecordFormat::System,
                EventType::StatusUpdate,
                ActorRole::Runtime,
            ),
            _ => (
                RecordFormat::System,
                EventType::StatusUpdate,
                ActorRole::Runtime,
            ),
        },
        _ => (
            RecordFormat::Diagnostic,
            EventType::DebugLog,
            ActorRole::Runtime,
        ),
    }
}

fn classify_event_msg_category(source_event_type: &str) -> Option<&'static str> {
    if !source_event_type.starts_with("event_msg") {
        return None;
    }

    let normalized = source_event_type.to_ascii_lowercase();
    if normalized.contains("meta") {
        Some("meta")
    } else if normalized.contains("progress") || normalized.contains("status") {
        Some("progress")
    } else {
        Some("generic")
    }
}

fn extract_rollout_content(
    object: &serde_json::Map<String, Value>,
) -> (Option<String>, Option<&'static str>) {
    for (key, label) in [
        ("text", "text"),
        ("message", "message"),
        ("response_item", "response_item"),
        ("response_items", "response_items"),
        ("output", "output"),
        ("content", "content"),
    ] {
        if let Some(value) = object.get(key)
            && let Some(text) = content::extract_text(value)
        {
            return (Some(text), Some(label));
        }
    }
    (None, None)
}

fn is_known_rollout_event_type(source_event_type: &str) -> bool {
    matches!(
        source_event_type,
        "user_prompt" | "assistant_response" | "tool_result"
    ) || source_event_type.starts_with("event_msg")
}

fn classify_history_role(
    role_hint: Option<&str>,
    line_number: usize,
    warnings: &mut Vec<String>,
) -> (RecordFormat, EventType, ActorRole) {
    match role_hint
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("user") => (RecordFormat::Message, EventType::Prompt, ActorRole::User),
        Some("assistant") => (
            RecordFormat::Message,
            EventType::Response,
            ActorRole::Assistant,
        ),
        Some("system") => (
            RecordFormat::System,
            EventType::SystemNotice,
            ActorRole::System,
        ),
        Some("tool") => (
            RecordFormat::ToolResult,
            EventType::ToolOutput,
            ActorRole::Tool,
        ),
        Some(other) => {
            warnings.push(format!(
                "line {line_number}: unrecognized history role `{other}`; mapped as diagnostic runtime event"
            ));
            (
                RecordFormat::Diagnostic,
                EventType::DebugLog,
                ActorRole::Runtime,
            )
        }
        None => {
            warnings.push(format!(
                "line {line_number}: missing history role; mapped as diagnostic runtime event"
            ));
            (
                RecordFormat::Diagnostic,
                EventType::DebugLog,
                ActorRole::Runtime,
            )
        }
    }
}

fn classify_diagnostic_level(level: &str) -> EventType {
    match level.trim().to_ascii_uppercase().as_str() {
        "ERROR" | "FATAL" => EventType::Error,
        "WARN" | "WARNING" => EventType::StatusUpdate,
        _ => EventType::DebugLog,
    }
}

fn is_known_diagnostic_level(level: &str) -> bool {
    matches!(
        level.trim().to_ascii_uppercase().as_str(),
        "TRACE" | "DEBUG" | "INFO" | "WARN" | "WARNING" | "ERROR" | "FATAL"
    )
}

#[derive(Debug, Clone)]
struct ParsedDiagnosticLine {
    timestamp: String,
    level: String,
    subsystem: String,
    event: String,
    payload: String,
    fields: BTreeMap<String, String>,
}

fn parse_diagnostic_line(line: &str) -> Option<ParsedDiagnosticLine> {
    let mut tokens = line.split_whitespace();
    let timestamp = tokens.next()?.to_string();
    let level = tokens.next()?.to_string();
    let subsystem = tokens.next()?.to_string();
    let event = tokens.next()?.to_string();
    let tail_tokens = tokens.map(str::to_string).collect::<Vec<_>>();
    let mut payload_tokens = vec![event.clone()];
    payload_tokens.extend(tail_tokens.iter().cloned());

    let mut fields = BTreeMap::new();
    for token in &tail_tokens {
        if let Some((key, value)) = token.split_once('=')
            && !key.is_empty()
            && !value.is_empty()
        {
            fields.insert(key.to_string(), value.to_string());
        }
    }

    Some(ParsedDiagnosticLine {
        timestamp,
        level,
        subsystem,
        event,
        payload: payload_tokens.join(" "),
        fields,
    })
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

fn extract_i64(value: Option<&Value>) -> Option<i64> {
    value?.as_i64()
}

fn codex_conversation_hash(
    session_id: Option<&str>,
    role: ActorRole,
    content_text: Option<&str>,
    timestamp_utc: &str,
) -> Option<String> {
    if !matches!(role, ActorRole::User | ActorRole::Assistant) {
        return None;
    }

    let normalized_content = content_text
        .map(|content| content.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    if normalized_content.is_empty() {
        return None;
    }

    Some(format!(
        "{:016x}",
        hash64(&(
            "codex.conversation.v1",
            session_id.unwrap_or(""),
            actor_role_key(role),
            timestamp_utc,
            normalized_content
        ))
    ))
}

const fn actor_role_key(role: ActorRole) -> &'static str {
    match role {
        ActorRole::User => "user",
        ActorRole::Assistant => "assistant",
        ActorRole::System => "system",
        ActorRole::Tool => "tool",
        ActorRole::Runtime => "runtime",
    }
}
