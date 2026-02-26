use std::collections::BTreeSet;
use std::sync::OnceLock;

use regex::{Captures, Regex};
use serde_json::Value;

pub const REDACTION_TOKEN: &str = "[REDACTED]";
pub const DEFAULT_SNAPSHOT_MAX_CHARS: usize = 240;

struct RegexRedactionMatcher {
    class_name: &'static str,
    regex: fn() -> &'static Regex,
    replacement: for<'a> fn(&Captures<'a>) -> String,
}

struct HeuristicRedactionMatcher {
    class_name: &'static str,
    predicate: fn(&str) -> bool,
    replacement: fn(&str) -> String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRedactionResult {
    pub text: String,
    pub pii_redacted: bool,
    pub truncated: bool,
    pub redaction_classes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsonRedactionResult {
    pub value: Value,
    pub pii_redacted: bool,
    pub truncated: bool,
    pub redaction_classes: Vec<String>,
}

#[must_use]
pub fn redact_secret(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        REDACTION_TOKEN.to_string()
    }
}

#[must_use]
pub fn redact_and_truncate_text(value: &str, max_chars: usize) -> TextRedactionResult {
    let mut classes = BTreeSet::new();
    let mut redacted = value.to_string();

    for matcher in heuristic_redaction_matcher_catalog() {
        if (matcher.predicate)(&redacted) {
            redacted = (matcher.replacement)(&redacted);
            classes.insert(matcher.class_name.to_string());
        }
    }

    for matcher in redaction_matcher_catalog() {
        redacted = apply_replace_all(
            redacted,
            (matcher.regex)(),
            matcher.replacement,
            matcher.class_name,
            &mut classes,
        );
    }

    let pii_redacted = !classes.is_empty();
    let (text, truncated) = truncate_deterministic(&redacted, max_chars);

    TextRedactionResult {
        text,
        pii_redacted,
        truncated,
        redaction_classes: classes.into_iter().collect(),
    }
}

#[must_use]
pub fn redact_and_truncate_json(value: &Value, max_chars: usize) -> JsonRedactionResult {
    let mut classes = BTreeSet::new();
    let mut pii_redacted = false;
    let mut truncated = false;
    let redacted = redact_json_value(
        value,
        max_chars,
        &mut classes,
        &mut pii_redacted,
        &mut truncated,
    );

    JsonRedactionResult {
        value: redacted,
        pii_redacted,
        truncated,
        redaction_classes: classes.into_iter().collect(),
    }
}

fn redact_json_value(
    value: &Value,
    max_chars: usize,
    classes: &mut BTreeSet<String>,
    pii_redacted: &mut bool,
    truncated: &mut bool,
) -> Value {
    match value {
        Value::String(text) => {
            let result = redact_and_truncate_text(text, max_chars);
            if result.pii_redacted {
                *pii_redacted = true;
            }
            if result.truncated {
                *truncated = true;
            }
            for class_name in result.redaction_classes {
                classes.insert(class_name);
            }
            Value::String(result.text)
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|item| redact_json_value(item, max_chars, classes, pii_redacted, truncated))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| {
                    (
                        key.clone(),
                        redact_json_value(item, max_chars, classes, pii_redacted, truncated),
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn apply_replace_all(
    input: String,
    regex: &Regex,
    replacement: impl Fn(&Captures<'_>) -> String,
    class_name: &str,
    classes: &mut BTreeSet<String>,
) -> String {
    let replaced = regex
        .replace_all(&input, |captures: &Captures<'_>| replacement(captures))
        .to_string();
    if replaced != input {
        classes.insert(class_name.to_string());
    }
    replaced
}

#[must_use]
pub fn redaction_catalog_classes() -> &'static [&'static str] {
    static CLASSES: OnceLock<Vec<&'static str>> = OnceLock::new();
    CLASSES
        .get_or_init(|| {
            let mut classes = Vec::new();
            classes.extend(
                heuristic_redaction_matcher_catalog()
                    .iter()
                    .map(|matcher| matcher.class_name),
            );
            classes.extend(
                redaction_matcher_catalog()
                    .iter()
                    .map(|matcher| matcher.class_name),
            );
            classes
        })
        .as_slice()
}

fn heuristic_redaction_matcher_catalog() -> &'static [HeuristicRedactionMatcher] {
    static CATALOG: OnceLock<Vec<HeuristicRedactionMatcher>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        vec![HeuristicRedactionMatcher {
            class_name: "binary_blob",
            predicate: looks_binary_like_text,
            replacement: replace_binary_blob,
        }]
    })
}

fn looks_binary_like_text(value: &str) -> bool {
    if value.contains('\0') {
        return true;
    }

    let mut total = 0_usize;
    let mut control = 0_usize;
    for ch in value.chars() {
        total += 1;
        if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
            control += 1;
        }
    }

    total >= 16 && (control * 5) >= total
}

fn replace_binary_blob(_value: &str) -> String {
    REDACTION_TOKEN.to_string()
}

fn truncate_deterministic(value: &str, max_chars: usize) -> (String, bool) {
    let total_chars = value.chars().count();
    if total_chars <= max_chars {
        return (value.to_string(), false);
    }

    if max_chars == 0 {
        return (String::new(), true);
    }

    if max_chars <= 3 {
        return (".".repeat(max_chars), true);
    }

    let prefix = value.chars().take(max_chars - 3).collect::<String>();
    (format!("{prefix}..."), true)
}

fn private_key_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
        )
        .expect("private key regex should compile")
    })
}

