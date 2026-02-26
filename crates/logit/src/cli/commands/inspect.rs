use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Args)]
pub struct InspectArgs {
    #[arg(value_name = "PATH")]
    pub target: PathBuf,

    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectReport {
    pub target_path: String,
    pub file_size_bytes: u64,
    pub classification: String,
    pub line_counts: Option<InspectLineCounts>,
    pub json_document: Option<InspectJsonDocumentSummary>,
    pub normalized_event_summary: Option<InspectNormalizedEventSummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectLineCounts {
    pub total_lines: usize,
    pub non_empty_lines: usize,
    pub json_rows: usize,
    pub invalid_json_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectJsonDocumentSummary {
    pub root_type: String,
    pub object_keys: Vec<String>,
    pub array_length: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectNormalizedEventSummary {
    pub normalized_rows: usize,
    pub adapter_counts: BTreeMap<String, usize>,
    pub event_type_counts: BTreeMap<String, usize>,
}

pub fn inspect_target(path: &Path) -> Result<InspectReport> {
    if !path.exists() {
        bail!("inspect target does not exist: {}", path.display());
    }

    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to stat file: {}", path.display()))?;
    if !metadata.is_file() {
        bail!("inspect target must be a file: {}", path.display());
    }

    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read file: {}", path.display()))?;
    let sample_len = bytes.len().min(4096);
    let classification = crate::discovery::classify_source(path, &bytes[..sample_len]);
    let mut report = InspectReport {
        target_path: path.to_string_lossy().to_string(),
        file_size_bytes: metadata.len(),
        classification: source_classification_key(classification).to_string(),
        line_counts: None,
        json_document: None,
        normalized_event_summary: None,
        warnings: Vec::new(),
    };

    match classification {
        crate::discovery::SourceClassification::Jsonl => {
            analyze_jsonl(&bytes, &mut report)?;
        }
        crate::discovery::SourceClassification::Json => {
            analyze_json_document(&bytes, &mut report)?;
        }
        crate::discovery::SourceClassification::TextLog => {
            analyze_text_log(&bytes, &mut report)?;
        }
        crate::discovery::SourceClassification::Binary => {}
    }

    Ok(report)
}

#[must_use]
pub fn render_text_report(report: &InspectReport) -> String {
    let mut lines = vec![
        format!("target_path: {}", report.target_path),
        format!("file_size_bytes: {}", report.file_size_bytes),
        format!("classification: {}", report.classification),
    ];

    if let Some(line_counts) = &report.line_counts {
        lines.push(format!(
            "line_counts.total_lines: {}",
            line_counts.total_lines
        ));
        lines.push(format!(
            "line_counts.non_empty_lines: {}",
            line_counts.non_empty_lines
        ));
        lines.push(format!("line_counts.json_rows: {}", line_counts.json_rows));
        lines.push(format!(
            "line_counts.invalid_json_rows: {}",
            line_counts.invalid_json_rows
        ));
    }

    if let Some(json_document) = &report.json_document {
        lines.push(format!(
            "json_document.root_type: {}",
            json_document.root_type
        ));
        if let Some(array_length) = json_document.array_length {
            lines.push(format!("json_document.array_length: {array_length}"));
        }
        if !json_document.object_keys.is_empty() {
            lines.push(format!(
                "json_document.object_keys: {}",
                json_document.object_keys.join(",")
            ));
        }
    }

    if let Some(normalized) = &report.normalized_event_summary {
        lines.push(format!(
            "normalized_event_summary.normalized_rows: {}",
            normalized.normalized_rows
        ));
        if !normalized.adapter_counts.is_empty() {
            lines.push(format!(
                "normalized_event_summary.adapter_counts: {}",
                render_count_map(&normalized.adapter_counts)
            ));
        }
        if !normalized.event_type_counts.is_empty() {
            lines.push(format!(
                "normalized_event_summary.event_type_counts: {}",
                render_count_map(&normalized.event_type_counts)
            ));
        }
    }

    if !report.warnings.is_empty() {
        lines.push("warnings:".to_string());
        lines.extend(report.warnings.iter().map(|warning| format!("- {warning}")));
    }

    lines.join("\n")
}

pub fn render_json_report(report: &InspectReport) -> Result<String> {
    serde_json::to_string_pretty(report).context("failed to encode inspect report as JSON")
}

pub fn run(args: &InspectArgs) -> Result<()> {
    let report = inspect_target(args.target.as_path())?;
    if args.json {
        println!("{}", render_json_report(&report)?);
    } else {
        println!("{}", render_text_report(&report));
    }
    Ok(())
}

fn analyze_jsonl(bytes: &[u8], report: &mut InspectReport) -> Result<()> {
    let text =
        std::str::from_utf8(bytes).context("jsonl source could not be decoded as valid UTF-8")?;
    let mut line_counts = InspectLineCounts {
        total_lines: 0,
        non_empty_lines: 0,
        json_rows: 0,
        invalid_json_rows: 0,
    };

    let mut normalized_rows = 0usize;
    let mut adapter_counts = BTreeMap::new();
    let mut event_type_counts = BTreeMap::new();

    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        line_counts.total_lines += 1;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        line_counts.non_empty_lines += 1;

        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => {
                line_counts.json_rows += 1;
                if is_normalized_event_row(&value) {
                    normalized_rows += 1;
                    let adapter = value
                        .get("adapter_name")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    let event_type = value
                        .get("event_type")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string();
                    *adapter_counts.entry(adapter).or_insert(0) += 1;
                    *event_type_counts.entry(event_type).or_insert(0) += 1;
                }
            }
            Err(error) => {
                line_counts.invalid_json_rows += 1;
                report
                    .warnings
                    .push(format!("line {line_number}: invalid JSON ({error})"));
            }
        }
    }

    if normalized_rows > 0 {
        report.normalized_event_summary = Some(InspectNormalizedEventSummary {
            normalized_rows,
            adapter_counts,
            event_type_counts,
        });
    }
    report.line_counts = Some(line_counts);
    Ok(())
}

fn analyze_json_document(bytes: &[u8], report: &mut InspectReport) -> Result<()> {
    let value = serde_json::from_slice::<Value>(bytes).context("json source is not valid JSON")?;
    let json_document = match value {
        Value::Object(map) => {
            let mut object_keys = map.keys().cloned().collect::<Vec<_>>();
            object_keys.sort();
            InspectJsonDocumentSummary {
                root_type: "object".to_string(),
                object_keys,
                array_length: None,
            }
        }
        Value::Array(items) => InspectJsonDocumentSummary {
            root_type: "array".to_string(),
            object_keys: Vec::new(),
            array_length: Some(items.len()),
        },
        Value::Null => InspectJsonDocumentSummary {
            root_type: "null".to_string(),
            object_keys: Vec::new(),
            array_length: None,
        },
        Value::Bool(_) => InspectJsonDocumentSummary {
            root_type: "bool".to_string(),
            object_keys: Vec::new(),
            array_length: None,
        },
        Value::Number(_) => InspectJsonDocumentSummary {
            root_type: "number".to_string(),
            object_keys: Vec::new(),
            array_length: None,
        },
        Value::String(_) => InspectJsonDocumentSummary {
            root_type: "string".to_string(),
            object_keys: Vec::new(),
            array_length: None,
        },
    };

    report.json_document = Some(json_document);
    Ok(())
}

fn analyze_text_log(bytes: &[u8], report: &mut InspectReport) -> Result<()> {
    let text =
        std::str::from_utf8(bytes).context("text source could not be decoded as valid UTF-8")?;
    let total_lines = text.lines().count();
    let non_empty_lines = text.lines().filter(|line| !line.trim().is_empty()).count();
    report.line_counts = Some(InspectLineCounts {
        total_lines,
        non_empty_lines,
        json_rows: 0,
        invalid_json_rows: 0,
    });
    Ok(())
}

fn is_normalized_event_row(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.get("schema_version").and_then(Value::as_str) == Some("agentlog.v1")
            && object
                .get("event_id")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
    })
}

fn render_count_map(counts: &BTreeMap<String, usize>) -> String {
    counts
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

const fn source_classification_key(
    classification: crate::discovery::SourceClassification,
) -> &'static str {
    match classification {
        crate::discovery::SourceClassification::Jsonl => "jsonl",
        crate::discovery::SourceClassification::Json => "json",
        crate::discovery::SourceClassification::TextLog => "text_log",
        crate::discovery::SourceClassification::Binary => "binary",
    }
}
