use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use crate::models::AgentSource;
use crate::utils::redaction;

use super::profiler::extract_event_kind;

const MAX_SAMPLE_RECORD_CHARS: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct SampleCandidate {
    pub source_kind: AgentSource,
    pub source_path: String,
    pub source_record_locator: String,
    pub record: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepresentativeSample {
    pub source_kind: AgentSource,
    pub source_path: String,
    pub source_record_locator: String,
    pub sample_rank: usize,
    pub event_kind: Option<String>,
    pub record: Value,
}

#[must_use]
pub fn extract_representative_samples(
    candidates: &[SampleCandidate],
    max_per_source: usize,
) -> Vec<RepresentativeSample> {
    if max_per_source == 0 || candidates.is_empty() {
        return Vec::new();
    }

    let mut grouped = BTreeMap::<(String, String), Vec<SampleCandidate>>::new();
    for candidate in candidates {
        grouped
            .entry((
                source_kind_key(candidate.source_kind).to_string(),
                candidate.source_path.clone(),
            ))
            .or_default()
            .push(candidate.clone());
    }

    let mut extracted = Vec::new();
    for (_, mut source_candidates) in grouped {
        source_candidates.sort_by(compare_candidates);
        let selected_indices = select_representative_indices(&source_candidates, max_per_source);

        for (sample_rank, index) in selected_indices.into_iter().enumerate() {
            let candidate = &source_candidates[index];
            extracted.push(RepresentativeSample {
                source_kind: candidate.source_kind,
                source_path: candidate.source_path.clone(),
                source_record_locator: candidate.source_record_locator.clone(),
                sample_rank,
                event_kind: extract_event_kind(&candidate.record),
                record: candidate.record.clone(),
            });
        }
    }

    extracted
}

#[must_use]
pub fn redact_and_truncate_samples(
    samples: &[RepresentativeSample],
    max_chars: usize,
) -> Vec<RepresentativeSample> {
    samples
        .iter()
        .map(|sample| {
            let redacted = redaction::redact_and_truncate_json(&sample.record, max_chars);
            let mut record = redacted.value;
            let sanitized_for_size =
                enforce_sample_record_size(&mut record, MAX_SAMPLE_RECORD_CHARS);
            if let Value::Object(object) = &mut record {
                if redacted.pii_redacted {
                    object.insert("pii_redacted".to_string(), Value::Bool(true));
                }
                if redacted.truncated || sanitized_for_size {
                    object.insert("snapshot_truncated".to_string(), Value::Bool(true));
                }
                if sanitized_for_size {
                    object.insert("snapshot_sanitized".to_string(), Value::Bool(true));
                }
                if !redacted.redaction_classes.is_empty() {
                    object.insert(
                        "redaction_classes".to_string(),
                        Value::Array(
                            redacted
                                .redaction_classes
                                .into_iter()
                                .map(Value::String)
                                .collect(),
                        ),
                    );
                }
            }

            RepresentativeSample {
                source_kind: sample.source_kind,
                source_path: sample.source_path.clone(),
                source_record_locator: sample.source_record_locator.clone(),
                sample_rank: sample.sample_rank,
                event_kind: sample.event_kind.clone(),
                record,
            }
        })
        .collect()
}

fn enforce_sample_record_size(record: &mut Value, max_chars: usize) -> bool {
    let rendered = canonical_json(record);
    let total_chars = rendered.chars().count();
    if total_chars <= max_chars {
        return false;
    }

    let preview_limit = std::cmp::max(32, max_chars.saturating_sub(64));
    let preview = truncate_text(&rendered, preview_limit);
    *record = serde_json::json!({
        "sanitization_reason": "max_record_chars_exceeded",
        "original_char_count": total_chars,
        "sanitized_preview": preview,
    });
    true
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    if max_chars == 0 {
        return String::new();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let prefix = value.chars().take(max_chars - 3).collect::<String>();
    format!("{prefix}...")
}

fn select_representative_indices(
    candidates: &[SampleCandidate],
    max_per_source: usize,
) -> Vec<usize> {
    if max_per_source == 0 || candidates.is_empty() {
        return Vec::new();
    }

    if candidates.len() <= max_per_source {
        return (0..candidates.len()).collect();
    }

    let mut selected = BTreeSet::new();

    let mut first_by_kind = BTreeMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        if let Some(kind) = extract_event_kind(&candidate.record) {
            first_by_kind.entry(kind).or_insert(index);
        }
    }

    for index in first_by_kind.values() {
        if selected.len() == max_per_source {
            break;
        }
        selected.insert(*index);
    }

    if selected.len() < max_per_source {
        for index in evenly_spaced_indices(candidates.len(), max_per_source) {
            if selected.len() == max_per_source {
                break;
            }
            selected.insert(index);
        }
    }

    if selected.len() < max_per_source {
        for index in 0..candidates.len() {
            if selected.len() == max_per_source {
                break;
            }
            selected.insert(index);
        }
    }

    selected.into_iter().collect()
}

fn evenly_spaced_indices(total: usize, count: usize) -> Vec<usize> {
    if total == 0 || count == 0 {
        return Vec::new();
    }

    if count == 1 {
        return vec![0];
    }

    let mut indices = Vec::with_capacity(count);
    let span = total - 1;
    let slots = count - 1;

    for step in 0..count {
        let numerator = step * span;
        let index = (numerator + (slots / 2)) / slots;
        indices.push(index);
    }

    indices
}

fn compare_candidates(left: &SampleCandidate, right: &SampleCandidate) -> std::cmp::Ordering {
    left.source_record_locator
        .cmp(&right.source_record_locator)
        .then_with(|| source_kind_key(left.source_kind).cmp(source_kind_key(right.source_kind)))
        .then_with(|| left.source_path.cmp(&right.source_path))
        .then_with(|| canonical_json(&left.record).cmp(&canonical_json(&right.record)))
}

fn source_kind_key(source_kind: AgentSource) -> &'static str {
    match source_kind {
        AgentSource::Codex => "codex",
        AgentSource::Claude => "claude",
        AgentSource::Gemini => "gemini",
        AgentSource::Amp => "amp",
        AgentSource::OpenCode => "opencode",
    }
}

fn canonical_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}
