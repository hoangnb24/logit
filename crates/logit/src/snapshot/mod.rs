pub mod profiler;
pub mod samples;

use std::collections::BTreeMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};

use crate::adapters::{AdapterKind, all_adapter_kinds};
use crate::discovery::{PrioritizedSource, SourceSelectionFilter, prioritized_sources};
use crate::models::{AgentSource, SCHEMA_VERSION};
use crate::utils::redaction::DEFAULT_SNAPSHOT_MAX_CHARS;

use self::profiler::{KeyStats, SourceProfile};
use self::samples::{
    RepresentativeSample, SampleCandidate, extract_representative_samples,
    redact_and_truncate_samples,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotConfig {
    pub sample_size: usize,
    pub redact_sensitive_values: bool,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            sample_size: 3,
            redact_sensitive_values: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotArtifactLayout {
    pub index_json: PathBuf,
    pub samples_jsonl: PathBuf,
    pub schema_profile_json: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotArtifactPointers {
    pub index_json: String,
    pub samples_jsonl: String,
    pub schema_profile_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotIndexCounts {
    pub discovered_sources: usize,
    pub existing_sources: usize,
    pub files_profiled: usize,
    pub records_profiled: usize,
    pub samples_emitted: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotDiscoveredSource {
    pub adapter: String,
    pub source_kind: String,
    pub path: String,
    pub resolved_path: String,
    pub format_hint: String,
    pub recursive: bool,
    pub exists: bool,
    pub files_profiled: usize,
    pub records_profiled: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotIndex {
    pub schema_version: String,
    pub sample_size: usize,
    pub redaction_enabled: bool,
    pub artifacts: SnapshotArtifactPointers,
    pub counts: SnapshotIndexCounts,
    pub sources: Vec<SnapshotDiscoveredSource>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SerializableKeyStats {
    pub occurrences: usize,
    pub value_types: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotSchemaProfileEntry {
    pub adapter: String,
    pub source_kind: String,
    pub source_path: String,
    pub resolved_path: String,
    pub format_hint: String,
    pub files_profiled: usize,
    pub records_profiled: usize,
    pub event_kind_frequency: BTreeMap<String, usize>,
    pub key_stats: BTreeMap<String, SerializableKeyStats>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotSchemaProfile {
    pub schema_version: String,
    pub profiles: Vec<SnapshotSchemaProfileEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotCollection {
    pub index: SnapshotIndex,
    pub samples: Vec<RepresentativeSample>,
    pub schema_profile: SnapshotSchemaProfile,
}

#[must_use]
pub fn build_artifact_layout(out_dir: &Path) -> SnapshotArtifactLayout {
    let snapshot_dir = out_dir.join("snapshot");
    SnapshotArtifactLayout {
        index_json: snapshot_dir.join("index.json"),
        samples_jsonl: snapshot_dir.join("samples.jsonl"),
        schema_profile_json: snapshot_dir.join("schema_profile.json"),
    }
}

pub fn collect_snapshot_data(
    config: &SnapshotConfig,
    home_dir: &Path,
    source_root_override: Option<&Path>,
    zsh_history: &str,
) -> Result<SnapshotCollection> {
    let filter = SourceSelectionFilter {
        adapters: all_adapter_kinds().to_vec(),
        ..SourceSelectionFilter::default()
    };
    let prioritized = prioritized_sources(zsh_history, &filter);

    let mut discovered_sources = Vec::new();
    let mut profile_entries = Vec::new();
    let mut sample_candidates = Vec::new();
    let mut warnings = Vec::new();
    let mut existing_sources = 0usize;
    let mut files_profiled_total = 0usize;
    let mut records_profiled_total = 0usize;

    for source in &prioritized {
        let resolved = resolve_candidate_path(&source.path, home_dir, source_root_override);
        let exists = resolved.exists();
        if exists {
            existing_sources += 1;
        }

        let mut merged_profile = SourceProfile::empty();
        let mut source_warnings = Vec::new();
        let mut files_profiled = 0usize;
        let mut records_profiled = 0usize;
        let parseable_files = match collect_parseable_files(&resolved, source) {
            Ok(files) => files,
            Err(error) => {
                source_warnings.push(format!(
                    "adapter `{}` source path unreadable `{}`: {error}",
                    source.adapter.as_str(),
                    resolved.display()
                ));
                Vec::new()
            }
        };

        for file in parseable_files {
            let parsed = parse_snapshot_records(&file).with_context(|| {
                format!("failed to parse snapshot source file: {}", file.display())
            })?;
            files_profiled += 1;
            records_profiled += parsed.records.len();

            let file_values = parsed
                .records
                .iter()
                .map(|record| record.value.clone())
                .collect::<Vec<_>>();
            let file_profile = profiler::profile_json_records(&file_values);
            merge_source_profile(&mut merged_profile, &file_profile);

            let source_kind = adapter_to_source(source.adapter);
            for record in parsed.records {
                sample_candidates.push(SampleCandidate {
                    source_kind,
                    source_path: file.to_string_lossy().to_string(),
                    source_record_locator: record.locator,
                    record: record.value,
                });
            }

            source_warnings.extend(
                parsed
                    .warnings
                    .into_iter()
                    .map(|warning| format!("{}: {warning}", file.display())),
            );
        }

        files_profiled_total += files_profiled;
        records_profiled_total += records_profiled;
        warnings.extend(source_warnings.clone());

        discovered_sources.push(SnapshotDiscoveredSource {
            adapter: source.adapter.as_str().to_string(),
            source_kind: source_kind_key(adapter_to_source(source.adapter)).to_string(),
            path: source.path.clone(),
            resolved_path: resolved.to_string_lossy().to_string(),
            format_hint: format_hint_key(source).to_string(),
            recursive: source.recursive,
            exists,
            files_profiled,
            records_profiled,
        });

        profile_entries.push(SnapshotSchemaProfileEntry {
            adapter: source.adapter.as_str().to_string(),
            source_kind: source_kind_key(adapter_to_source(source.adapter)).to_string(),
            source_path: source.path.clone(),
            resolved_path: resolved.to_string_lossy().to_string(),
            format_hint: format_hint_key(source).to_string(),
            files_profiled,
            records_profiled,
            event_kind_frequency: merged_profile.event_kind_frequency,
            key_stats: serialize_key_stats(merged_profile.key_stats),
            warnings: source_warnings,
        });
    }

    let mut samples = extract_representative_samples(&sample_candidates, config.sample_size);
    if config.redact_sensitive_values {
        samples = redact_and_truncate_samples(&samples, DEFAULT_SNAPSHOT_MAX_CHARS);
    }

    let index = SnapshotIndex {
        schema_version: SCHEMA_VERSION.to_string(),
        sample_size: config.sample_size,
        redaction_enabled: config.redact_sensitive_values,
        artifacts: SnapshotArtifactPointers {
            index_json: "snapshot/index.json".to_string(),
            samples_jsonl: "snapshot/samples.jsonl".to_string(),
            schema_profile_json: "snapshot/schema_profile.json".to_string(),
        },
        counts: SnapshotIndexCounts {
            discovered_sources: discovered_sources.len(),
            existing_sources,
            files_profiled: files_profiled_total,
            records_profiled: records_profiled_total,
            samples_emitted: samples.len(),
            warnings: warnings.len(),
        },
        sources: discovered_sources,
        warnings,
    };

    Ok(SnapshotCollection {
        index,
        samples,
        schema_profile: SnapshotSchemaProfile {
            schema_version: SCHEMA_VERSION.to_string(),
            profiles: profile_entries,
        },
    })
}

pub fn write_snapshot_artifacts(
    layout: &SnapshotArtifactLayout,
    collection: &SnapshotCollection,
) -> Result<()> {
    verify_snapshot_collection_integrity(collection)?;
    write_index_artifact(&layout.index_json, &collection.index)?;
    write_samples_artifact(&layout.samples_jsonl, &collection.samples)?;
    write_schema_profile_artifact(&layout.schema_profile_json, &collection.schema_profile)?;
    verify_snapshot_artifacts_parseable(layout)
}

pub fn write_index_artifact(path: &Path, index: &SnapshotIndex) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("failed to create snapshot index directory")?;
    }
    let encoded = serde_json::to_vec_pretty(index).context("failed to encode snapshot index")?;
    std::fs::write(path, encoded).context("failed to write snapshot index artifact")
}

pub fn write_samples_artifact(path: &Path, samples: &[RepresentativeSample]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("failed to create snapshot samples directory")?;
    }
    let file = std::fs::File::create(path).context("failed to create snapshot samples artifact")?;
    let mut writer = BufWriter::new(file);
    for sample in samples {
        serde_json::to_writer(&mut writer, sample)
            .context("failed to encode snapshot samples jsonl row")?;
        writer
            .write_all(b"\n")
            .context("failed to write snapshot samples newline")?;
    }
    writer
        .flush()
        .context("failed to flush snapshot samples artifact writer")
}

pub fn write_schema_profile_artifact(path: &Path, profile: &SnapshotSchemaProfile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context("failed to create snapshot schema profile directory")?;
    }
    let encoded =
        serde_json::to_vec_pretty(profile).context("failed to encode snapshot schema profile")?;
    std::fs::write(path, encoded).context("failed to write snapshot schema profile artifact")
}

pub fn verify_snapshot_collection_integrity(collection: &SnapshotCollection) -> Result<()> {
    let mut issues = Vec::new();

    if collection.index.counts.discovered_sources != collection.index.sources.len() {
        issues.push(format!(
            "index counts mismatch: discovered_sources={}, sources.len()={}",
            collection.index.counts.discovered_sources,
            collection.index.sources.len()
        ));
    }
    if collection.index.counts.existing_sources > collection.index.counts.discovered_sources {
        issues.push(format!(
            "index counts mismatch: existing_sources={} exceeds discovered_sources={}",
            collection.index.counts.existing_sources, collection.index.counts.discovered_sources
        ));
    }
    if collection.index.counts.samples_emitted != collection.samples.len() {
        issues.push(format!(
            "index counts mismatch: samples_emitted={}, samples.len()={}",
            collection.index.counts.samples_emitted,
            collection.samples.len()
        ));
    }
    if collection.index.counts.warnings != collection.index.warnings.len() {
        issues.push(format!(
            "index counts mismatch: warnings={}, warnings.len()={}",
            collection.index.counts.warnings,
            collection.index.warnings.len()
        ));
    }

    let files_profiled = collection
        .index
        .sources
        .iter()
        .map(|source| source.files_profiled)
        .sum::<usize>();
    if files_profiled != collection.index.counts.files_profiled {
        issues.push(format!(
            "index counts mismatch: files_profiled={}, summed sources={}",
            collection.index.counts.files_profiled, files_profiled
        ));
    }

    let records_profiled = collection
        .index
        .sources
        .iter()
        .map(|source| source.records_profiled)
        .sum::<usize>();
    if records_profiled != collection.index.counts.records_profiled {
        issues.push(format!(
            "index counts mismatch: records_profiled={}, summed sources={}",
            collection.index.counts.records_profiled, records_profiled
        ));
    }

    for source in &collection.index.sources {
        if !source.exists && (source.files_profiled > 0 || source.records_profiled > 0) {
            issues.push(format!(
                "source `{}` marked non-existent but has profiled counts (files={}, records={})",
                source.path, source.files_profiled, source.records_profiled
            ));
        }
    }

    issues.extend(validate_sample_determinism(&collection.samples));

    if issues.is_empty() {
        Ok(())
    } else {
        bail!(
            "snapshot collection integrity check failed: {}",
            issues.join("; ")
        );
    }
}

pub fn verify_snapshot_artifacts_parseable(layout: &SnapshotArtifactLayout) -> Result<()> {
    let index_raw = std::fs::read_to_string(&layout.index_json).with_context(|| {
        format!(
            "failed to read snapshot index: {}",
            layout.index_json.display()
        )
    })?;
    let index: Value =
        serde_json::from_str(&index_raw).context("snapshot index artifact is not valid JSON")?;
    let samples_raw = std::fs::read_to_string(&layout.samples_jsonl).with_context(|| {
        format!(
            "failed to read snapshot samples artifact: {}",
            layout.samples_jsonl.display()
        )
    })?;
    let schema_profile_raw =
        std::fs::read_to_string(&layout.schema_profile_json).with_context(|| {
            format!(
                "failed to read snapshot schema profile artifact: {}",
                layout.schema_profile_json.display()
            )
        })?;
    let schema_profile: Value = serde_json::from_str(&schema_profile_raw)
        .context("snapshot schema profile artifact is not valid JSON")?;

    let sample_rows = parse_samples_jsonl_rows(&samples_raw)?;
    let samples_emitted = index
        .pointer("/counts/samples_emitted")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            anyhow::anyhow!("snapshot index missing numeric `counts.samples_emitted`")
        })?;

    if samples_emitted != sample_rows.len() as u64 {
        bail!(
            "snapshot artifact mismatch: counts.samples_emitted={} but parsed sample rows={}",
            samples_emitted,
            sample_rows.len()
        );
    }

    if index
        .pointer("/artifacts/index_json")
        .and_then(Value::as_str)
        != Some("snapshot/index.json")
    {
        bail!("snapshot index artifact pointer mismatch for `artifacts.index_json`");
    }
    if index
        .pointer("/artifacts/samples_jsonl")
        .and_then(Value::as_str)
        != Some("snapshot/samples.jsonl")
    {
        bail!("snapshot index artifact pointer mismatch for `artifacts.samples_jsonl`");
    }
    if index
        .pointer("/artifacts/schema_profile_json")
        .and_then(Value::as_str)
        != Some("snapshot/schema_profile.json")
    {
        bail!("snapshot index artifact pointer mismatch for `artifacts.schema_profile_json`");
    }

    if schema_profile
        .pointer("/profiles")
        .and_then(Value::as_array)
        .is_none()
    {
        bail!("snapshot schema profile artifact is missing `profiles` array");
    }

    Ok(())
}

fn parse_samples_jsonl_rows(input: &str) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    for (line_idx, line) in input.lines().enumerate() {
        let line_number = line_idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed = serde_json::from_str::<Value>(trimmed).with_context(|| {
            format!("snapshot samples artifact contains invalid JSON at line {line_number}")
        })?;
        rows.push(parsed);
    }
    Ok(rows)
}

fn validate_sample_determinism(samples: &[RepresentativeSample]) -> Vec<String> {
    if samples.is_empty() {
        return Vec::new();
    }

    let mut issues = Vec::new();
    let mut expected_rank_by_source: BTreeMap<(String, String), usize> = BTreeMap::new();

    for sample in samples {
        let key = (
            source_kind_key(sample.source_kind).to_string(),
            sample.source_path.clone(),
        );
        let expected = *expected_rank_by_source.get(&key).unwrap_or(&0);
        if sample.sample_rank != expected {
            issues.push(format!(
                "sample rank sequence mismatch for source `{}`: expected {}, found {} at locator `{}`",
                sample.source_path, expected, sample.sample_rank, sample.source_record_locator
            ));
        }
        expected_rank_by_source.insert(key, sample.sample_rank.saturating_add(1));
    }

    for pair in samples.windows(2) {
        if compare_representative_samples(&pair[0], &pair[1]) == std::cmp::Ordering::Greater {
            issues.push(format!(
                "samples are not in deterministic order at `{}` then `{}`",
                pair[0].source_record_locator, pair[1].source_record_locator
            ));
            break;
        }
    }

    issues
}

fn compare_representative_samples(
    left: &RepresentativeSample,
    right: &RepresentativeSample,
) -> std::cmp::Ordering {
    source_kind_key(left.source_kind)
        .cmp(source_kind_key(right.source_kind))
        .then_with(|| left.source_path.cmp(&right.source_path))
        .then_with(|| left.sample_rank.cmp(&right.sample_rank))
        .then_with(|| left.source_record_locator.cmp(&right.source_record_locator))
        .then_with(|| canonical_json(&left.record).cmp(&canonical_json(&right.record)))
}

fn canonical_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn resolve_candidate_path(
    candidate: &str,
    home_dir: &Path,
    source_root_override: Option<&Path>,
) -> PathBuf {
    if let Some(stripped) = candidate.strip_prefix("~/") {
        if let Some(root) = source_root_override {
            return root.join(stripped);
        }
        return home_dir.join(stripped);
    }

    if candidate == "~" {
        return source_root_override.unwrap_or(home_dir).to_path_buf();
    }

    if let Some(root) = source_root_override {
        return root.join(candidate);
    }

    PathBuf::from(candidate)
}

fn collect_parseable_files(resolved: &Path, source: &PrioritizedSource) -> Result<Vec<PathBuf>> {
    if resolved.is_file() {
        return Ok(if is_parseable_snapshot_file(resolved) {
            vec![resolved.to_path_buf()]
        } else {
            Vec::new()
        });
    }

    if !resolved.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_dir_files(resolved, source.recursive, &mut files)?;
    files.sort();
    files.retain(|path| is_parseable_snapshot_file(path));
    Ok(files)
}

fn collect_dir_files(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = std::fs::read_dir(dir)
        .with_context(|| format!("failed to read source directory: {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to enumerate source directory: {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_file() {
            out.push(path);
        } else if recursive && path.is_dir() {
            collect_dir_files(&path, recursive, out)?;
        }
    }

    Ok(())
}

fn is_parseable_snapshot_file(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "json" | "jsonl" | "ndjson" | "pb"
            )
        })
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedSnapshotRecord {
    locator: String,
    value: Value,
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedSnapshotRecords {
    records: Vec<ParsedSnapshotRecord>,
    warnings: Vec<String>,
}

fn parse_snapshot_records(path: &Path) -> Result<ParsedSnapshotRecords> {
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(|ext| ext.to_ascii_lowercase());

    match extension.as_deref() {
        Some("jsonl") | Some("ndjson") => {
            let input = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read source file: {}", path.display()))?;
            Ok(parse_snapshot_jsonl_records(&input))
        }
        Some("json") => {
            let input = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read source file: {}", path.display()))?;
            Ok(parse_snapshot_json_records(&input))
        }
        Some("pb") => parse_snapshot_protobuf_records(path),
        _ => Ok(ParsedSnapshotRecords {
            records: Vec::new(),
            warnings: vec![format!(
                "unsupported source extension for snapshot parsing: {}",
                path.display()
            )],
        }),
    }
}

fn parse_snapshot_protobuf_records(path: &Path) -> Result<ParsedSnapshotRecords> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to stat protobuf source file: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("unknown.pb");

    Ok(ParsedSnapshotRecords {
        records: vec![ParsedSnapshotRecord {
            locator: "protobuf:metadata".to_string(),
            value: json!({
                "snapshot_only": true,
                "artifact_kind": "protobuf-binary",
                "file_name": file_name,
                "byte_size": metadata.len(),
            }),
        }],
        warnings: vec!["protobuf binary indexed as snapshot-only metadata; decode skipped".into()],
    })
}

fn parse_snapshot_jsonl_records(input: &str) -> ParsedSnapshotRecords {
    let mut records = Vec::new();
    let mut warnings = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => records.push(ParsedSnapshotRecord {
                locator: format!("line:{line_number}"),
                value,
            }),
            Err(error) => warnings.push(format!("line {line_number}: invalid JSON ({error})")),
        }
    }

    ParsedSnapshotRecords { records, warnings }
}

