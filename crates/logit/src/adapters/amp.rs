use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::models::{
    ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat, TimestampQuality,
};
use crate::utils::content::{DEFAULT_EXCERPT_MAX_CHARS, derive_excerpt, extract_text};
use crate::utils::hash::hash64;
use crate::utils::time::{format_unix_ms, normalize_timestamp_exact};

pub const DEFAULT_PATHS: &[&str] = &[
    "~/.amp/sessions",
    "~/.amp/history",
    "~/.amp/logs",
    "~/.amp/file-changes",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmpThreadMetadata {
    pub thread_id: String,
    pub session_id: Option<String>,
    pub message_count: usize,
    pub first_created_at: Option<String>,
    pub last_created_at: Option<String>,
    pub roles_seen: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmpContentPart {
    pub path: String,
    pub kind: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmpMessageMetadata {
    pub message_id: String,
    pub role: String,
    pub created_at: Option<String>,
    pub part_count: usize,
    pub part_kinds: Vec<String>,
    pub content_text: Option<String>,
    pub content_excerpt: Option<String>,
    pub content_parts: Vec<AmpContentPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmpThreadParseResult {
    pub thread: AmpThreadMetadata,
    pub messages: Vec<AmpMessageMetadata>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmpAuxiliaryRecord {
    pub record_id: String,
    pub record_kind: String,
    pub thread_id: Option<String>,
    pub session_id: Option<String>,
    pub role: Option<String>,
    pub created_at: Option<String>,
    pub content_text: Option<String>,
    pub content_excerpt: Option<String>,
    pub metadata_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmpAuxiliaryParseResult {
    pub records: Vec<AmpAuxiliaryRecord>,
    pub record_kinds: Vec<String>,
    pub skipped_message_duplicates: usize,
    pub warnings: Vec<String>,
}

pub const DEFAULT_CHANGE_BLOB_LIMIT_BYTES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmpAttachmentTelemetry {
    pub attachment_id: String,
    pub size_bytes: Option<u64>,
    pub status: String,
    pub at_or_over_limit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmpFileChangeTelemetry {
    pub path: String,
    pub operation: String,
    pub tool_name: Option<String>,
    pub before_preview: Option<String>,
    pub after_preview: Option<String>,
    pub before_size_bytes: Option<usize>,
    pub after_size_bytes: Option<usize>,
    pub before_truncated: bool,
    pub after_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmpFileChangeParseResult {
    pub thread_id: Option<String>,
    pub blob_limit_bytes: usize,
    pub attachments: Vec<AmpAttachmentTelemetry>,
    pub file_changes: Vec<AmpFileChangeTelemetry>,
    pub paths_seen: Vec<String>,
    pub tools_seen: Vec<String>,
    pub over_limit_attachments: usize,
    pub truncated_blobs: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AmpFileChangeEventParseResult {
    pub events: Vec<AgentLogEvent>,
    pub warnings: Vec<String>,
}

pub fn parse_thread_envelope(raw_json: &str) -> Result<AmpThreadParseResult> {
    let parsed: Value = serde_json::from_str(raw_json).context("invalid amp envelope JSON")?;
    parse_thread_envelope_value(&parsed)
}

pub fn parse_auxiliary_history_session_file(path: &Path) -> Result<AmpAuxiliaryParseResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read amp auxiliary history/session file: {path:?}"))?;
    Ok(parse_auxiliary_history_session_jsonl(&content))
}

#[must_use]
pub fn parse_auxiliary_history_session_jsonl(input: &str) -> AmpAuxiliaryParseResult {
    let mut records = Vec::new();
    let mut record_kinds = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut skipped_message_duplicates = 0_usize;

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

        if looks_like_thread_message_duplicate(object) {
            skipped_message_duplicates += 1;
            warnings.push(format!(
                "line {line_number}: skipped likely duplicate thread message payload in auxiliary stream"
            ));
            continue;
        }

        let record_id = optional_trimmed_string_any(object, &["event_id", "history_id", "id"])
            .unwrap_or_else(|| format!("amp-aux-line-{line_number:06}"));
        let record_kind = optional_trimmed_string_any(object, &["kind", "type", "event"])
            .unwrap_or_else(|| "auxiliary".to_string());
        record_kinds.insert(record_kind.clone());

        let thread_id = optional_trimmed_string_any(object, &["thread_id", "thread"]);
        let session_id = optional_trimmed_string_any(object, &["session_id", "session"]);
        let role = optional_trimmed_string_any(object, &["role"]);
        let created_at = optional_trimmed_string_any(object, &["created_at", "timestamp", "ts"]);

        let content_text = extract_auxiliary_content_text(object);
        if content_text.is_none() {
            warnings.push(format!(
                "line {line_number}: missing auxiliary content text fields"
            ));
        }
        let content_excerpt = content_text
            .as_deref()
            .and_then(|text| derive_excerpt(text, DEFAULT_EXCERPT_MAX_CHARS));

        let mut metadata_keys: Vec<String> = object
            .keys()
            .filter(|key| {
                !matches!(
                    key.as_str(),
                    "event_id"
                        | "history_id"
                        | "id"
                        | "kind"
                        | "type"
                        | "event"
                        | "thread_id"
                        | "thread"
                        | "session_id"
                        | "session"
                        | "role"
                        | "created_at"
                        | "timestamp"
                        | "ts"
                        | "summary"
                        | "text"
                        | "message"
                        | "note"
                        | "description"
                        | "prompt"
                )
            })
            .cloned()
            .collect();
        metadata_keys.sort();

        records.push(AmpAuxiliaryRecord {
            record_id,
            record_kind,
            thread_id,
            session_id,
            role,
            created_at,
            content_text,
            content_excerpt,
            metadata_keys,
        });
    }

    AmpAuxiliaryParseResult {
        records,
        record_kinds: record_kinds.into_iter().collect(),
        skipped_message_duplicates,
        warnings,
    }
}

pub fn parse_thread_envelope_value(parsed: &Value) -> Result<AmpThreadParseResult> {
    let object = parsed
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("amp envelope root must be a JSON object"))?;
    let thread_id = required_non_empty_string(object, "thread_id")?;
    let session_id = optional_non_empty_string(object.get("session_id"), "session_id")?;
    let messages = object
        .get("messages")
        .ok_or_else(|| anyhow::anyhow!("amp envelope must contain `messages`"))?
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("amp envelope `messages` must be an array"))?;

    let mut parsed_messages = Vec::with_capacity(messages.len());
    let mut warnings = Vec::new();
    let mut roles_seen = BTreeSet::new();
    let mut first_created_at: Option<String> = None;
    let mut last_created_at: Option<String> = None;

    for (index, message) in messages.iter().enumerate() {
        if let Some(parsed_message) = parse_message(index, message, &mut warnings)? {
            if let Some(created_at) = &parsed_message.created_at {
                update_created_at_bounds(created_at, &mut first_created_at, &mut last_created_at);
            }
            roles_seen.insert(parsed_message.role.clone());
            parsed_messages.push(parsed_message);
        }
    }

    if parsed_messages.is_empty() {
        warnings.push("no parseable amp messages found in envelope".to_string());
    }

    Ok(AmpThreadParseResult {
        thread: AmpThreadMetadata {
            thread_id,
            session_id,
            message_count: parsed_messages.len(),
            first_created_at,
            last_created_at,
            roles_seen: roles_seen.into_iter().collect(),
        },
        messages: parsed_messages,
        warnings,
    })
}

pub fn parse_file_change_artifact(raw_json: &str) -> Result<AmpFileChangeParseResult> {
    let parsed: Value =
        serde_json::from_str(raw_json).context("invalid amp file-change artifact JSON")?;
    parse_file_change_artifact_value(&parsed)
}

pub fn parse_file_change_artifact_value(parsed: &Value) -> Result<AmpFileChangeParseResult> {
    let object = parsed
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("amp file-change artifact root must be a JSON object"))?;

    let mut warnings = Vec::new();
    let thread_id = object.get("thread_id").and_then(optional_trimmed_string);
    let blob_limit_bytes = parse_blob_limit_bytes(object, &mut warnings);
    let attachments =
        parse_attachment_telemetry(object.get("attachments"), blob_limit_bytes, &mut warnings);
    let file_changes = parse_file_change_telemetry(
        object.get("file_changes").or_else(|| object.get("changes")),
        blob_limit_bytes,
        &mut warnings,
    );

    let paths_seen: BTreeSet<String> = file_changes
        .iter()
        .map(|change| change.path.clone())
        .collect();
    let tools_seen: BTreeSet<String> = file_changes
        .iter()
        .filter_map(|change| change.tool_name.clone())
        .collect();
    let over_limit_attachments = attachments
        .iter()
        .filter(|attachment| attachment.at_or_over_limit)
        .count();
    let truncated_blobs = file_changes
        .iter()
        .filter(|change| change.before_truncated || change.after_truncated)
        .count();

    Ok(AmpFileChangeParseResult {
        thread_id,
        blob_limit_bytes,
        attachments,
        file_changes,
        paths_seen: paths_seen.into_iter().collect(),
        tools_seen: tools_seen.into_iter().collect(),
        over_limit_attachments,
        truncated_blobs,
        warnings,
    })
}

pub fn parse_file_change_event_file(
    path: &Path,
    run_id: &str,
) -> Result<AmpFileChangeEventParseResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read amp file-change event file: {path:?}"))?;
    parse_file_change_event_json(&content, run_id, path.to_string_lossy().as_ref())
}

pub fn parse_file_change_event_json(
    input: &str,
    run_id: &str,
    source_path: &str,
) -> Result<AmpFileChangeEventParseResult> {
    let parsed =
        serde_json::from_str::<Value>(input).context("invalid amp file-change event JSON")?;

    let mut events = Vec::new();
    let mut warnings = Vec::new();
    match parsed {
        Value::Object(object) => {
            let event =
                map_file_change_event(&object, run_id, source_path, "json:root", 0, &mut warnings);
            events.push(event);
        }
        Value::Array(items) => {
            for (index, entry) in items.iter().enumerate() {
                let Some(object) = entry.as_object() else {
                    warnings.push(format!(
                        "item {}: expected object in amp file-change array; skipped",
                        index + 1
                    ));
                    continue;
                };
                let locator = format!("items:{}", index + 1);
                let event = map_file_change_event(
                    object,
                    run_id,
                    source_path,
                    &locator,
                    index as u64,
                    &mut warnings,
                );
                events.push(event);
            }
        }
        _ => {
            bail!("amp file-change event root must be an object or array of objects");
        }
    }

    Ok(AmpFileChangeEventParseResult { events, warnings })
}

fn parse_message(
    index: usize,
    message: &Value,
    warnings: &mut Vec<String>,
) -> Result<Option<AmpMessageMetadata>> {
    let Some(message_object) = message.as_object() else {
        warnings.push(format!("message[{index}] is not a JSON object"));
        return Ok(None);
    };

    let context = format!("message[{index}]");
    let message_id =
        match tolerant_optional_non_empty_string(message_object, "id", &context, warnings) {
            Some(value) => value,
            None => {
                warnings.push(format!("message[{index}] skipped: missing required `id`"));
                return Ok(None);
            }
        };

    let role = match tolerant_optional_non_empty_string(message_object, "role", &context, warnings)
    {
        Some(value) => value,
        None => {
            warnings.push(format!("message[{index}] skipped: missing required `role`"));
            return Ok(None);
        }
    };

    let created_at =
        tolerant_optional_non_empty_string(message_object, "created_at", &context, warnings);
    let parsed_parts = parse_parts(index, message_object.get("parts"), warnings)?;

    Ok(Some(AmpMessageMetadata {
        message_id,
        role,
        created_at,
        part_count: parsed_parts.part_count,
        part_kinds: parsed_parts.part_kinds,
        content_text: parsed_parts.content_text,
        content_excerpt: parsed_parts.content_excerpt,
        content_parts: parsed_parts.content_parts,
    }))
}

fn map_file_change_event(
    object: &Map<String, Value>,
    run_id: &str,
    source_path: &str,
    source_record_locator: &str,
    sequence_source: u64,
    warnings: &mut Vec<String>,
) -> AgentLogEvent {
    let (path_thread_hint, path_session_hint) = infer_session_context_from_path(source_path);
    let thread_id = optional_scalar_string_any(
        object,
        &[
            "thread_id",
            "threadId",
            "thread",
            "conversation_id",
            "conversationId",
        ],
    )
    .or(path_thread_hint);
    let session_id = optional_scalar_string_any(object, &["session_id", "sessionId", "session"])
        .or(path_session_hint)
        .or_else(|| thread_id.clone());
    let uri = optional_scalar_string_any(object, &["uri", "path", "file", "file_path"]);
    let source_path_hash = hash64(&source_path.to_string());
    let event_id = optional_scalar_string_any(object, &["event_id", "id", "change_id", "changeId"])
        .unwrap_or_else(|| format!("amp-file-change-{source_path_hash:016x}-{sequence_source:06}"));

    let diff_text = object.get("diff").and_then(extract_text);
    let before_text = object.get("before").and_then(extract_text);
    let after_text = object.get("after").and_then(extract_text);
    let content_text =
        diff_text
            .clone()
            .or_else(|| match (before_text.clone(), after_text.clone()) {
                (Some(before), Some(after)) => {
                    Some(format!("before:\n{before}\n\nafter:\n{after}"))
                }
                (Some(before), None) => Some(format!("before:\n{before}")),
                (None, Some(after)) => Some(format!("after:\n{after}")),
                (None, None) => None,
            });
    if content_text.is_none() {
        warnings.push(format!(
            "{source_record_locator}: amp file-change record has no diff/before/after payload"
        ));
    }
    let content_excerpt = content_text
        .as_deref()
        .and_then(|text| derive_excerpt(text, DEFAULT_EXCERPT_MAX_CHARS));

    let (timestamp_unix_ms, timestamp_utc, timestamp_quality) =
        map_event_timestamp(object, source_record_locator, sequence_source + 1, warnings);

    let raw_hash = format!(
        "{:016x}",
        hash64(&Value::Object(object.clone()).to_string())
    );
    let canonical_hash = format!(
        "{:016x}",
        hash64(&(
            "amp.file_change.v1",
            event_id.clone(),
            source_path,
            source_record_locator,
            session_id.clone(),
            thread_id.clone(),
            uri.clone(),
            content_text.clone(),
            timestamp_unix_ms
        ))
    );

    let mut metadata = BTreeMap::new();
    metadata.insert(
        "source_record".to_string(),
        serde_json::json!(source_record_locator),
    );
    if let Some(uri) = &uri {
        metadata.insert("uri".to_string(), serde_json::json!(uri));
    }
    if let Some(is_new_file) = optional_bool_any(object, &["isNewFile", "is_new_file"]) {
        metadata.insert("is_new_file".to_string(), serde_json::json!(is_new_file));
    }
    if let Some(reverted) = optional_bool_any(object, &["reverted"]) {
        metadata.insert("reverted".to_string(), serde_json::json!(reverted));
    }
    if let Some(thread_id) = &thread_id {
        metadata.insert("thread_id".to_string(), serde_json::json!(thread_id));
    }
    if let Some(session_id) = &session_id {
        metadata.insert("session_id".to_string(), serde_json::json!(session_id));
    }

    AgentLogEvent {
        schema_version: crate::models::SchemaVersion::AgentLogV1,
        event_id,
        run_id: run_id.to_string(),
        sequence_global: 0,
        sequence_source: Some(sequence_source),
        source_kind: AgentSource::Amp,
        source_path: source_path.to_string(),
        source_record_locator: source_record_locator.to_string(),
        source_record_hash: None,
        adapter_name: AgentSource::Amp,
        adapter_version: Some("v1".to_string()),
        record_format: RecordFormat::ToolResult,
        event_type: EventType::ToolOutput,
        role: ActorRole::Tool,
        timestamp_utc,
        timestamp_unix_ms,
        timestamp_quality,
        session_id,
        conversation_id: thread_id,
        turn_id: None,
        parent_event_id: None,
        actor_id: None,
        actor_name: None,
        provider: None,
        model: None,
        content_text: content_text.clone(),
        content_excerpt,
        content_mime: Some(if diff_text.is_some() {
            "text/x-diff".to_string()
        } else {
            "text/plain".to_string()
        }),
        tool_name: optional_scalar_string_any(object, &["tool_name", "tool", "source_tool"]),
        tool_call_id: optional_scalar_string_any(
            object,
            &["tool_call_id", "toolCallId", "call_id", "callId"],
        ),
        tool_arguments_json: object
            .get("arguments")
            .or_else(|| object.get("input"))
            .and_then(|value| serde_json::to_string(value).ok()),
        tool_result_text: content_text,
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        cost_usd: None,
        tags: vec![
            "amp".to_string(),
            "file_change".to_string(),
            "tool_result".to_string(),
        ],
        flags: vec!["auxiliary".to_string()],
        pii_redacted: None,
        warnings: Vec::new(),
        errors: Vec::new(),
        raw_hash,
        canonical_hash,
        metadata,
    }
}

#[derive(Debug, Default)]
struct ParsedParts {
    part_count: usize,
    part_kinds: Vec<String>,
    content_text: Option<String>,
    content_excerpt: Option<String>,
    content_parts: Vec<AmpContentPart>,
}

fn parse_parts(
    index: usize,
    parts_value: Option<&Value>,
    warnings: &mut Vec<String>,
) -> Result<ParsedParts> {
    let Some(parts_value) = parts_value else {
        return Ok(ParsedParts::default());
    };

    let Some(parts_array) = parts_value.as_array() else {
        warnings.push(format!("message[{index}] `parts` is not an array"));
        return Ok(ParsedParts::default());
    };

    let mut content_parts = Vec::new();
    collect_typed_parts(index, parts_array, "", warnings, &mut content_parts)?;

    let mut part_kinds: Vec<String> = content_parts.iter().map(|part| part.kind.clone()).collect();
    if part_kinds.is_empty() && !parts_array.is_empty() {
        part_kinds.push("unknown".to_string());
    }

    let text_fragments: Vec<String> = content_parts
        .iter()
        .filter_map(|part| part.text.clone())
        .collect();
    let content_text = if text_fragments.is_empty() {
        None
    } else {
        Some(text_fragments.join("\n"))
    };
    let content_excerpt = content_text
        .as_deref()
        .and_then(|text| derive_excerpt(text, DEFAULT_EXCERPT_MAX_CHARS));

    Ok(ParsedParts {
        part_count: parts_array.len(),
        part_kinds,
        content_text,
        content_excerpt,
        content_parts,
    })
}

fn collect_typed_parts(
    message_index: usize,
    parts: &[Value],
    path_prefix: &str,
    warnings: &mut Vec<String>,
    out: &mut Vec<AmpContentPart>,
) -> Result<()> {
    for (part_index, part_value) in parts.iter().enumerate() {
        let path = if path_prefix.is_empty() {
            part_index.to_string()
        } else {
            format!("{path_prefix}.{part_index}")
        };

        let Some(part_object) = part_value.as_object() else {
            warnings.push(format!(
                "message[{message_index}] part[{path}] is not an object"
            ));
            continue;
        };

        let context = format!("message[{message_index}] part[{path}]");
        let kind = match tolerant_optional_non_empty_string(part_object, "type", &context, warnings)
        {
            Some(kind) => kind,
            None => {
                warnings.push(format!(
                    "message[{message_index}] part[{path}] missing `type`; defaulting to `unknown`"
                ));
                "unknown".to_string()
            }
        };

        out.push(AmpContentPart {
            path: path.clone(),
            kind,
            text: extract_part_text(part_object),
        });

        collect_nested_typed_parts(message_index, &path, part_object, warnings, out)?;
    }
    Ok(())
}

fn collect_nested_typed_parts(
    message_index: usize,
    path: &str,
    part_object: &Map<String, Value>,
    warnings: &mut Vec<String>,
    out: &mut Vec<AmpContentPart>,
) -> Result<()> {
    for key in ["parts", "content"] {
        let Some(value) = part_object.get(key) else {
            continue;
        };

        if value.is_null() {
            continue;
        }

        let Some(array) = value.as_array() else {
            if key == "parts" {
                warnings.push(format!(
                    "message[{message_index}] part[{path}] `{key}` is not an array"
                ));
            }
            continue;
        };

        if is_typed_part_array(array) {
            collect_typed_parts(message_index, array, path, warnings, out)?;
        }
    }
    Ok(())
}

fn is_typed_part_array(values: &[Value]) -> bool {
    values.iter().any(|value| {
        value
            .as_object()
            .is_some_and(|object| object.contains_key("type"))
    })
}

fn extract_part_text(part_object: &Map<String, Value>) -> Option<String> {
    for key in [
        "text", "message", "value", "output", "input", "body", "prompt",
    ] {
        if let Some(text) = part_object.get(key).and_then(extract_text) {
            return Some(text);
        }
    }

    if let Some(content_value) = part_object.get("content") {
        if let Some(array) = content_value.as_array()
            && is_typed_part_array(array)
        {
            return None;
        }
        return match content_value {
            Value::String(_) | Value::Array(_) => extract_text(content_value),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => None,
        };
    }

    None
}

fn parse_blob_limit_bytes(object: &Map<String, Value>, warnings: &mut Vec<String>) -> usize {
    let Some(value) = object.get("blob_limit_bytes") else {
        return DEFAULT_CHANGE_BLOB_LIMIT_BYTES;
    };

    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_else(|| {
                warnings.push(
                    "invalid `blob_limit_bytes`; using default blob truncation limit".to_string(),
                );
                DEFAULT_CHANGE_BLOB_LIMIT_BYTES
            }),
        Value::String(text) => match text.trim().parse::<usize>() {
            Ok(value) => value,
            Err(_) => {
                warnings.push(
                    "invalid `blob_limit_bytes`; using default blob truncation limit".to_string(),
                );
                DEFAULT_CHANGE_BLOB_LIMIT_BYTES
            }
        },
        _ => {
            warnings.push(
                "invalid `blob_limit_bytes`; using default blob truncation limit".to_string(),
            );
            DEFAULT_CHANGE_BLOB_LIMIT_BYTES
        }
    }
}

fn parse_attachment_telemetry(
    attachments_value: Option<&Value>,
    blob_limit_bytes: usize,
    warnings: &mut Vec<String>,
) -> Vec<AmpAttachmentTelemetry> {
    let Some(attachments_value) = attachments_value else {
        return Vec::new();
    };

    let Some(items) = attachments_value.as_array() else {
        warnings.push("`attachments` is not an array".to_string());
        return Vec::new();
    };

    let mut attachments = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let Some(object) = item.as_object() else {
            warnings.push(format!("attachment[{index}] is not an object"));
            continue;
        };

        let attachment_id = optional_trimmed_string_any(object, &["attachment_id", "id"])
            .unwrap_or_else(|| format!("attachment-{index:03}"));
        if attachment_id.starts_with("attachment-") {
            warnings.push(format!(
                "attachment[{index}] missing `attachment_id`; generated fallback id"
            ));
        }

        let size_bytes = optional_u64_any(object, &["size_bytes", "size", "bytes"]);
        let status = optional_trimmed_string_any(object, &["status"])
            .unwrap_or_else(|| "unknown".to_string());
        let at_or_over_limit = size_bytes.is_some_and(|size| size >= blob_limit_bytes as u64)
            || matches!(status.as_str(), "at_limit" | "over_limit" | "too_large");

        attachments.push(AmpAttachmentTelemetry {
            attachment_id,
            size_bytes,
            status,
            at_or_over_limit,
        });
    }

    attachments
}

fn parse_file_change_telemetry(
    file_changes_value: Option<&Value>,
    blob_limit_bytes: usize,
    warnings: &mut Vec<String>,
) -> Vec<AmpFileChangeTelemetry> {
    let Some(file_changes_value) = file_changes_value else {
        return Vec::new();
    };

    let Some(items) = file_changes_value.as_array() else {
        warnings.push("`file_changes`/`changes` is not an array".to_string());
        return Vec::new();
    };

    let mut changes = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let Some(object) = item.as_object() else {
            warnings.push(format!("file_change[{index}] is not an object"));
            continue;
        };

        let path = optional_trimmed_string_any(object, &["path", "file_path", "file", "filename"])
            .unwrap_or_else(|| {
                warnings.push(format!(
                    "file_change[{index}] missing path field; defaulting to `unknown`"
                ));
                "unknown".to_string()
            });
        let operation = optional_trimmed_string_any(object, &["operation", "op", "action", "type"])
            .unwrap_or_else(|| "unknown".to_string());
        let tool_name = optional_trimmed_string_any(object, &["tool_name", "tool", "source_tool"]);

        let before_blob = object
            .get("before")
            .or_else(|| object.get("old"))
            .or_else(|| object.get("old_content"));
        let after_blob = object
            .get("after")
            .or_else(|| object.get("new"))
            .or_else(|| object.get("new_content"));
        let (before_preview, before_size_bytes, before_truncated) =
            truncate_blob_preview(before_blob, blob_limit_bytes);
        let (after_preview, after_size_bytes, after_truncated) =
            truncate_blob_preview(after_blob, blob_limit_bytes);

        changes.push(AmpFileChangeTelemetry {
            path,
            operation,
            tool_name,
            before_preview,
            after_preview,
            before_size_bytes,
            after_size_bytes,
            before_truncated,
            after_truncated,
        });
    }

    changes
}

