use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

pub mod classifier;

use crate::adapters::{AdapterKind, all_adapter_kinds, default_paths};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryPathRole {
    SessionStore,
    HistoryStream,
    RuntimeDiagnostics,
    ConfigMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormatHint {
    Directory,
    Json,
    Jsonl,
    TextLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceClassification {
    Jsonl,
    Json,
    TextLog,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveryCandidate {
    pub precedence: u8,
    pub path: &'static str,
    pub role: DiscoveryPathRole,
    pub format_hint: SourceFormatHint,
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryRule {
    pub adapter: AdapterKind,
    pub candidate_paths: Vec<&'static str>,
    pub candidates: Vec<DiscoveryCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryScore {
    pub adapter: AdapterKind,
    pub score: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceSelectionFilter {
    pub adapters: Vec<AdapterKind>,
    pub format_hints: Vec<SourceFormatHint>,
    pub path_substrings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrioritizedSource {
    pub adapter: AdapterKind,
    pub path: String,
    pub role: DiscoveryPathRole,
    pub format_hint: SourceFormatHint,
    pub recursive: bool,
    pub precedence: u8,
    pub history_score: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryArtifactLayout {
    pub sources_json: PathBuf,
    pub zsh_history_usage_json: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoverySourceEvidence {
    pub adapter: String,
    pub path: String,
    pub role: String,
    pub format_hint: String,
    pub recursive: bool,
    pub precedence: u8,
    pub history_score: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoverySourcesArtifact {
    pub total_sources: usize,
    pub adapter_counts: std::collections::BTreeMap<String, usize>,
    pub sources: Vec<DiscoverySourceEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveryHistoryUsage {
    pub adapter: String,
    pub score: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveryHistoryUsageArtifact {
    pub total_command_hits: usize,
    pub adapter_usage: Vec<DiscoveryHistoryUsage>,
}

const CODEX_CANDIDATES: [DiscoveryCandidate; 3] = [
    DiscoveryCandidate {
        precedence: 10,
        path: "~/.codex/sessions",
        role: DiscoveryPathRole::SessionStore,
        format_hint: SourceFormatHint::Directory,
        recursive: true,
    },
    DiscoveryCandidate {
        precedence: 20,
        path: "~/.codex/history.jsonl",
        role: DiscoveryPathRole::HistoryStream,
        format_hint: SourceFormatHint::Jsonl,
        recursive: false,
    },
    DiscoveryCandidate {
        precedence: 30,
        path: "~/.codex/log",
        role: DiscoveryPathRole::RuntimeDiagnostics,
        format_hint: SourceFormatHint::TextLog,
        recursive: true,
    },
];

const CLAUDE_CANDIDATES: [DiscoveryCandidate; 3] = [
    DiscoveryCandidate {
        precedence: 10,
        path: "~/.claude/projects",
        role: DiscoveryPathRole::SessionStore,
        format_hint: SourceFormatHint::Directory,
        recursive: true,
    },
    DiscoveryCandidate {
        precedence: 20,
        path: "~/.claude/statsig",
        role: DiscoveryPathRole::RuntimeDiagnostics,
        format_hint: SourceFormatHint::TextLog,
        recursive: true,
    },
    DiscoveryCandidate {
        precedence: 30,
        path: "~/.claude.json",
        role: DiscoveryPathRole::ConfigMetadata,
        format_hint: SourceFormatHint::Json,
        recursive: false,
    },
];

const GEMINI_CANDIDATES: [DiscoveryCandidate; 3] = [
    DiscoveryCandidate {
        precedence: 10,
        path: "~/.gemini/tmp",
        role: DiscoveryPathRole::SessionStore,
        format_hint: SourceFormatHint::Directory,
        recursive: true,
    },
    DiscoveryCandidate {
        precedence: 20,
        path: "~/.gemini/history",
        role: DiscoveryPathRole::HistoryStream,
        format_hint: SourceFormatHint::Directory,
        recursive: true,
    },
    DiscoveryCandidate {
        precedence: 30,
        path: "~/.gemini/debug",
        role: DiscoveryPathRole::RuntimeDiagnostics,
        format_hint: SourceFormatHint::TextLog,
        recursive: true,
    },
];

const AMP_CANDIDATES: [DiscoveryCandidate; 3] = [
    DiscoveryCandidate {
        precedence: 10,
        path: "~/.amp/sessions",
        role: DiscoveryPathRole::SessionStore,
        format_hint: SourceFormatHint::Directory,
        recursive: true,
    },
    DiscoveryCandidate {
        precedence: 20,
        path: "~/.amp/history",
        role: DiscoveryPathRole::HistoryStream,
        format_hint: SourceFormatHint::Directory,
        recursive: true,
    },
    DiscoveryCandidate {
        precedence: 30,
        path: "~/.amp/logs",
        role: DiscoveryPathRole::RuntimeDiagnostics,
        format_hint: SourceFormatHint::TextLog,
        recursive: true,
    },
];

const OPENCODE_CANDIDATES: [DiscoveryCandidate; 3] = [
    DiscoveryCandidate {
        precedence: 10,
        path: "~/.opencode/project",
        role: DiscoveryPathRole::SessionStore,
        format_hint: SourceFormatHint::Directory,
        recursive: true,
    },
    DiscoveryCandidate {
        precedence: 20,
        path: "~/.opencode/sessions",
        role: DiscoveryPathRole::SessionStore,
        format_hint: SourceFormatHint::Directory,
        recursive: true,
    },
    DiscoveryCandidate {
        precedence: 30,
        path: "~/.opencode/logs",
        role: DiscoveryPathRole::RuntimeDiagnostics,
        format_hint: SourceFormatHint::TextLog,
        recursive: true,
    },
];

#[must_use]
pub const fn known_path_candidates(adapter: AdapterKind) -> &'static [DiscoveryCandidate] {
    match adapter {
        AdapterKind::Codex => &CODEX_CANDIDATES,
        AdapterKind::Claude => &CLAUDE_CANDIDATES,
        AdapterKind::Gemini => &GEMINI_CANDIDATES,
        AdapterKind::Amp => &AMP_CANDIDATES,
        AdapterKind::OpenCode => &OPENCODE_CANDIDATES,
    }
}

#[must_use]
pub fn classify_source(path: &Path, sample: &[u8]) -> SourceClassification {
    if sample.contains(&0) {
        return SourceClassification::Binary;
    }

    if let Some(extension) = path.extension().and_then(std::ffi::OsStr::to_str) {
        match extension.to_ascii_lowercase().as_str() {
            "jsonl" | "ndjson" => return SourceClassification::Jsonl,
            "json" => return SourceClassification::Json,
            "log" | "txt" => return SourceClassification::TextLog,
            _ => {}
        }
    }

    let text = match std::str::from_utf8(sample) {
        Ok(text) => text,
        Err(_) => return SourceClassification::Binary,
    };

    if looks_like_jsonl(text) {
        return SourceClassification::Jsonl;
    }

    if looks_like_json_document(text) {
        return SourceClassification::Json;
    }

    SourceClassification::TextLog
}

fn looks_like_json_document(text: &str) -> bool {
    let trimmed = text.trim();
    (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}

fn looks_like_jsonl(text: &str) -> bool {
    let mut non_empty = text.lines().map(str::trim).filter(|line| !line.is_empty());

    let first = match non_empty.next() {
        Some(line) => line,
        None => return false,
    };

    let second = match non_empty.next() {
        Some(line) => line,
        None => return false,
    };

    looks_like_json_line(first) && looks_like_json_line(second)
}

fn looks_like_json_line(line: &str) -> bool {
    (line.starts_with('{') && line.ends_with('}')) || (line.starts_with('[') && line.ends_with(']'))
}

#[must_use]
pub fn zsh_history_scores(raw_history: &str) -> Vec<HistoryScore> {
    let entries = crate::utils::history::parse_zsh_history(raw_history);
    crate::utils::history::score_adapter_command_frequency(&entries)
        .into_iter()
        .map(|score| HistoryScore {
            adapter: score.adapter,
            score: score.command_hits,
        })
        .collect()
}

#[must_use]
pub fn prioritized_sources(
    raw_history: &str,
    filter: &SourceSelectionFilter,
) -> Vec<PrioritizedSource> {
    let rules = known_path_registry();
    let history_scores = zsh_history_scores(raw_history);
    prioritize_sources(&rules, &history_scores, filter)
}

#[must_use]
pub fn build_artifact_layout(out_dir: &Path) -> DiscoveryArtifactLayout {
    let discovery_dir = out_dir.join("discovery");
    DiscoveryArtifactLayout {
        sources_json: discovery_dir.join("sources.json"),
        zsh_history_usage_json: discovery_dir.join("zsh_history_usage.json"),
    }
}

#[must_use]
pub fn build_sources_artifact(
    prioritized_sources: &[PrioritizedSource],
) -> DiscoverySourcesArtifact {
    let mut sources = prioritized_sources
        .iter()
        .map(|source| DiscoverySourceEvidence {
            adapter: adapter_sort_key(source.adapter).to_string(),
            path: source.path.clone(),
            role: discovery_path_role_key(source.role).to_string(),
            format_hint: source_format_hint_key(source.format_hint).to_string(),
            recursive: source.recursive,
            precedence: source.precedence,
            history_score: source.history_score,
        })
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| {
        right
            .history_score
            .cmp(&left.history_score)
            .then_with(|| left.precedence.cmp(&right.precedence))
            .then_with(|| left.adapter.cmp(&right.adapter))
            .then_with(|| left.path.cmp(&right.path))
    });

    let mut adapter_counts = std::collections::BTreeMap::new();
    for source in &sources {
        *adapter_counts.entry(source.adapter.clone()).or_insert(0) += 1;
    }

    DiscoverySourcesArtifact {
        total_sources: sources.len(),
        adapter_counts,
        sources,
    }
}

#[must_use]
pub fn build_zsh_history_usage_artifact(
    history_scores: &[HistoryScore],
) -> DiscoveryHistoryUsageArtifact {
    let mut usage = history_scores
        .iter()
        .map(|score| DiscoveryHistoryUsage {
            adapter: adapter_sort_key(score.adapter).to_string(),
            score: score.score,
        })
        .collect::<Vec<_>>();
    usage.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.adapter.cmp(&right.adapter))
    });

    DiscoveryHistoryUsageArtifact {
        total_command_hits: usage.iter().map(|entry| entry.score).sum(),
        adapter_usage: usage,
    }
}

pub fn write_discovery_artifacts(
    layout: &DiscoveryArtifactLayout,
    prioritized_sources: &[PrioritizedSource],
    history_scores: &[HistoryScore],
) -> Result<()> {
    let sources = build_sources_artifact(prioritized_sources);
    let history_usage = build_zsh_history_usage_artifact(history_scores);
    write_sources_artifact(&layout.sources_json, &sources)?;
    write_zsh_history_usage_artifact(&layout.zsh_history_usage_json, &history_usage)
}

pub fn write_sources_artifact(path: &Path, artifact: &DiscoverySourcesArtifact) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create discovery artifact directory: {}",
                parent.display()
            )
        })?;
    }

    let encoded = serde_json::to_vec_pretty(artifact)
        .context("failed to encode discovery sources artifact")?;
    std::fs::write(path, encoded).with_context(|| {
        format!(
            "failed to write discovery sources artifact: {}",
            path.display()
        )
    })
}

pub fn write_zsh_history_usage_artifact(
    path: &Path,
    artifact: &DiscoveryHistoryUsageArtifact,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create discovery artifact directory: {}",
                parent.display()
            )
        })?;
    }

    let encoded = serde_json::to_vec_pretty(artifact)
        .context("failed to encode discovery history usage artifact")?;
    std::fs::write(path, encoded).with_context(|| {
        format!(
            "failed to write discovery history usage artifact: {}",
            path.display()
        )
    })
}