fn bearer_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._=\-]{8,}")
            .expect("bearer token regex should compile")
    })
}

fn api_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\b(?:sk-[A-Za-z0-9]{8,}|ghp_[A-Za-z0-9]{8,}|xox[baprs]-[A-Za-z0-9\-]{8,})\b")
            .expect("api token regex should compile")
    })
}

fn secret_assignment_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"(?i)\b(password|passwd|secret|api[_\-]?key|token)\b(\s*[:=]\s*)([^\s,;"']+)"#)
            .expect("secret assignment regex should compile")
    })
}

fn url_query_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)([?&](?:access_token|token|api_key)=)([^&\s]+)")
            .expect("url query token regex should compile")
    })
}

fn email_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b")
            .expect("email regex should compile")
    })
}

fn phone_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\b\+?\d[\d\-\(\) ]{8,}\d\b").expect("phone regex should compile")
    })
}

fn replace_with_redaction(_captures: &Captures<'_>) -> String {
    REDACTION_TOKEN.to_string()
}

fn replace_bearer_token(_captures: &Captures<'_>) -> String {
    format!("Bearer {REDACTION_TOKEN}")
}

fn replace_secret_assignment(captures: &Captures<'_>) -> String {
    format!("{}{}{}", &captures[1], &captures[2], REDACTION_TOKEN)
}

fn replace_url_query_token(captures: &Captures<'_>) -> String {
    format!("{}{}", &captures[1], REDACTION_TOKEN)
}

