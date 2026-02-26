use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationMode {
    Baseline,
    Strict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationIssueKind {
    InvalidJson,
    SchemaViolation,
    InvariantViolation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationIssueSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PerAgentValidationStats {
    pub records_validated: usize,
    pub errors: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub line: usize,
    pub kind: ValidationIssueKind,
    pub severity: ValidationIssueSeverity,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationQualityScorecard {
    pub overall_score: u8,
    pub coverage_score: u8,
    pub parse_success_score: u8,
    pub content_completeness_score: u8,
    pub timestamp_quality_score: u8,
    pub weakest_dimensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub schema_version: String,
    pub mode: ValidationMode,
    pub status: ValidationStatus,
    pub interpreted_exit_code: i32,
    pub total_records: usize,
    pub records_validated: usize,
    pub errors: usize,
    pub warnings: usize,
    pub quality_scorecard: ValidationQualityScorecard,
    pub per_agent_summary: BTreeMap<String, PerAgentValidationStats>,
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        if self.errors > 0 { 2 } else { 0 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationArtifactLayout {
    pub report_json: PathBuf,
}

#[must_use]
pub fn build_artifact_layout(out_dir: &Path) -> ValidationArtifactLayout {
    ValidationArtifactLayout {
        report_json: out_dir.join("validate").join("report.json"),
    }
}

pub fn write_report_artifact(path: &Path, report: &ValidationReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("failed to create validate artifact directory")?;
    }

    let encoded =
        serde_json::to_vec_pretty(report).context("failed to encode validation report json")?;
    std::fs::write(path, encoded).context("failed to write validation report artifact")
}

pub fn validate_jsonl_file(path: &Path, mode: ValidationMode) -> Result<ValidationReport> {
    let input = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read input file: {}", path.display()))?;
    Ok(validate_jsonl_against_generated_schema(&input, mode))
}

#[must_use]
pub fn validate_jsonl_against_generated_schema(
    input: &str,
    mode: ValidationMode,
) -> ValidationReport {
    let schema = crate::normalize::build_schema_document();
    let required_fields = required_fields_from_schema(&schema);
    let mut issues = Vec::new();
    let mut total_records = 0usize;
    let mut json_records_parsed = 0usize;
    let mut records_validated = 0usize;
    let mut parsed_records = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        total_records += 1;

        let value = match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => value,
            Err(error) => {
                issues.push(ValidationIssue {
                    line: line_number,
                    kind: ValidationIssueKind::InvalidJson,
                    severity: ValidationIssueSeverity::Error,
                    detail: format!("invalid JSON: {error}"),
                });
                continue;
            }
        };
        json_records_parsed += 1;

        match validate_record_against_schema(&value, &required_fields) {
            Ok(record) => {
                records_validated += 1;
                parsed_records.push((line_number, record));
            }
            Err(detail) => {
                issues.push(ValidationIssue {
                    line: line_number,
                    kind: ValidationIssueKind::SchemaViolation,
                    severity: ValidationIssueSeverity::Error,
                    detail,
                });
            }
        }
    }

    issues.extend(validate_invariants(&parsed_records, mode));

    let errors = issues
        .iter()
        .filter(|issue| issue.severity == ValidationIssueSeverity::Error)
        .count();
    let warnings = issues
        .iter()
        .filter(|issue| issue.severity == ValidationIssueSeverity::Warning)
        .count();
    let status = validation_status(errors, warnings);
    let interpreted_exit_code = exit_code_from_counts(errors, warnings);
    let per_agent_summary = build_per_agent_summary(&parsed_records, &issues);
    let quality_scorecard = build_quality_scorecard(
        total_records,
        json_records_parsed,
        records_validated,
        &parsed_records,
    );

    ValidationReport {
        schema_version: crate::models::SCHEMA_VERSION.to_string(),
        mode,
        status,
        interpreted_exit_code,
        total_records,
        records_validated,
        errors,
        warnings,
        quality_scorecard,
        per_agent_summary,
        issues,
    }
}

fn required_fields_from_schema(schema: &Value) -> BTreeSet<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<BTreeSet<String>>()
        })
        .unwrap_or_default()
}

fn validate_record_against_schema(
    value: &Value,
    required_fields: &BTreeSet<String>,
) -> Result<crate::models::AgentLogEvent, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "record is not a JSON object".to_string())?;

    let missing_fields = required_fields
        .iter()
        .filter(|field| !object.contains_key(field.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    if !missing_fields.is_empty() {
        return Err(format!(
            "missing required fields: {}",
            missing_fields.join(", ")
        ));
    }

    serde_json::from_value::<crate::models::AgentLogEvent>(value.clone())
        .map_err(|error| format!("record does not match agentlog.v1 schema: {error}"))
}

type InvariantCheck =
    fn(&[(usize, crate::models::AgentLogEvent)], ValidationMode) -> Vec<ValidationIssue>;

fn validate_invariants(
    records: &[(usize, crate::models::AgentLogEvent)],
    mode: ValidationMode,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    for check in invariant_catalog() {
        issues.extend(check(records, mode));
    }

    issues.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.detail.cmp(&right.detail))
    });
    issues
}