fn truncate_blob_preview(
    value: Option<&Value>,
    blob_limit_bytes: usize,
) -> (Option<String>, Option<usize>, bool) {
    let Some(value) = value else {
        return (None, None, false);
    };

    let serialized = match value {
        Value::Null => return (None, None, false),
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    };

    let size_bytes = serialized.len();
    if serialized.is_empty() {
        return (None, Some(size_bytes), false);
    }

    let (preview, truncated) = truncate_utf8_to_byte_limit(&serialized, blob_limit_bytes);
    (Some(preview), Some(size_bytes), truncated)
}

fn truncate_utf8_to_byte_limit(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    if max_bytes == 0 {
        return (String::new(), true);
    }
    if max_bytes <= 3 {
        return (".".repeat(max_bytes), true);
    }

    let target_bytes = max_bytes - 3;
    let mut result = String::new();
    for ch in value.chars() {
        if result.len() + ch.len_utf8() > target_bytes {
            break;
        }
        result.push(ch);
    }
    result.push_str("...");
    (result, true)
}

fn optional_trimmed_string_any(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(optional_trimmed_string)
}

fn extract_auxiliary_content_text(object: &Map<String, Value>) -> Option<String> {
    for key in [
        "summary",
        "text",
        "message",
        "note",
        "description",
        "prompt",
    ] {
        if let Some(text) = object.get(key).and_then(extract_text) {
            return Some(text);
        }
    }
    None
}