fn redaction_matcher_catalog() -> &'static [RegexRedactionMatcher] {
    static CATALOG: OnceLock<Vec<RegexRedactionMatcher>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        vec![
            RegexRedactionMatcher {
                class_name: "private_key_pem",
                regex: private_key_regex,
                replacement: replace_with_redaction,
            },
            RegexRedactionMatcher {
                class_name: "bearer_token",
                regex: bearer_token_regex,
                replacement: replace_bearer_token,
            },
            RegexRedactionMatcher {
                class_name: "api_token",
                regex: api_token_regex,
                replacement: replace_with_redaction,
            },
            RegexRedactionMatcher {
                class_name: "secret_assignment",
                regex: secret_assignment_regex,
                replacement: replace_secret_assignment,
            },
            RegexRedactionMatcher {
                class_name: "url_query_token",
                regex: url_query_token_regex,
                replacement: replace_url_query_token,
            },
            RegexRedactionMatcher {
                class_name: "email",
                regex: email_regex,
                replacement: replace_with_redaction,
            },
            RegexRedactionMatcher {
                class_name: "phone",
                regex: phone_regex,
                replacement: replace_with_redaction,
            },
        ]
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        REDACTION_TOKEN, redact_and_truncate_json, redact_and_truncate_text, redact_secret,
        redaction_catalog_classes,
    };

    #[test]
    fn redacts_non_empty_values() {
        assert_eq!(redact_secret("sk-live-abc"), REDACTION_TOKEN);
    }

    #[test]
    fn keeps_empty_values_empty() {
        assert_eq!(redact_secret(""), "");
    }

    #[test]
    fn redacts_sensitive_patterns_and_tracks_classes() {
        let assignment_key = ["pass", "word"].concat();
        let assignment_value = ["demo", "value"].concat();
        let bearer_value = ["demo", "token", "99"].concat();
        let input = format!(
            "email=alpha@example.com {assignment_key} = {assignment_value} Bearer {bearer_value}"
        );
        let result = redact_and_truncate_text(&input, 400);

        assert!(result.pii_redacted);
        assert!(result.text.contains(REDACTION_TOKEN));
        assert!(!result.text.contains("alpha@example.com"));
        assert!(!result.text.contains(&assignment_value));
        assert!(result.redaction_classes.contains(&"email".to_string()));
        assert!(
            result
                .redaction_classes
                .contains(&"secret_assignment".to_string())
        );
        assert!(
            result
                .redaction_classes
                .contains(&"bearer_token".to_string())
        );
        assert!(!result.truncated);
    }

    #[test]
    fn truncates_deterministically_after_redaction() {
        let secret_key = ["sec", "ret"].concat();
        let secret_value = ["abcdefghijklmnopqrstuvwxyz", "0123456789"].concat();
        let input = format!("{secret_key}={secret_value}");
        let result = redact_and_truncate_text(&input, 12);

        assert!(result.truncated);
        assert_eq!(result.text.len(), 12);
        assert!(result.text.ends_with("..."));
    }

    #[test]
    fn redacts_nested_json_strings() {
        let input = json!({
            "level": "info",
            "payload": {
                "email": "someone@example.com",
                "note": "call me at +1 (555) 123-4567"
            }
        });
        let result = redact_and_truncate_json(&input, 200);
        assert!(result.pii_redacted);
        assert!(result.redaction_classes.contains(&"email".to_string()));
        assert!(result.redaction_classes.contains(&"phone".to_string()));
        assert_eq!(
            result
                .value
                .pointer("/payload/email")
                .and_then(|v| v.as_str()),
            Some(REDACTION_TOKEN)
        );
    }

    #[test]
    fn exposes_catalog_classes_in_deterministic_order() {
        assert_eq!(
            redaction_catalog_classes(),
            &[
                "binary_blob",
                "private_key_pem",
                "bearer_token",
                "api_token",
                "secret_assignment",
                "url_query_token",
                "email",
                "phone"
            ]
        );
    }

    #[test]
    fn applies_catalog_order_for_overlapping_token_patterns() {
        let input = "Authorization: Bearer sk-abcdefghijklmnopqrstuvwxyz";
        let result = redact_and_truncate_text(input, 200);
        assert!(result.text.contains("Bearer [REDACTED]"));
        assert!(
            result
                .redaction_classes
                .contains(&"bearer_token".to_string())
        );
        assert!(!result.redaction_classes.contains(&"api_token".to_string()));
    }

    #[test]
    fn redacts_binary_like_text_via_heuristic_matcher() {
        let input = "\0\u{0001}\u{0002}\u{0003}\u{0004}\u{0005}\u{0006}\u{0007}abcdefghijklmnop";
        let result = redact_and_truncate_text(input, 400);
        assert_eq!(result.text, REDACTION_TOKEN);
        assert!(
            result
                .redaction_classes
                .contains(&"binary_blob".to_string())
        );
        assert!(result.pii_redacted);
        assert!(!result.truncated);
    }
}