fn parse_snapshot_json_records(input: &str) -> ParsedSnapshotRecords {
    let value = match serde_json::from_str::<Value>(input) {
        Ok(value) => value,
        Err(error) => {
            return ParsedSnapshotRecords {
                records: Vec::new(),
                warnings: vec![format!("invalid JSON document ({error})")],
            };
        }
    };

    if let Some(messages) = value
        .as_object()
        .and_then(|object| object.get("messages"))
        .and_then(Value::as_array)
    {
        let records = messages
            .iter()
            .enumerate()
            .map(|(index, message)| ParsedSnapshotRecord {
                locator: format!("messages:{}", index + 1),
                value: message.clone(),
            })
            .collect::<Vec<_>>();

        return ParsedSnapshotRecords {
            records,
            warnings: Vec::new(),
        };
    }

    match value {
        Value::Array(items) => ParsedSnapshotRecords {
            records: items
                .into_iter()
                .enumerate()
                .map(|(index, item)| ParsedSnapshotRecord {
                    locator: format!("index:{}", index + 1),
                    value: item,
                })
                .collect(),
            warnings: Vec::new(),
        },
        root => ParsedSnapshotRecords {
            records: vec![ParsedSnapshotRecord {
                locator: "root".to_string(),
                value: root,
            }],
            warnings: Vec::new(),
        },
    }
}

