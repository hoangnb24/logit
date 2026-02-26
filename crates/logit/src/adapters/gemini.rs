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

pub const DEFAULT_PATHS: &[&str] = &["~/.gemini/tmp", "~/.gemini/history", "~/.gemini/debug"];

#[derive(Debug, Clone, PartialEq)]
pub struct GeminiLogsParseResult {
    pub events: Vec<AgentLogEvent>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeminiChatParseResult {
    pub events: Vec<AgentLogEvent>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct GeminiToolDetails {
    tool_name: Option<String>,
    tool_call_id: Option<String>,
    tool_arguments_json: Option<String>,
    tool_result_text: Option<String>,
    tool_calls_count: usize,
}

pub fn parse_logs_file(path: &Path, run_id: &str) -> Result<GeminiLogsParseResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read gemini logs file: {path:?}"))?;
    parse_logs_json_array(&content, run_id, path.to_string_lossy().as_ref())
}

pub fn parse_logs_json_array(
    input: &str,
    run_id: &str,
    source_path: &str,
) -> Result<GeminiLogsParseResult> {
    let parsed =
        serde_json::from_str::<Value>(input).context("gemini logs payload must be valid JSON")?;
    let records = parsed
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("gemini logs payload root must be an array"))?;

    let mut events = Vec::new();
    let mut warnings = Vec::new();
    let source_path_hash = hash64(&source_path.to_string());

    for (index, record) in records.iter().enumerate() {
        let record_number = index + 1;
        let Some(object) = record.as_object() else {
            warnings.push(format!(
                "record {record_number}: entry is not an object; skipped"
            ));
            continue;
        };

        let source_event_id =
            extract_string(object, &["event_id", "id", "message_id", "messageId"]);
        let event_id = source_event_id
            .as_ref()
            .map(|value| format!("gemini-{source_path_hash:016x}-{value}"))
            .unwrap_or_else(|| format!("gemini-log-{source_path_hash:016x}-{record_number:06}"));
        let role_hint = extract_string(object, &["role", "actor", "type"]);
        let level_hint = extract_string(object, &["level", "severity"]);
        let kind_hint = extract_string(object, &["event_type", "kind", "type"]);
        let (record_format, event_type, role) = classify_record(
            role_hint.as_deref(),
            level_hint.as_deref(),
            kind_hint.as_deref(),
        );

        let content_text = object
            .get("message")
            .and_then(content::extract_text)
            .or_else(|| object.get("text").and_then(content::extract_text))
            .or_else(|| object.get("content").and_then(content::extract_text));
        let content_excerpt = content_text
            .as_deref()
            .and_then(|text| content::derive_excerpt(text, content::DEFAULT_EXCERPT_MAX_CHARS));

        let (timestamp_unix_ms, timestamp_utc, timestamp_quality) =
            map_timestamp(object, record_number, &mut warnings);

        let message_session_id = extract_string(object, &["session_id", "sessionId"]);
        let conversation_id = extract_string(
            object,
            &["conversation_id", "conversationId", "chat_id", "chatId"],
        )
        .or_else(|| message_session_id.clone());
        let model = extract_string(object, &["model", "model_name", "modelName"]);

        let raw_hash = format!("{:016x}", hash64(&record.to_string()));
        let canonical_hash = format!(
            "{:016x}",
            hash64(&(
                event_id.clone(),
                source_path,
                record_number,
                role_hint.clone(),
                level_hint.clone(),
                kind_hint.clone(),
                content_text.clone()
            ))
        );

        let mut metadata = BTreeMap::new();
        metadata.insert("source_index".to_string(), serde_json::json!(record_number));
        if let Some(level) = &level_hint {
            metadata.insert("gemini_level".to_string(), serde_json::json!(level));
        }
        if let Some(kind) = &kind_hint {
            metadata.insert("gemini_kind".to_string(), serde_json::json!(kind));
        }
        if let Some(source_event_id) = &source_event_id {
            metadata.insert(
                "gemini_source_event_id".to_string(),
                serde_json::json!(source_event_id),
            );
        }
        if message_session_id.is_some() {
            metadata.insert(
                "gemini_session_id_source".to_string(),
                serde_json::json!("record"),
            );
        }

        events.push(AgentLogEvent {
            schema_version: crate::models::SchemaVersion::AgentLogV1,
            event_id,
            run_id: run_id.to_string(),
            sequence_global: events.len() as u64,
            sequence_source: Some(index as u64),
            source_kind: AgentSource::Gemini,
            source_path: source_path.to_string(),
            source_record_locator: format!("index:{record_number}"),
            source_record_hash: None,
            adapter_name: AgentSource::Gemini,
            adapter_version: Some("v1".to_string()),
            record_format,
            event_type,
            role,
            timestamp_utc,
            timestamp_unix_ms,
            timestamp_quality,
            session_id: message_session_id,
            conversation_id,
            turn_id: None,
            parent_event_id: None,
            actor_id: None,
            actor_name: None,
            provider: Some("google".to_string()),
            model,
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
            tags: vec!["gemini".to_string(), "logs_json".to_string()],
            flags: Vec::new(),
            pii_redacted: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            raw_hash,
            canonical_hash,
            metadata,
        });
    }