fn invariant_catalog() -> &'static [InvariantCheck] {
    &[
        invariant_timestamp_consistency,
        invariant_hash_presence,
        invariant_content_presence,
    ]
}

fn invariant_timestamp_consistency(
    records: &[(usize, crate::models::AgentLogEvent)],
    _mode: ValidationMode,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    for (line, record) in records {
        if let Err(detail) = validate_timestamp_consistency(record) {
            issues.push(ValidationIssue {
                line: *line,
                kind: ValidationIssueKind::InvariantViolation,
                severity: ValidationIssueSeverity::Error,
                detail,
            });
        }
    }
    issues
}

fn invariant_hash_presence(
    records: &[(usize, crate::models::AgentLogEvent)],
    _mode: ValidationMode,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    for (line, record) in records {
        if record.raw_hash.trim().is_empty() {
            issues.push(ValidationIssue {
                line: *line,
                kind: ValidationIssueKind::InvariantViolation,
                severity: ValidationIssueSeverity::Error,
                detail: "raw_hash must be non-empty".to_string(),
            });
        }

        if record.canonical_hash.trim().is_empty() {
            issues.push(ValidationIssue {
                line: *line,
                kind: ValidationIssueKind::InvariantViolation,
                severity: ValidationIssueSeverity::Error,
                detail: "canonical_hash must be non-empty".to_string(),
            });
        }
    }
    issues
}

fn invariant_content_presence(
    records: &[(usize, crate::models::AgentLogEvent)],
    mode: ValidationMode,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut message_lines = Vec::new();
    let mut missing_content_lines = Vec::new();

    for (line, record) in records {
        if !requires_content_text(record) {
            continue;
        }

        message_lines.push(*line);
        if record.content_text.as_deref().is_none_or(str::is_empty) {
            missing_content_lines.push(*line);
            issues.push(ValidationIssue {
                line: *line,
                kind: ValidationIssueKind::InvariantViolation,
                severity: missing_content_severity(mode),
                detail: "content_text is empty for user/assistant message record".to_string(),
            });
        }
    }

    if !message_lines.is_empty() {
        let null_rate = (missing_content_lines.len() as f64) / (message_lines.len() as f64);
        let threshold = missing_content_null_rate_threshold(mode);
        if null_rate > threshold {
            let anchor_line = *missing_content_lines.first().unwrap_or(&message_lines[0]);
            issues.push(ValidationIssue {
                line: anchor_line,
                kind: ValidationIssueKind::InvariantViolation,
                severity: missing_content_severity(mode),
                detail: format!(
                    "content_text null-rate {:.2} exceeds {:.2} threshold",
                    null_rate, threshold
                ),
            });
        }
    }

    issues
}

fn validate_timestamp_consistency(record: &crate::models::AgentLogEvent) -> Result<(), String> {
    let parsed = OffsetDateTime::parse(&record.timestamp_utc, &Rfc3339)
        .map_err(|error| format!("timestamp_utc is not RFC3339: {error}"))?;
    let parsed_millis = parsed.unix_timestamp_nanos() / 1_000_000;
    if parsed_millis < 0 {
        return Err("timestamp_utc resolves to negative epoch milliseconds".to_string());
    }

    let parsed_unix_ms = u64::try_from(parsed_millis)
        .map_err(|_| "timestamp_utc cannot be represented as u64 epoch milliseconds".to_string())?;

    if parsed_unix_ms != record.timestamp_unix_ms {
        return Err(format!(
            "timestamp mismatch: timestamp_utc={}, timestamp_unix_ms={}",
            parsed_unix_ms, record.timestamp_unix_ms
        ));
    }

    Ok(())
}

fn requires_content_text(record: &crate::models::AgentLogEvent) -> bool {
    if record.record_format != crate::models::RecordFormat::Message {
        return false;
    }

    matches!(
        record.role,
        crate::models::ActorRole::User | crate::models::ActorRole::Assistant
    )
}

fn missing_content_severity(mode: ValidationMode) -> ValidationIssueSeverity {
    match mode {
        ValidationMode::Baseline => ValidationIssueSeverity::Warning,
        ValidationMode::Strict => ValidationIssueSeverity::Error,
    }
}

fn missing_content_null_rate_threshold(mode: ValidationMode) -> f64 {
    match mode {
        ValidationMode::Baseline => 0.4,
        ValidationMode::Strict => 0.2,
    }
}