#[must_use]
pub fn prioritize_sources(
    rules: &[DiscoveryRule],
    history_scores: &[HistoryScore],
    filter: &SourceSelectionFilter,
) -> Vec<PrioritizedSource> {
    let normalized_needles = filter
        .path_substrings
        .iter()
        .map(|needle| needle.to_ascii_lowercase())
        .collect::<Vec<_>>();

    let mut prioritized = Vec::new();
    for rule in rules {
        if !matches_adapter_filter(filter, rule.adapter) {
            continue;
        }

        let history_score = history_score_for(rule.adapter, history_scores);
        for candidate in &rule.candidates {
            if !matches_format_filter(filter, candidate.format_hint) {
                continue;
            }

            if !matches_path_filter(candidate.path, &normalized_needles) {
                continue;
            }

            prioritized.push(PrioritizedSource {
                adapter: rule.adapter,
                path: candidate.path.to_string(),
                role: candidate.role,
                format_hint: candidate.format_hint,
                recursive: candidate.recursive,
                precedence: candidate.precedence,
                history_score,
            });
        }
    }

    prioritized.sort_by(|left, right| {
        right
            .history_score
            .cmp(&left.history_score)
            .then_with(|| left.precedence.cmp(&right.precedence))
            .then_with(|| adapter_sort_key(left.adapter).cmp(adapter_sort_key(right.adapter)))
            .then_with(|| left.path.cmp(&right.path))
    });
    prioritized
}