    Ok(GeminiLogsParseResult { events, warnings })
}

pub fn parse_chat_session_file(path: &Path, run_id: &str) -> Result<GeminiChatParseResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read gemini chat session file: {path:?}"))?;
    parse_chat_session_json(&content, run_id, path.to_string_lossy().as_ref())
}

pub fn parse_chat_session_json(
    input: &str,
    run_id: &str,
    source_path: &str,
) -> Result<GeminiChatParseResult> {
    let parsed =
        serde_json::from_str::<Value>(input).context("gemini chat payload must be valid JSON")?;
    let root = parsed
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("gemini chat payload root must be an object"))?;
    let messages = root
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("gemini chat payload must include `messages` array"))?;

    let mut events = Vec::new();
    let mut warnings = Vec::new();
    let source_path_hash = hash64(&source_path.to_string());

    let root_conversation_id = extract_string(
        root,
        &["conversation_id", "conversationId", "chat_id", "chatId"],
    );
    let root_session_id = extract_string(root, &["session_id", "sessionId"]);
    let root_model = extract_string(root, &["model", "model_name", "modelName"]);

    for (index, message) in messages.iter().enumerate() {
        let message_number = index + 1;
        let Some(object) = message.as_object() else {
            warnings.push(format!(
                "message {message_number}: entry is not an object; skipped"
            ));
            continue;
        };

        let role_hint = extract_string(object, &["role", "author", "actor", "type"]);
        let base_classification =
            classify_chat_role(role_hint.as_deref(), message_number, &mut warnings);
        let tool_details = extract_tool_details(object);
        let (record_format, event_type, role) =
            classify_with_tool_details(base_classification, &tool_details);
        let conversation_id = extract_string(
            object,
            &["conversation_id", "conversationId", "chat_id", "chatId"],
        );
        let session_id = extract_string(object, &["session_id", "sessionId"]);
        let conversation_id_source = if conversation_id.is_some() {
            Some("message")
        } else if root_conversation_id.is_some() {
            Some("root")
        } else {
            None
        };
        let session_id_source = if session_id.is_some() {
            Some("message")
        } else if root_session_id.is_some() {
            Some("root")
        } else {
            None
        };
        let conversation_id = conversation_id.or_else(|| root_conversation_id.clone());
        let session_id = session_id.or_else(|| root_session_id.clone());

        let source_event_id =
            extract_string(object, &["event_id", "id", "message_id", "messageId"]);
        let event_id = source_event_id
            .as_ref()
            .map(|value| format!("gemini-{source_path_hash:016x}-{value}"))
            .unwrap_or_else(|| format!("gemini-chat-{source_path_hash:016x}-{message_number:06}"));
        let (content_text, content_source) = extract_chat_content(object);
        if content_text.is_none() {
            warnings.push(format!(
                "message {message_number}: missing content text; emitting empty content"
            ));
        }
        let content_excerpt = content_text
            .as_deref()
            .and_then(|text| content::derive_excerpt(text, content::DEFAULT_EXCERPT_MAX_CHARS));

        let (timestamp_unix_ms, timestamp_utc, timestamp_quality) =
            map_timestamp(object, message_number, &mut warnings);
        let message_model = extract_string(object, &["model", "model_name", "modelName"]);
        let model_source = if message_model.is_some() {
            Some("message")
        } else if root_model.is_some() {
            Some("root")
        } else {
            None
        };
        let model = message_model.or_else(|| root_model.clone());

        let raw_hash = format!("{:016x}", hash64(&message.to_string()));
        let canonical_hash = format!(
            "{:016x}",
            hash64(&(
                event_id.clone(),
                conversation_id.clone(),
                session_id.clone(),
                role_hint.clone(),
                content_text.clone(),
                message_number
            ))
        );

        let mut metadata = BTreeMap::new();
        metadata.insert(
            "source_index".to_string(),
            serde_json::json!(message_number),
        );
        if let Some(role) = role_hint {
            metadata.insert("gemini_role".to_string(), serde_json::json!(role));
        }
        if let Some(root_conversation_id) = &root_conversation_id {
            metadata.insert(
                "gemini_root_conversation_id".to_string(),
                serde_json::json!(root_conversation_id),
            );
        }
        if let Some(root_session_id) = &root_session_id {
            metadata.insert(
                "gemini_root_session_id".to_string(),
                serde_json::json!(root_session_id),
            );
        }
        if let Some(root_model) = &root_model {
            metadata.insert(
                "gemini_root_model".to_string(),
                serde_json::json!(root_model),
            );
        }
        if let Some(source) = conversation_id_source {
            metadata.insert(
                "gemini_conversation_id_source".to_string(),
                serde_json::json!(source),
            );
        }
        if let Some(source) = session_id_source {
            metadata.insert(
                "gemini_session_id_source".to_string(),
                serde_json::json!(source),
            );
        }
        if let Some(source) = model_source {
            metadata.insert("gemini_model_source".to_string(), serde_json::json!(source));
        }
        if let Some(content_source) = content_source {
            metadata.insert(
                "gemini_content_source".to_string(),
                serde_json::json!(content_source),
            );
        }
        if let Some(source_event_id) = &source_event_id {
            metadata.insert(
                "gemini_source_event_id".to_string(),
                serde_json::json!(source_event_id),
            );
        }
        if tool_details.tool_calls_count > 0 {
            metadata.insert(
                "gemini_tool_calls_count".to_string(),
                serde_json::json!(tool_details.tool_calls_count),
            );
        }
        if let Some(parts_count) = object
            .get("content")
            .and_then(Value::as_array)
            .or_else(|| object.get("parts").and_then(Value::as_array))
            .map(Vec::len)
        {
            metadata.insert(
                "gemini_content_parts".to_string(),
                serde_json::json!(parts_count),
            );
        }

        events.push(AgentLogEvent {
            schema_version: crate::models::SchemaVersion::AgentLogV1,
            event_id,
            run_id: run_id.to_string(),
            sequence_global: events.len() as u64,
            sequence_source: Some(index as u64),
            source_kind: AgentSource::Gemini,
            source_path: source_path.to_string(),
            source_record_locator: format!("messages:{message_number}"),
            source_record_hash: None,
            adapter_name: AgentSource::Gemini,
            adapter_version: Some("v1".to_string()),
            record_format,
            event_type,
            role,
            timestamp_utc,
            timestamp_unix_ms,
            timestamp_quality,
            session_id: session_id.clone(),
            conversation_id: conversation_id.clone(),
            turn_id: None,
            parent_event_id: None,
            actor_id: None,
            actor_name: None,
            provider: Some("google".to_string()),
            model,
            content_text,
            content_excerpt,
            content_mime: Some("text/plain".to_string()),
            tool_name: tool_details.tool_name,
            tool_call_id: tool_details.tool_call_id,
            tool_arguments_json: tool_details.tool_arguments_json,
            tool_result_text: tool_details.tool_result_text,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cost_usd: None,
            tags: vec!["gemini".to_string(), "chat_session".to_string()],
            flags: Vec::new(),
            pii_redacted: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            raw_hash,
            canonical_hash,
            metadata,
        });
    }

    Ok(GeminiChatParseResult { events, warnings })
}