fn looks_like_thread_message_duplicate(object: &Map<String, Value>) -> bool {
    object.contains_key("message_id")
        && object.contains_key("role")
        && (object.contains_key("parts") || object.contains_key("content"))
}

fn optional_trimmed_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

fn optional_u64_any(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    for key in keys {
        let Some(value) = object.get(*key) else {
            continue;
        };
        match value {
            Value::Number(number) => {
                if let Some(value) = number.as_u64() {
                    return Some(value);
                }
            }
            Value::String(text) => {
                if let Ok(value) = text.trim().parse::<u64>() {
                    return Some(value);
                }
            }
            _ => {}
        }
    }
    None
}

fn optional_bool_any(object: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    for key in keys {
        let Some(value) = object.get(*key) else {
            continue;
        };
        match value {
            Value::Bool(boolean) => return Some(*boolean),
            Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
                "true" => return Some(true),
                "false" => return Some(false),
                _ => {}
            },
            _ => {}
        }
    }
    None
}

fn optional_scalar_string_any(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(scalar_to_string)
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

fn map_event_timestamp(
    object: &Map<String, Value>,
    source_record_locator: &str,
    fallback_seed: u64,
    warnings: &mut Vec<String>,
) -> (u64, String, TimestampQuality) {
    if let Some(raw_timestamp) =
        optional_scalar_string_any(object, &["timestamp", "created_at", "time", "ts"])
    {
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
                    "{source_record_locator}: invalid timestamp `{raw_timestamp}` ({error}); using fallback"
                ));
            }
        }
    } else {
        warnings.push(format!(
            "{source_record_locator}: missing timestamp; using fallback"
        ));
    }

    (
        fallback_seed,
        format_unix_ms(fallback_seed),
        TimestampQuality::Fallback,
    )
}