fn matches_adapter_filter(filter: &SourceSelectionFilter, adapter: AdapterKind) -> bool {
    filter.adapters.is_empty() || filter.adapters.contains(&adapter)
}

fn matches_format_filter(filter: &SourceSelectionFilter, format_hint: SourceFormatHint) -> bool {
    filter.format_hints.is_empty() || filter.format_hints.contains(&format_hint)
}

fn matches_path_filter(path: &str, normalized_needles: &[String]) -> bool {
    if normalized_needles.is_empty() {
        return true;
    }

    let normalized_path = path.to_ascii_lowercase();
    normalized_needles
        .iter()
        .any(|needle| normalized_path.contains(needle))
}

fn history_score_for(adapter: AdapterKind, history_scores: &[HistoryScore]) -> usize {
    history_scores
        .iter()
        .find_map(|score| (score.adapter == adapter).then_some(score.score))
        .unwrap_or(0)
}

fn adapter_sort_key(adapter: AdapterKind) -> &'static str {
    match adapter {
        AdapterKind::Codex => "codex",
        AdapterKind::Claude => "claude",
        AdapterKind::Gemini => "gemini",
        AdapterKind::Amp => "amp",
        AdapterKind::OpenCode => "opencode",
    }
}

const fn discovery_path_role_key(role: DiscoveryPathRole) -> &'static str {
    match role {
        DiscoveryPathRole::SessionStore => "session_store",
        DiscoveryPathRole::HistoryStream => "history_stream",
        DiscoveryPathRole::RuntimeDiagnostics => "runtime_diagnostics",
        DiscoveryPathRole::ConfigMetadata => "config_metadata",
    }
}