fn classify_record(
    role_hint: Option<&str>,
    level_hint: Option<&str>,
    kind_hint: Option<&str>,
) -> (RecordFormat, EventType, ActorRole) {
    if let Some(role) = role_hint {
        match role.to_ascii_lowercase().as_str() {
            "user" => return (RecordFormat::Message, EventType::Prompt, ActorRole::User),
            "assistant" | "model" | "gemini" => {
                return (
                    RecordFormat::Message,
                    EventType::Response,
                    ActorRole::Assistant,
                );
            }
            "system" => {
                return (
                    RecordFormat::System,
                    EventType::SystemNotice,
                    ActorRole::System,
                );
            }
            "tool" => {
                return (
                    RecordFormat::ToolResult,
                    EventType::ToolOutput,
                    ActorRole::Tool,
                );
            }
            _ => {}
        }
    }

    if let Some(level) = level_hint {
        match level.to_ascii_lowercase().as_str() {
            "error" | "fatal" => {
                return (
                    RecordFormat::Diagnostic,
                    EventType::Error,
                    ActorRole::Runtime,
                );
            }
            "warn" | "warning" => {
                return (
                    RecordFormat::Diagnostic,
                    EventType::StatusUpdate,
                    ActorRole::Runtime,
                );
            }
            _ => {}
        }
    }

    if let Some(kind) = kind_hint {
        match kind.to_ascii_lowercase().as_str() {
            "user" | "prompt" => {
                return (RecordFormat::Message, EventType::Prompt, ActorRole::User);
            }
            "assistant" | "model" | "gemini" | "response" => {
                return (
                    RecordFormat::Message,
                    EventType::Response,
                    ActorRole::Assistant,
                );
            }
            "tool_call" | "tool_invocation" => {
                return (
                    RecordFormat::ToolCall,
                    EventType::ToolInvocation,
                    ActorRole::Tool,
                );
            }
            "tool" | "tool_result" | "tool_output" => {
                return (
                    RecordFormat::ToolResult,
                    EventType::ToolOutput,
                    ActorRole::Tool,
                );
            }
            "system" => {
                return (
                    RecordFormat::System,
                    EventType::SystemNotice,
                    ActorRole::System,
                );
            }
            "warn" | "warning" | "status" | "notice" => {
                return (
                    RecordFormat::Diagnostic,
                    EventType::StatusUpdate,
                    ActorRole::Runtime,
                );
            }
            "error" | "fatal" => {
                return (
                    RecordFormat::Diagnostic,
                    EventType::Error,
                    ActorRole::Runtime,
                );
            }
            "metric" => {
                return (
                    RecordFormat::Diagnostic,
                    EventType::Metric,
                    ActorRole::Runtime,
                );
            }
            "artifact" | "artifact_reference" => {
                return (
                    RecordFormat::Diagnostic,
                    EventType::ArtifactReference,
                    ActorRole::Runtime,
                );
            }
            "info" | "debug" | "trace" | "log" => {
                return (
                    RecordFormat::Diagnostic,
                    EventType::DebugLog,
                    ActorRole::Runtime,
                );
            }
            _ => {}
        }
    }

    (
        RecordFormat::Diagnostic,
        EventType::DebugLog,
        ActorRole::Runtime,
    )
}