fn build_quality_scorecard(
    total_records: usize,
    json_records_parsed: usize,
    records_validated: usize,
    parsed_records: &[(usize, crate::models::AgentLogEvent)],
) -> ValidationQualityScorecard {
    let coverage_score = ratio_to_score(records_validated, total_records);
    let parse_success_score = ratio_to_score(json_records_parsed, total_records);

    let mut content_total = 0usize;
    let mut content_present = 0usize;
    let mut exact_count = 0usize;
    let mut derived_count = 0usize;
    let mut fallback_count = 0usize;

    for (_, record) in parsed_records {
        if requires_content_text(record) {
            content_total += 1;
            if record
                .content_text
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            {
                content_present += 1;
            }
        }

        match record.timestamp_quality {
            crate::models::TimestampQuality::Exact => exact_count += 1,
            crate::models::TimestampQuality::Derived => derived_count += 1,
            crate::models::TimestampQuality::Fallback => fallback_count += 1,
        }
    }

    let content_completeness_score = ratio_to_score(content_present, content_total);
    let timestamp_quality_score =
        weighted_timestamp_quality_score(exact_count, derived_count, fallback_count);

    let dimensions = [
        ("coverage".to_string(), coverage_score),
        ("parse_success".to_string(), parse_success_score),
        (
            "content_completeness".to_string(),
            content_completeness_score,
        ),
        ("timestamp_quality".to_string(), timestamp_quality_score),
    ];

    let overall_score = ((u32::from(coverage_score)
        + u32::from(parse_success_score)
        + u32::from(content_completeness_score)
        + u32::from(timestamp_quality_score)) as f64
        / 4.0)
        .round() as u8;

    let mut weakest_ranked = dimensions.to_vec();
    weakest_ranked.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    let weakest_dimensions = weakest_ranked
        .into_iter()
        .take(2)
        .map(|(name, _)| name)
        .collect::<Vec<_>>();

    ValidationQualityScorecard {
        overall_score,
        coverage_score,
        parse_success_score,
        content_completeness_score,
        timestamp_quality_score,
        weakest_dimensions,
    }
}

fn ratio_to_score(numerator: usize, denominator: usize) -> u8 {
    if denominator == 0 {
        return 100;
    }

    (((numerator as f64 / denominator as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0)) as u8
}

fn weighted_timestamp_quality_score(exact: usize, derived: usize, fallback: usize) -> u8 {
    let total = exact + derived + fallback;
    if total == 0 {
        return 100;
    }

    let weighted = (exact * 100) + (derived * 70) + (fallback * 30);
    (((weighted as f64 / total as f64).round()).clamp(0.0, 100.0)) as u8
}

fn validation_status(errors: usize, warnings: usize) -> ValidationStatus {
    if errors > 0 {
        ValidationStatus::Fail
    } else if warnings > 0 {
        ValidationStatus::Warn
    } else {
        ValidationStatus::Pass
    }
}

fn exit_code_from_counts(errors: usize, warnings: usize) -> i32 {
    if errors > 0 {
        2
    } else {
        let _ = warnings;
        0
    }
}

fn build_per_agent_summary(
    records: &[(usize, crate::models::AgentLogEvent)],
    issues: &[ValidationIssue],
) -> BTreeMap<String, PerAgentValidationStats> {
    let mut summary = seeded_per_agent_summary();
    let mut line_to_agent = BTreeMap::new();

    for (line, record) in records {
        let key = source_kind_key(record.source_kind);
        line_to_agent.insert(*line, key);
        let entry = summary
            .get_mut(key)
            .expect("seeded per-agent summary must include all known adapters");
        entry.records_validated += 1;
    }

    for issue in issues {
        let key = line_to_agent.get(&issue.line).copied().unwrap_or("unknown");
        let entry = summary
            .get_mut(key)
            .expect("seeded per-agent summary must include unknown bucket");
        match issue.severity {
            ValidationIssueSeverity::Warning => entry.warnings += 1,
            ValidationIssueSeverity::Error => entry.errors += 1,
        }
    }

    summary
}

fn seeded_per_agent_summary() -> BTreeMap<String, PerAgentValidationStats> {
    ["codex", "claude", "gemini", "amp", "opencode", "unknown"]
        .into_iter()
        .map(|key| (key.to_string(), PerAgentValidationStats::default()))
        .collect()
}

fn source_kind_key(source: crate::models::AgentSource) -> &'static str {
    match source {
        crate::models::AgentSource::Codex => "codex",
        crate::models::AgentSource::Claude => "claude",
        crate::models::AgentSource::Gemini => "gemini",
        crate::models::AgentSource::Amp => "amp",
        crate::models::AgentSource::OpenCode => "opencode",
    }
}