const fn source_format_hint_key(hint: SourceFormatHint) -> &'static str {
    match hint {
        SourceFormatHint::Directory => "directory",
        SourceFormatHint::Json => "json",
        SourceFormatHint::Jsonl => "jsonl",
        SourceFormatHint::TextLog => "text_log",
    }
}

#[must_use]
pub fn known_path_registry() -> Vec<DiscoveryRule> {
    all_adapter_kinds()
        .into_iter()
        .map(|adapter| {
            let candidates = known_path_candidates(adapter).to_vec();
            let candidate_paths = candidates
                .iter()
                .map(|candidate| candidate.path)
                .collect::<Vec<_>>();
            debug_assert_eq!(candidate_paths.as_slice(), default_paths(adapter));

            DiscoveryRule {
                adapter,
                candidate_paths,
                candidates,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        SourceClassification, classify_source, known_path_candidates, known_path_registry,
        zsh_history_scores,
    };
    use crate::adapters::{AdapterKind, all_adapter_kinds};

    #[test]
    fn registry_covers_all_adapters() {
        let registry = known_path_registry();
        assert_eq!(registry.len(), all_adapter_kinds().len());
        assert!(registry.iter().all(|rule| !rule.candidates.is_empty()));
    }

    #[test]
    fn precedence_is_strictly_increasing_per_adapter() {
        for adapter in all_adapter_kinds() {
            let mut previous = 0_u8;
            for candidate in known_path_candidates(adapter) {
                assert!(
                    candidate.precedence > previous,
                    "precedence must increase for {:?}",
                    adapter
                );
                previous = candidate.precedence;
            }
        }
    }

    #[test]
    fn candidate_path_projection_matches_detailed_candidates() {
        for rule in known_path_registry() {
            let projected = rule
                .candidates
                .iter()
                .map(|candidate| candidate.path)
                .collect::<Vec<_>>();
            assert_eq!(projected, rule.candidate_paths);
        }
    }

    #[test]
    fn classifies_by_extension_when_available() {
        let classified = classify_source(Path::new("/tmp/events.jsonl"), b"not-json");
        assert_eq!(classified, SourceClassification::Jsonl);

        let classified = classify_source(Path::new("/tmp/config.json"), b"not-json");
        assert_eq!(classified, SourceClassification::Json);
    }

    #[test]
    fn classifies_jsonl_from_content() {
        let sample = br#"{"a":1}
{"b":2}
"#;
        let classified = classify_source(Path::new("/tmp/unknown.dat"), sample);
        assert_eq!(classified, SourceClassification::Jsonl);
    }

    #[test]
    fn classifies_json_document_from_content() {
        let classified = classify_source(Path::new("/tmp/blob"), br#"{"hello":"world"}"#);
        assert_eq!(classified, SourceClassification::Json);
    }

    #[test]
    fn classifies_binary_when_null_bytes_present() {
        let classified = classify_source(Path::new("/tmp/blob"), b"\x00\x01\x02");
        assert_eq!(classified, SourceClassification::Binary);
    }

    #[test]
    fn falls_back_to_text_log_for_plain_text() {
        let classified = classify_source(Path::new("/tmp/runtime"), b"INFO started\nWARN retrying");
        assert_eq!(classified, SourceClassification::TextLog);
    }

    #[test]
    fn computes_zsh_history_scores_for_each_adapter() {
        let history = r#"
: 1740467001:0;codex --full-auto
: 1740467002:0;claude --resume
cat ~/.opencode/sessions/latest.json
"#;

        let scores = zsh_history_scores(history);
        assert_eq!(scores.len(), all_adapter_kinds().len());

        let codex = scores
            .iter()
            .find(|score| score.adapter == AdapterKind::Codex)
            .expect("codex score should exist");
        assert_eq!(codex.score, 1);

        let claude = scores
            .iter()
            .find(|score| score.adapter == AdapterKind::Claude)
            .expect("claude score should exist");
        assert_eq!(claude.score, 1);

        let opencode = scores
            .iter()
            .find(|score| score.adapter == AdapterKind::OpenCode)
            .expect("opencode score should exist");
        assert_eq!(opencode.score, 1);

        let gemini = scores
            .iter()
            .find(|score| score.adapter == AdapterKind::Gemini)
            .expect("gemini score should exist");
        assert_eq!(gemini.score, 0);
    }
}
