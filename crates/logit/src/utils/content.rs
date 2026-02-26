use std::borrow::Cow;

use serde_json::Value;

const PRIORITY_TEXT_KEYS: &[&str] = &[
    "text", "content", "message", "value", "output", "input", "body", "prompt", "parts",
];

const NON_CONTENT_KEYS: &[&str] = &[
    "id",
    "type",
    "role",
    "name",
    "model",
    "provider",
    "timestamp",
    "created_at",
    "updated_at",
    "status",
    "kind",
    "index",
];

pub const DEFAULT_EXCERPT_MAX_CHARS: usize = 280;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedContent {
    pub content_text: Option<String>,
    pub content_excerpt: Option<String>,
}

#[must_use]
pub fn extract_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => non_empty_text(Cow::Borrowed(text)),
        Value::Array(items) => {
            let fragments: Vec<String> = items.iter().filter_map(extract_text).collect();
            join_fragments(&fragments)
        }
        Value::Object(map) => {
            for key in PRIORITY_TEXT_KEYS {
                if let Some(value) = map.get(*key)
                    && let Some(text) = extract_text(value)
                {
                    return Some(text);
                }
            }

            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();

            let fragments: Vec<String> = keys
                .into_iter()
                .filter(|key| !PRIORITY_TEXT_KEYS.contains(key) && !NON_CONTENT_KEYS.contains(key))
                .filter_map(|key| map.get(key))
                .filter_map(extract_text)
                .collect();

            join_fragments(&fragments)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

#[must_use]
pub fn derive_excerpt(text: &str, max_chars: usize) -> Option<String> {
    if max_chars == 0 {
        return None;
    }

    let normalized = normalize_whitespace(text);
    if normalized.is_empty() {
        return None;
    }

    let char_count = normalized.chars().count();
    if char_count <= max_chars {
        return Some(normalized);
    }

    let mut excerpt = String::with_capacity(max_chars + 3);
    for ch in normalized.chars().take(max_chars) {
        excerpt.push(ch);
    }
    excerpt.push_str("...");
    Some(excerpt)
}

#[must_use]
pub fn extract_text_and_excerpt(value: &Value, excerpt_max_chars: usize) -> ExtractedContent {
    let content_text = extract_text(value);
    let content_excerpt = content_text
        .as_deref()
        .and_then(|text| derive_excerpt(text, excerpt_max_chars));

    ExtractedContent {
        content_text,
        content_excerpt,
    }
}

fn non_empty_text(value: Cow<'_, str>) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn join_fragments(fragments: &[String]) -> Option<String> {
    if fragments.is_empty() {
        return None;
    }

    Some(fragments.join("\n"))
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
