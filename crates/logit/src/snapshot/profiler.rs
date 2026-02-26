use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyStats {
    pub occurrences: usize,
    pub value_types: BTreeMap<String, usize>,
}

impl KeyStats {
    fn observe_type(&mut self, value_type: &str) {
        self.occurrences += 1;
        *self.value_types.entry(value_type.to_string()).or_insert(0) += 1;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProfile {
    pub total_records: usize,
    pub key_stats: BTreeMap<String, KeyStats>,
    pub event_kind_frequency: BTreeMap<String, usize>,
}

impl SourceProfile {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            total_records: 0,
            key_stats: BTreeMap::new(),
            event_kind_frequency: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProfileResult {
    pub profile: SourceProfile,
    pub warnings: Vec<String>,
}

#[must_use]
pub fn profile_json_records(records: &[Value]) -> SourceProfile {
    let mut profile = SourceProfile::empty();
    profile.total_records = records.len();

    for record in records {
        if let Some(kind) = extract_event_kind(record) {
            *profile.event_kind_frequency.entry(kind).or_insert(0) += 1;
        }

        observe_value("", record, &mut profile);
    }

    profile
}

#[must_use]
pub fn profile_jsonl(input: &str) -> SourceProfileResult {
    let mut warnings = Vec::new();
    let mut records = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => records.push(value),
            Err(error) => warnings.push(format!("line {line_number}: invalid JSON ({error})")),
        }
    }

    SourceProfileResult {
        profile: profile_json_records(&records),
        warnings,
    }
}

fn observe_value(path: &str, value: &Value, profile: &mut SourceProfile) {
    if !path.is_empty() {
        let value_type = value_type(value);
        profile
            .key_stats
            .entry(path.to_string())
            .or_insert_with(|| KeyStats {
                occurrences: 0,
                value_types: BTreeMap::new(),
            })
            .observe_type(value_type);
    }

    match value {
        Value::Object(map) => {
            let mut keys = map.keys().map(String::as_str).collect::<Vec<_>>();
            keys.sort_unstable();

            for key in keys {
                if let Some(child) = map.get(key) {
                    let child_path = if path.is_empty() {
                        key.to_string()
                    } else {
                        format!("{path}.{key}")
                    };
                    observe_value(&child_path, child, profile);
                }
            }
        }
        Value::Array(items) => {
            let array_item_path = if path.is_empty() {
                "[]".to_string()
            } else {
                format!("{path}[]")
            };
            for item in items {
                observe_value(&array_item_path, item, profile);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[must_use]
pub fn extract_event_kind(value: &Value) -> Option<String> {
    let object = value.as_object()?;

    for key in ["event_type", "kind", "type"] {
        let Some(raw) = object.get(key).and_then(Value::as_str) else {
            continue;
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    None
}