fn map_timestamp(
    object: &serde_json::Map<String, Value>,
    record_number: usize,
    warnings: &mut Vec<String>,
) -> (u64, String, TimestampQuality) {
    if let Some(raw_timestamp) = extract_string(object, &["timestamp", "created_at", "time"]) {
        match normalize_timestamp_exact(&raw_timestamp) {
            Ok(normalized) => {
                return (
                    normalized.timestamp_unix_ms,
                    normalized.timestamp_utc(),
                    normalized.timestamp_quality,
                );
            }
            Err(error) => {
                warnings.push(format!(
                    "record {record_number}: invalid timestamp `{raw_timestamp}` ({error}); using fallback"
                ));
            }
        }
    } else {
        warnings.push(format!(
            "record {record_number}: missing timestamp; using fallback"
        ));
    }

    let fallback_unix_ms = record_number as u64;
    (
        fallback_unix_ms,
        format_unix_ms(fallback_unix_ms),
        TimestampQuality::Fallback,
    )
}

fn classify_chat_role(
    role_hint: Option<&str>,
    message_number: usize,
    warnings: &mut Vec<String>,
) -> (RecordFormat, EventType, ActorRole) {
    if role_hint.is_none() {
        warnings.push(format!(
            "message {message_number}: missing role; mapped to diagnostic runtime event"
        ));
    }

    let classification = classify_record(role_hint, None, None);
    if let Some(role) = role_hint {
        let normalized = role.to_ascii_lowercase();
        if !matches!(
            normalized.as_str(),
            "user" | "assistant" | "model" | "gemini" | "system" | "tool"
        ) {
            warnings.push(format!(
                "message {message_number}: unknown role `{role}`; mapped to diagnostic runtime event"
            ));
        }
    }
    classification
}

