# logit Discovery Path Precedence Table

Status: canonical discovery candidate table for `bd-8vn`

Source of truth:
- `crates/logit/src/discovery/mod.rs` (`*_CANDIDATES`, `known_path_candidates`, `prioritize_sources`)

## 1. Purpose

This document defines the ordered known-path discovery candidates per supported agent adapter and the precedence/tie-break behavior used when building prioritized source lists.

## 2. Precedence Semantics

- Lower `precedence` value wins within the same adapter (`10` before `20` before `30`).
- `prioritize_sources(...)` sorts by:
  1. higher `history_score` (derived from zsh command frequency)
  2. lower `precedence`
  3. adapter key (`codex`, `claude`, `gemini`, `amp`, `opencode`)
  4. path string (lexicographic)
- Path substring filters are case-insensitive.
- Format and adapter filters are exact enum matches.

## 3. Candidate Table

### 3.1 Codex

| Precedence | Path | Role | Format Hint | Recursive |
|---|---|---|---|---|
| `10` | `~/.codex/sessions` | `session_store` | `directory` | `true` |
| `20` | `~/.codex/history.jsonl` | `history_stream` | `jsonl` | `false` |
| `30` | `~/.codex/log` | `runtime_diagnostics` | `text_log` | `true` |

### 3.2 Claude

| Precedence | Path | Role | Format Hint | Recursive |
|---|---|---|---|---|
| `10` | `~/.claude/projects` | `session_store` | `directory` | `true` |
| `20` | `~/.claude/statsig` | `runtime_diagnostics` | `text_log` | `true` |
| `30` | `~/.claude.json` | `config_metadata` | `json` | `false` |

### 3.3 Gemini

| Precedence | Path | Role | Format Hint | Recursive |
|---|---|---|---|---|
| `10` | `~/.gemini/tmp` | `session_store` | `directory` | `true` |
| `20` | `~/.gemini/history` | `history_stream` | `directory` | `true` |
| `30` | `~/.gemini/debug` | `runtime_diagnostics` | `text_log` | `true` |

### 3.4 Amp

| Precedence | Path | Role | Format Hint | Recursive |
|---|---|---|---|---|
| `10` | `~/.amp/sessions` | `session_store` | `directory` | `true` |
| `20` | `~/.amp/history` | `history_stream` | `directory` | `true` |
| `30` | `~/.amp/logs` | `runtime_diagnostics` | `text_log` | `true` |
| `40` | `~/.amp/file-changes` | `session_store` | `directory` | `true` |

### 3.5 OpenCode

| Precedence | Path | Role | Format Hint | Recursive |
|---|---|---|---|---|
| `10` | `~/.opencode/project` | `session_store` | `directory` | `true` |
| `20` | `~/.opencode/sessions` | `session_store` | `directory` | `true` |
| `30` | `~/.opencode/logs` | `runtime_diagnostics` | `text_log` | `true` |

## 4. Notes for Follow-on Work

- This table documents *candidate ordering*, not file readability/permission policy.
- Unreadable/missing-path handling belongs to downstream discovery execution and policy work (for example `bd-26g`).
- `default_paths(adapter)` is expected to match the ordered `candidate_paths` projection of these detailed candidates.