fn infer_session_context_from_path(source_path: &str) -> (Option<String>, Option<String>) {
    let mut thread_id = None;
    let mut session_id = None;

    for component in Path::new(source_path).components() {
        let Component::Normal(segment) = component else {
            continue;
        };
        let name = segment.to_string_lossy();
        if thread_id.is_none() && name.starts_with("T-") {
            thread_id = Some(name.to_string());
        }
        if session_id.is_none() && name.starts_with("S-") {
            session_id = Some(name.to_string());
        }
    }

    (thread_id, session_id)
}

fn required_non_empty_string(object: &Map<String, Value>, key: &str) -> Result<String> {
    optional_non_empty_string(object.get(key), key)?
        .ok_or_else(|| anyhow::anyhow!("missing required `{key}`"))
}

fn optional_non_empty_string(value: Option<&Value>, key: &str) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };

    if value.is_null() {
        return Ok(None);
    }

    let Some(text) = value.as_str() else {
        bail!("`{key}` must be a string when present");
    };

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(trimmed.to_string()))
}

fn tolerant_optional_non_empty_string(
    object: &Map<String, Value>,
    key: &str,
    context: &str,
    warnings: &mut Vec<String>,
) -> Option<String> {
    match optional_non_empty_string(object.get(key), key) {
        Ok(value) => value,
        Err(_) => {
            warnings.push(format!(
                "{context} `{key}` must be a string when present; ignoring field"
            ));
            None
        }
    }
}

fn update_created_at_bounds(
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