fn extract_chat_content(
    object: &serde_json::Map<String, Value>,
) -> (Option<String>, Option<&'static str>) {
    for (key, label) in [
        ("content", "content"),
        ("parts", "parts"),
        ("text", "text"),
        ("message", "message"),
        ("response", "response"),
        ("responses", "responses"),
        ("candidate", "candidate"),
        ("candidates", "candidates"),
        ("payload", "payload"),
    ] {
        if let Some(value) = object.get(key)
            && let Some(text) = content::extract_text(value)
        {
            return (Some(text), Some(label));
        }
    }

    (None, None)
}

fn extract_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(raw) = object.get(*key).and_then(scalar_to_string) else {
            continue;
        };
        if !raw.is_empty() {
            return Some(raw);
        }
    }
    None
}

fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn classify_with_tool_details(
    fallback: (RecordFormat, EventType, ActorRole),
    tool_details: &GeminiToolDetails,
) -> (RecordFormat, EventType, ActorRole) {
    if tool_details.tool_result_text.is_some() {
        return (
            RecordFormat::ToolResult,
            EventType::ToolOutput,
            ActorRole::Tool,
        );
    }
    if tool_details.tool_name.is_some() || tool_details.tool_arguments_json.is_some() {
        return (
            RecordFormat::ToolCall,
            EventType::ToolInvocation,
            ActorRole::Tool,
        );
    }
    fallback
}

fn extract_tool_details(object: &serde_json::Map<String, Value>) -> GeminiToolDetails {
    let Some(tool_calls_value) = object
        .get("toolCalls")
        .or_else(|| object.get("tool_calls"))
        .or_else(|| object.get("toolcalls"))
    else {
        return GeminiToolDetails::default();
    };

    let tool_calls = if let Some(array) = tool_calls_value.as_array() {
        array.iter().collect::<Vec<_>>()
    } else {
        vec![tool_calls_value]
    };
    let tool_calls_count = tool_calls.len();
    let Some(first_tool_call) = tool_calls.first().and_then(|entry| entry.as_object()) else {
        return GeminiToolDetails {
            tool_calls_count,
            ..GeminiToolDetails::default()
        };
    };

    let tool_name = extract_string(
        first_tool_call,
        &["name", "toolName", "tool", "functionName"],
    );
    let tool_call_id = extract_string(
        first_tool_call,
        &["id", "toolCallId", "tool_call_id", "callId"],
    );
    let tool_arguments_json = extract_object_value(
        first_tool_call,
        &[
            "arguments",
            "args",
            "input",
            "params",
            "parameters",
            "functionArgs",
        ],
    )
    .and_then(|value| serde_json::to_string(value).ok());
    let tool_result_text = extract_object_value(
        first_tool_call,
        &[
            "result",
            "response",
            "output",
            "toolResult",
            "tool_result",
            "content",
        ],
    )
    .and_then(content::extract_text);

    GeminiToolDetails {
        tool_name,
        tool_call_id,
        tool_arguments_json,
        tool_result_text,
        tool_calls_count,
    }
}

fn extract_object_value<'a>(
    object: &'a serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<&'a Value> {
    keys.iter().find_map(|key| object.get(*key))
}
