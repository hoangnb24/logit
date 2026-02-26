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

    for (index, record) in records.iter().enumerate() {
        let record_number = index + 1;
        let Some(object) = record.as_object() else {
            warnings.push(format!(
                "record {record_number}: entry is not an object; skipped"
            ));
            continue;
        };

        let event_id = extract_string(object, &["event_id", "id"])
            .unwrap_or_else(|| format!("gemini-log-{record_number:06}"));
        let role_hint = extract_string(object, &["role", "actor"]);
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

        let conversation_id = extract_string(object, &["conversation_id", "session_id"]);
        let model = extract_string(object, &["model", "model_name"]);

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
            session_id: None,
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

    let root_conversation_id = extract_string(
        root,
        &["conversation_id", "conversationId", "chat_id", "chatId"],
    );
    let root_session_id = extract_string(root, &["session_id", "sessionId"]);
    let root_model = extract_string(root, &["model", "model_name"]);

    for (index, message) in messages.iter().enumerate() {
        let message_number = index + 1;
        let Some(object) = message.as_object() else {
            warnings.push(format!(
                "message {message_number}: entry is not an object; skipped"
            ));
            continue;
        };

        let role_hint = extract_string(object, &["role", "author", "actor"]);
        let (record_format, event_type, role) =
            classify_chat_role(role_hint.as_deref(), message_number, &mut warnings);
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

        let event_id = extract_string(object, &["event_id", "id", "message_id"])
            .unwrap_or_else(|| format!("gemini-chat-{message_number:06}"));
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
        let message_model = extract_string(object, &["model", "model_name"]);
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
            tool_name: None,
            tool_call_id: None,
            tool_arguments_json: None,
            tool_result_text: None,
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
            "assistant" | "model" => {
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
            "user" | "assistant" | "model" | "system" | "tool"
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
        let Some(raw) = object.get(*key).and_then(Value::as_str) else {
            continue;
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}
