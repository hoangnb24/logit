use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, TimestampQuality,
};
use crate::utils::content;
use crate::utils::hash::hash64;
use crate::utils::time::{format_unix_ms, normalize_timestamp_exact};

pub const DEFAULT_PATHS: &[&str] = &[
    "~/.opencode/project",
    "~/.opencode/sessions",
    "~/.opencode/logs",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeMessageMetadata {
    pub session_id: String,
    pub message_id: String,
    pub created_at: Option<String>,
    pub role: String,
    pub model: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeSessionInfo {
    pub session_id: String,
    pub title: Option<String>,
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeSessionMetadata {
    pub session_id: String,
    pub message_count: usize,
    pub first_created_at: Option<String>,
    pub last_created_at: Option<String>,
    pub roles_seen: Vec<String>,
    pub model_hints: Vec<String>,
    pub provider_hints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeMetadataParseResult {
    pub sessions: Vec<OpenCodeSessionMetadata>,
    pub session_info: Vec<OpenCodeSessionInfo>,
    pub messages: Vec<OpenCodeMessageMetadata>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OpenCodeMessageKey {
    pub session_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodePartRecord {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
    pub kind: String,
    pub text: Option<String>,
    pub is_step_event: bool,
    pub is_orphan: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodePartParseResult {
    pub parts: Vec<OpenCodePartRecord>,
    pub orphan_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeJoinedMessageParts {
    pub message: OpenCodeMessageMetadata,
    pub parts: Vec<OpenCodePartRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeJoinResult {
    pub joined_messages: Vec<OpenCodeJoinedMessageParts>,
    pub messages_without_parts: Vec<OpenCodeMessageMetadata>,
    pub orphan_parts: Vec<OpenCodePartRecord>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenCodeAuxiliaryLogParseResult {
    pub events: Vec<AgentLogEvent>,
    pub warnings: Vec<String>,
}

pub fn parse_session_metadata_jsonl(raw_jsonl: &str) -> Result<OpenCodeMetadataParseResult> {
    let mut warnings = Vec::new();
    let mut messages = Vec::new();
    let mut session_rollups: BTreeMap<String, SessionRollup> = BTreeMap::new();
    let mut session_info: BTreeMap<String, OpenCodeSessionInfo> = BTreeMap::new();

    for (line_number, line) in raw_jsonl.lines().enumerate() {
        let line_number = line_number + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(error) => {
                warnings.push(format!("line {line_number}: invalid JSON: {error}"));
                continue;
            }
        };

        let Some(object) = value.as_object() else {
            warnings.push(format!("line {line_number}: record is not a JSON object"));
            continue;
        };

        let Some(session_id) = extract_string(object, &["sessionID", "session_id"]) else {
            warnings.push(format!(
                "line {line_number}: missing required `sessionID`/`session_id`; record skipped"
            ));
            continue;
        };

        if let Some(message_id) = extract_string(object, &["messageID", "message_id"]) {
            let role = extract_string(object, &["role"]).unwrap_or_else(|| "unknown".to_string());
            let created_at = extract_string(object, &["createdAt", "created_at"]);
            let model = extract_string(object, &["model"]);
            let provider = extract_string(object, &["provider"]);

            messages.push(OpenCodeMessageMetadata {
                session_id: session_id.clone(),
                message_id,
                created_at: created_at.clone(),
                role: role.clone(),
                model: model.clone(),
                provider: provider.clone(),
            });

            let rollup = session_rollups.entry(session_id).or_default();
            rollup.message_count += 1;
            if let Some(created_at) = created_at {
                update_timestamp_bounds(
                    &created_at,
                    &mut rollup.first_created_at,
                    &mut rollup.last_created_at,
                );
            }
            rollup.roles_seen.insert(role);
            if let Some(model) = model {
                rollup.model_hints.insert(model);
            }
            if let Some(provider) = provider {
                rollup.provider_hints.insert(provider);
            }
            continue;
        }

        let entry = session_info
            .entry(session_id.clone())
            .or_insert(OpenCodeSessionInfo {
                session_id,
                title: None,
                workspace_path: None,
            });
        if let Some(title) = extract_string(object, &["title", "sessionTitle", "session_title"]) {
            entry.title = Some(title);
        }
        if let Some(workspace_path) = extract_string(
            object,
            &[
                "workspacePath",
                "workspace_path",
                "projectPath",
                "project_path",
            ],
        ) {
            entry.workspace_path = Some(workspace_path);
        }
    }

    messages.sort_by(|a, b| {
        a.session_id
            .cmp(&b.session_id)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.message_id.cmp(&b.message_id))
    });

    let sessions = session_rollups
        .into_iter()
        .map(|(session_id, rollup)| OpenCodeSessionMetadata {
            session_id,
            message_count: rollup.message_count,
            first_created_at: rollup.first_created_at,
            last_created_at: rollup.last_created_at,
            roles_seen: rollup.roles_seen.into_iter().collect(),
            model_hints: rollup.model_hints.into_iter().collect(),
            provider_hints: rollup.provider_hints.into_iter().collect(),
        })
        .collect();

    Ok(OpenCodeMetadataParseResult {
        sessions,
        session_info: session_info.into_values().collect(),
        messages,
        warnings,
    })
}

pub fn parse_auxiliary_log_file(
    path: &Path,
    run_id: &str,
) -> Result<OpenCodeAuxiliaryLogParseResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read opencode auxiliary log file: {path:?}"))?;
    Ok(parse_auxiliary_log_text(
        &content,
        run_id,
        path.to_string_lossy().as_ref(),
    ))
}

#[must_use]
pub fn parse_auxiliary_log_text(
    input: &str,
    run_id: &str,
    source_path: &str,
) -> OpenCodeAuxiliaryLogParseResult {
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed = match parse_auxiliary_log_line(trimmed) {
            Some(parsed) => parsed,
            None => {
                warnings.push(format!(
                    "line {line_number}: unrecognized auxiliary log format; line skipped"
                ));
                continue;
            }
        };

        let (timestamp_unix_ms, timestamp_utc, timestamp_quality) =
            map_auxiliary_timestamp(&parsed.timestamp, line_number, &mut warnings);
        let (record_format, event_type, role, mode_tag) =
            classify_auxiliary_subsystem(&parsed.level, &parsed.subsystem, &parsed.event);

        let mut metadata = BTreeMap::new();
        metadata.insert("source_line".to_string(), serde_json::json!(line_number));
        metadata.insert(
            "opencode_log_level".to_string(),
            serde_json::json!(parsed.level.clone()),
        );
        metadata.insert(
            "opencode_subsystem".to_string(),
            serde_json::json!(parsed.subsystem.clone()),
        );
        metadata.insert(
            "opencode_event".to_string(),
            serde_json::json!(parsed.event.clone()),
        );
        if !parsed.fields.is_empty() {
            metadata.insert(
                "opencode_fields".to_string(),
                serde_json::to_value(&parsed.fields)
                    .expect("opencode auxiliary log fields should serialize"),
            );
        }

        let content_text = if matches!(record_format, RecordFormat::Message) {
            Some(parsed.payload.clone())
        } else {
            None
        };
        let content_excerpt = content_text
            .as_deref()
            .and_then(|text| content::derive_excerpt(text, content::DEFAULT_EXCERPT_MAX_CHARS));

        let input_tokens = parse_u64_field(&parsed.fields, "prompt");
        let output_tokens = parse_u64_field(&parsed.fields, "completion");
        let total_tokens = match (input_tokens, output_tokens) {
            (Some(prompt), Some(completion)) => Some(prompt + completion),
            _ => None,
        };

        let raw_hash = format!("{:016x}", hash64(&trimmed));
        let canonical_hash = format!(
            "{:016x}",
            hash64(&(
                source_path,
                line_number,
                parsed.subsystem.as_str(),
                parsed.event.as_str(),
                parsed
                    .fields
                    .get("session")
                    .or(parsed.fields.get("session_id")),
                parsed.fields.get("message_id"),
                parsed.payload.as_str()
            ))
        );

        let mut tags = vec!["opencode".to_string()];
        if matches!(record_format, RecordFormat::Diagnostic) {
            tags.push("diagnostic_log".to_string());
        }
        tags.push(mode_tag.to_string());

        events.push(AgentLogEvent {
            schema_version: crate::models::SchemaVersion::AgentLogV1,
            event_id: parsed
                .fields
                .get("message_id")
                .cloned()
                .unwrap_or_else(|| format!("opencode-aux-line-{line_number:06}")),
            run_id: run_id.to_string(),
            sequence_global: events.len() as u64,
            sequence_source: Some(index as u64),
            source_kind: AgentSource::OpenCode,
            source_path: source_path.to_string(),
            source_record_locator: format!("line:{line_number}"),
            source_record_hash: None,
            adapter_name: AgentSource::OpenCode,
            adapter_version: Some("v1".to_string()),
            record_format,
            event_type,
            role,
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
            input_tokens,
            output_tokens,
            total_tokens,
            cost_usd: None,
            tags,
            flags: Vec::new(),
            pii_redacted: None,
            warnings: Vec::new(),
            errors: Vec::new(),
            raw_hash,
            canonical_hash,
            metadata,
        });
    }

    OpenCodeAuxiliaryLogParseResult { events, warnings }
}

#[must_use]
pub fn build_message_key_index(
    messages: &[OpenCodeMessageMetadata],
) -> BTreeSet<OpenCodeMessageKey> {
    messages
        .iter()
        .map(|message| OpenCodeMessageKey {
            session_id: message.session_id.clone(),
            message_id: message.message_id.clone(),
        })
        .collect()
}

pub fn parse_part_records_jsonl(
    raw_jsonl: &str,
    known_messages: Option<&BTreeSet<OpenCodeMessageKey>>,
) -> Result<OpenCodePartParseResult> {
    let mut warnings = Vec::new();
    let mut parts = Vec::new();
    let mut orphan_count = 0_usize;

    for (line_number, line) in raw_jsonl.lines().enumerate() {
        let line_number = line_number + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(error) => {
                warnings.push(format!("line {line_number}: invalid JSON: {error}"));
                continue;
            }
        };

        let Some(object) = value.as_object() else {
            warnings.push(format!("line {line_number}: record is not a JSON object"));
            continue;
        };

        let Some(session_id) = extract_string(object, &["sessionID", "session_id"]) else {
            warnings.push(format!(
                "line {line_number}: missing required `sessionID`/`session_id`; part skipped"
            ));
            continue;
        };
        let Some(message_id) = extract_string(object, &["messageID", "message_id"]) else {
            warnings.push(format!(
                "line {line_number}: missing required `messageID`/`message_id`; part skipped"
            ));
            continue;
        };
        let Some(part_id) = extract_string(object, &["partID", "part_id"]) else {
            warnings.push(format!(
                "line {line_number}: missing required `partID`/`part_id`; part skipped"
            ));
            continue;
        };
        let kind = extract_string(object, &["kind"]).unwrap_or_else(|| "unknown".to_string());
        let text = object
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string);

        let is_step_event = is_step_kind(&kind);
        let is_orphan = known_messages.is_some_and(|index| {
            !index.contains(&OpenCodeMessageKey {
                session_id: session_id.clone(),
                message_id: message_id.clone(),
            })
        });
        if is_orphan {
            orphan_count += 1;
            warnings.push(format!(
                "line {line_number}: orphan part `{part_id}` for missing message `{message_id}` in session `{session_id}`"
            ));
        }

        parts.push(OpenCodePartRecord {
            session_id,
            message_id,
            part_id,
            kind,
            text,
            is_step_event,
            is_orphan,
        });
    }

    parts.sort_by(|a, b| {
        a.session_id
            .cmp(&b.session_id)
            .then_with(|| a.message_id.cmp(&b.message_id))
            .then_with(|| a.part_id.cmp(&b.part_id))
    });

    Ok(OpenCodePartParseResult {
        parts,
        orphan_count,
        warnings,
    })
}

#[must_use]
pub fn join_message_metadata_with_parts(
    messages: &[OpenCodeMessageMetadata],
    parts: &[OpenCodePartRecord],
) -> OpenCodeJoinResult {
    let mut sorted_messages = messages.to_vec();
    sorted_messages.sort_by(|left, right| {
        left.session_id
            .cmp(&right.session_id)
            .then_with(|| left.created_at.cmp(&right.created_at))
            .then_with(|| left.message_id.cmp(&right.message_id))
    });

    let message_index = build_message_key_index(&sorted_messages);
    let mut parts_by_message: BTreeMap<OpenCodeMessageKey, Vec<OpenCodePartRecord>> =
        BTreeMap::new();
    let mut orphan_parts = Vec::new();
    let mut warnings = Vec::new();

    for part in parts {
        let key = OpenCodeMessageKey {
            session_id: part.session_id.clone(),
            message_id: part.message_id.clone(),
        };
        if part.is_orphan || !message_index.contains(&key) {
            orphan_parts.push(part.clone());
            warnings.push(format!(
                "orphan part `{}` for message `{}` in session `{}`",
                part.part_id, part.message_id, part.session_id
            ));
            continue;
        }

        parts_by_message.entry(key).or_default().push(part.clone());
    }

    for parts in parts_by_message.values_mut() {
        parts.sort_by(|left, right| {
            left.part_id
                .cmp(&right.part_id)
                .then_with(|| left.kind.cmp(&right.kind))
        });
    }

    orphan_parts.sort_by(|left, right| {
        left.session_id
            .cmp(&right.session_id)
            .then_with(|| left.message_id.cmp(&right.message_id))
            .then_with(|| left.part_id.cmp(&right.part_id))
    });

    let mut joined_messages = Vec::with_capacity(sorted_messages.len());
    let mut messages_without_parts = Vec::new();
    for message in sorted_messages {
        let key = OpenCodeMessageKey {
            session_id: message.session_id.clone(),
            message_id: message.message_id.clone(),
        };
        let matched_parts = parts_by_message.remove(&key).unwrap_or_default();
        if matched_parts.is_empty() {
            warnings.push(format!(
                "message `{}` in session `{}` has no part records",
                message.message_id, message.session_id
            ));
            messages_without_parts.push(message.clone());
        }

        joined_messages.push(OpenCodeJoinedMessageParts {
            message,
            parts: matched_parts,
        });
    }

    OpenCodeJoinResult {
        joined_messages,
        messages_without_parts,
        orphan_parts,
        warnings,
    }
}

#[derive(Debug, Default)]
struct SessionRollup {
    message_count: usize,
    first_created_at: Option<String>,
    last_created_at: Option<String>,
    roles_seen: BTreeSet<String>,
    model_hints: BTreeSet<String>,
    provider_hints: BTreeSet<String>,
}

fn extract_string(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(value) = object.get(*key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        if let Some(text) = value.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

fn update_timestamp_bounds(
    created_at: &str,
    first: &mut Option<String>,
    last: &mut Option<String>,
) {
    if first.as_ref().is_none_or(|current| created_at < current) {
        *first = Some(created_at.to_string());
    }
    if last.as_ref().is_none_or(|current| created_at > current) {
        *last = Some(created_at.to_string());
    }
}

fn is_step_kind(kind: &str) -> bool {
    let normalized = kind.trim().to_ascii_lowercase();
    normalized == "step"
        || normalized.starts_with("step_")
        || normalized.ends_with("_step")
        || normalized.ends_with("_event")
}

#[derive(Debug, Clone)]
struct ParsedAuxiliaryLogLine {
    timestamp: String,
    level: String,
    subsystem: String,
    event: String,
    payload: String,
    fields: BTreeMap<String, String>,
}

fn parse_auxiliary_log_line(line: &str) -> Option<ParsedAuxiliaryLogLine> {
    let mut tokens = line.split_whitespace();
    let timestamp = tokens.next()?.to_string();
    let level = tokens.next()?.to_string();
    let subsystem = tokens.next()?.to_string();

    let remaining = tokens.map(str::to_string).collect::<Vec<_>>();
    if remaining.is_empty() {
        return None;
    }

    let (event, field_tokens): (String, Vec<String>) = if remaining[0].contains('=') {
        ("entry".to_string(), remaining)
    } else {
        (remaining[0].clone(), remaining[1..].to_vec())
    };

    let mut payload_tokens = vec![event.clone()];
    payload_tokens.extend(field_tokens.iter().cloned());

    let mut fields = BTreeMap::new();
    for token in &field_tokens {
        if let Some((key, value)) = token.split_once('=')
            && !key.is_empty()
            && !value.is_empty()
        {
            fields.insert(key.to_string(), value.to_string());
        }
    }

    Some(ParsedAuxiliaryLogLine {
        timestamp,
        level,
        subsystem,
        event,
        payload: payload_tokens.join(" "),
        fields,
    })
}

fn map_auxiliary_timestamp(
    raw_timestamp: &str,
    line_number: usize,
    warnings: &mut Vec<String>,
) -> (u64, String, TimestampQuality) {
    match normalize_timestamp_exact(raw_timestamp) {
        Ok(normalized) => (
            normalized.timestamp_unix_ms,
            normalized.timestamp_utc(),
            normalized.timestamp_quality,
        ),
        Err(error) => {
            warnings.push(format!(
                "line {line_number}: invalid timestamp `{raw_timestamp}` ({error}); using fallback timestamp"
            ));
            let fallback_unix_ms = line_number as u64;
            (
                fallback_unix_ms,
                format_unix_ms(fallback_unix_ms),
                TimestampQuality::Fallback,
            )
        }
    }
}

fn classify_auxiliary_subsystem(
    level: &str,
    subsystem: &str,
    event: &str,
) -> (RecordFormat, EventType, ActorRole, &'static str) {
    let normalized_subsystem = subsystem.trim().to_ascii_lowercase();
    if normalized_subsystem.contains("prompt_history") {
        return (
            RecordFormat::Message,
            EventType::Prompt,
            ActorRole::User,
            "prompt_history_auxiliary",
        );
    }

    let normalized_event = event.trim().to_ascii_lowercase();
    let event_type = if normalized_event.contains("token_usage") {
        EventType::Metric
    } else {
        match level.trim().to_ascii_uppercase().as_str() {
            "ERROR" | "FATAL" => EventType::Error,
            "WARN" | "WARNING" => EventType::StatusUpdate,
            _ => EventType::DebugLog,
        }
    };

    (
        RecordFormat::Diagnostic,
        event_type,
        ActorRole::Runtime,
        "runtime_diagnostic",
    )
}

fn parse_u64_field(fields: &BTreeMap<String, String>, key: &str) -> Option<u64> {
    fields.get(key)?.parse::<u64>().ok()
}