fn merge_source_profile(into: &mut SourceProfile, other: &SourceProfile) {
    into.total_records += other.total_records;
    for (key, stats) in &other.key_stats {
        let entry = into
            .key_stats
            .entry(key.clone())
            .or_insert_with(|| KeyStats {
                occurrences: 0,
                value_types: BTreeMap::new(),
            });
        entry.occurrences += stats.occurrences;
        for (value_type, count) in &stats.value_types {
            *entry.value_types.entry(value_type.clone()).or_insert(0) += *count;
        }
    }
    for (event_kind, count) in &other.event_kind_frequency {
        *into
            .event_kind_frequency
            .entry(event_kind.clone())
            .or_insert(0) += *count;
    }
}

fn serialize_key_stats(
    key_stats: BTreeMap<String, KeyStats>,
) -> BTreeMap<String, SerializableKeyStats> {
    key_stats
        .into_iter()
        .map(|(key, stats)| {
            (
                key,
                SerializableKeyStats {
                    occurrences: stats.occurrences,
                    value_types: stats.value_types,
                },
            )
        })
        .collect()
}

fn adapter_to_source(adapter: AdapterKind) -> AgentSource {
    match adapter {
        AdapterKind::Codex => AgentSource::Codex,
        AdapterKind::Claude => AgentSource::Claude,
        AdapterKind::Gemini => AgentSource::Gemini,
        AdapterKind::Amp => AgentSource::Amp,
        AdapterKind::OpenCode => AgentSource::OpenCode,
    }
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

fn format_hint_key(source: &PrioritizedSource) -> &'static str {
    match source.format_hint {
        crate::discovery::SourceFormatHint::Directory => "directory",
        crate::discovery::SourceFormatHint::Json => "json",
        crate::discovery::SourceFormatHint::Jsonl => "jsonl",
        crate::discovery::SourceFormatHint::TextLog => "text_log",
    }
}
