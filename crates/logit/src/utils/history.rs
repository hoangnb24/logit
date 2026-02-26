use crate::adapters::{AdapterKind, all_adapter_kinds};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZshHistoryEntry {
    pub timestamp_unix: Option<i64>,
    pub duration_seconds: Option<u64>,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterCommandFrequency {
    pub adapter: AdapterKind,
    pub command_hits: usize,
}

#[must_use]
pub fn parse_zsh_history(input: &str) -> Vec<ZshHistoryEntry> {
    input.lines().filter_map(parse_history_line).collect()
}

#[must_use]
pub fn score_adapter_command_frequency(
    entries: &[ZshHistoryEntry],
) -> Vec<AdapterCommandFrequency> {
    let mut scores = all_adapter_kinds()
        .into_iter()
        .map(|adapter| AdapterCommandFrequency {
            adapter,
            command_hits: 0,
        })
        .collect::<Vec<_>>();

    for entry in entries {
        for score in &mut scores {
            if command_matches_adapter(&entry.command, score.adapter) {
                score.command_hits += 1;
            }
        }
    }

    scores.sort_by(|left, right| {
        right
            .command_hits
            .cmp(&left.command_hits)
            .then_with(|| left.adapter.as_str().cmp(right.adapter.as_str()))
    });
    scores
}

#[must_use]
pub fn command_frequency(lines: &[&str], needle: &str) -> usize {
    lines.iter().filter(|line| line.contains(needle)).count()
}

fn parse_history_line(line: &str) -> Option<ZshHistoryEntry> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix(": ")
        && let Some((metadata, command)) = rest.split_once(';')
    {
        let command = command.trim();
        if command.is_empty() {
            return None;
        }

        let (timestamp_unix, duration_seconds) = parse_metadata(metadata);
        return Some(ZshHistoryEntry {
            timestamp_unix,
            duration_seconds,
            command: command.to_string(),
        });
    }

    Some(ZshHistoryEntry {
        timestamp_unix: None,
        duration_seconds: None,
        command: trimmed.to_string(),
    })
}

fn parse_metadata(metadata: &str) -> (Option<i64>, Option<u64>) {
    let mut parts = metadata.split(':');
    let timestamp_unix = parts
        .next()
        .map(str::trim)
        .and_then(|value| value.parse::<i64>().ok());
    let duration_seconds = parts
        .next()
        .map(str::trim)
        .and_then(|value| value.parse::<u64>().ok());
    (timestamp_unix, duration_seconds)
}

fn command_matches_adapter(command: &str, adapter: AdapterKind) -> bool {
    let normalized = command.to_ascii_lowercase();
    let primary = normalized.split_whitespace().next().unwrap_or_default();

    match adapter {
        AdapterKind::Codex => {
            primary == "codex"
                || normalized.contains(".codex/")
                || normalized.contains("/.codex")
                || normalized.contains("codex-cli")
        }
        AdapterKind::Claude => {
            primary == "claude"
                || normalized.contains(".claude/")
                || normalized.contains("/.claude")
                || normalized.contains("claude-code")
        }
        AdapterKind::Gemini => {
            primary == "gemini"
                || normalized.contains(".gemini/")
                || normalized.contains("/.gemini")
        }
        AdapterKind::Amp => {
            primary == "amp" || normalized.contains(".amp/") || normalized.contains("/.amp")
        }
        AdapterKind::OpenCode => {
            primary == "opencode"
                || normalized.contains(".opencode/")
                || normalized.contains("/.opencode")
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::adapters::AdapterKind;

    use super::{
        ZshHistoryEntry, command_frequency, parse_zsh_history, score_adapter_command_frequency,
    };

    #[test]
    fn counts_matching_lines() {
        let lines = [
            "logit normalize --fail-fast",
            "cargo test",
            "logit snapshot --sample-size 5",
            "logit normalize --fail-fast",
        ];

        assert_eq!(command_frequency(&lines, "logit normalize"), 2);
    }

    #[test]
    fn returns_zero_when_no_match() {
        let lines = ["cargo check", "cargo clippy", "cargo test"];
        assert_eq!(command_frequency(&lines, "logit"), 0);
    }

    #[test]
    fn parses_extended_and_plain_history_lines() {
        let input = r#"
: 1740467001:0;codex --full-auto
claude --resume
"#;

        let parsed = parse_zsh_history(input);
        assert_eq!(
            parsed,
            vec![
                ZshHistoryEntry {
                    timestamp_unix: Some(1_740_467_001),
                    duration_seconds: Some(0),
                    command: "codex --full-auto".to_string(),
                },
                ZshHistoryEntry {
                    timestamp_unix: None,
                    duration_seconds: None,
                    command: "claude --resume".to_string(),
                },
            ]
        );
    }

    #[test]
    fn ignores_blank_or_commandless_extended_lines() {
        let input = r#"

: 1740467001:0;
: missing; 
 
"#;

        let parsed = parse_zsh_history(input);
        assert!(parsed.is_empty());
    }

    #[test]
    fn scores_adapter_frequency_and_keeps_zeroes() {
        let entries = parse_zsh_history(
            r#"
: 1740467001:0;codex --full-auto
: 1740467002:0;codex --continue
: 1740467003:1;claude --resume
cat ~/.opencode/sessions/latest.json
"#,
        );

        let scores = score_adapter_command_frequency(&entries);

        assert_eq!(scores[0].adapter, AdapterKind::Codex);
        assert_eq!(scores[0].command_hits, 2);

        let claude = scores
            .iter()
            .find(|score| score.adapter == AdapterKind::Claude)
            .expect("claude score should be present");
        assert_eq!(claude.command_hits, 1);

        let opencode = scores
            .iter()
            .find(|score| score.adapter == AdapterKind::OpenCode)
            .expect("opencode score should be present");
        assert_eq!(opencode.command_hits, 1);

        let gemini = scores
            .iter()
            .find(|score| score.adapter == AdapterKind::Gemini)
            .expect("gemini score should be present");
        assert_eq!(gemini.command_hits, 0);
    }
}
