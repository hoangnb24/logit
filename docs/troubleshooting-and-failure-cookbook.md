# Troubleshooting and Failure-Mode Cookbook

Status: canonical operator cookbook for `bd-3kf`

This guide is for diagnosing three common classes of problems:
- discovery gaps (expected sources/events missing)
- parse failures or partial ingestion warnings
- validation failures (`validate/report.json` errors/warnings)

The workflow is intentionally artifact-first and deterministic.

## 1. Fast Triage Sequence

Run the pipeline in stages and inspect artifacts after each step:

```bash
OUT_DIR=/tmp/logit-out

cargo run -p logit -- --out-dir "$OUT_DIR" snapshot --source-root "$(pwd)" --sample-size 5
cargo run -p logit -- --out-dir "$OUT_DIR" normalize --source-root "$(pwd)"
cargo run -p logit -- --out-dir "$OUT_DIR" validate "$OUT_DIR/events.jsonl"
```

Then inspect the machine-readable artifacts:

```bash
logit inspect "$OUT_DIR/snapshot/index.json" --json
logit inspect "$OUT_DIR/events.jsonl" --json
cat "$OUT_DIR/stats.json"
cat "$OUT_DIR/discovery/sources.json"
cat "$OUT_DIR/discovery/zsh_history_usage.json"
cat "$OUT_DIR/validate/report.json"
```

## 2. Where Signals Live

- Discovery selection evidence:
  - `discovery/sources.json`
  - `discovery/zsh_history_usage.json`
- Snapshot profiling + warnings:
  - `snapshot/index.json`
  - `snapshot/schema_profile.json`
  - `snapshot/samples.jsonl`
- Normalize output + aggregate counts:
  - `events.jsonl`
  - `stats.json`
- Validation status + line-level issues:
  - `validate/report.json`

CLI summaries are useful, but artifact files are the source of truth for diagnosis.

## 3. Discovery Gaps (Missing Sources or Unexpected Zero Coverage)

Symptoms:
- `normalize` emits fewer events than expected
- `snapshot/index.json` shows low `existing_sources` or `files_profiled`
- expected adapter paths are absent from `discovery/sources.json`

### Checks

1. Confirm the candidate paths exist under the runtime home/source root you are using.

```bash
ls -la ~/.codex ~/.claude ~/.gemini ~/.amp ~/.opencode
```

2. Confirm `--source-root` vs runtime `home_dir` assumptions.

- `--source-root` rewrites `~/...` candidate paths into the provided root for snapshot/normalize testing.
- Without `--source-root`, discovery resolves paths under `--home-dir` (or `$HOME`).

3. Inspect discovery evidence ordering and path coverage:

```bash
cat "$OUT_DIR/discovery/sources.json"
```

Look for:
- `total_sources`
- `adapter_counts`
- `sources[].path`
- `sources[].precedence`
- `sources[].history_score`

4. Inspect zsh history weighting evidence:

```bash
cat "$OUT_DIR/discovery/zsh_history_usage.json"
```

If `total_command_hits=0`, path ordering falls back to precedence-only ranking.

### Common Causes and Actions

- Cause: wrong `--source-root` or `--home-dir`
  - Action: rerun with explicit absolute paths.
- Cause: files exist but under a non-default layout
  - Action: place fixtures under documented discovery paths or extend discovery registry in code.
- Cause: unreadable directory
  - Action: check permissions; `snapshot`/`normalize` now warn and continue in default mode.
- Cause: unsupported adapter ingestion path in normalize orchestrator
  - Action: review `normalize` warnings; unsupported adapters are intentionally surfaced as non-fatal warnings in v1.

## 4. Parse Failures and Partial Ingestion Warnings

Symptoms:
- `normalize` succeeds but emits fewer rows than expected
- `normalize` prints non-zero `parse_warnings`
- `snapshot` reports warnings > 0

### Normalize: What to Read

- CLI summary:
  - `normalize: complete ... parse_warnings=<N> event_warnings=<N> event_errors=<N>`
- Aggregates:
  - `stats.json`
- Output sanity:
  - `logit inspect "$OUT_DIR/events.jsonl" --json`

Important behavior:
- Default normalize mode (`--fail-fast` omitted) prefers warnings + continuation.
- `--fail-fast` converts parse/collection failures into command failure.

Typical warning examples:
- `source path not found: ...`
- `adapter '<name>' source path unreadable '...': ...`
- `adapter '<name>' parse error for '...': ...`
- `adapter '<name>' not yet supported by normalize orchestrator; skipped '...'`

### Snapshot: What to Read

- `snapshot/index.json`
  - `counts.warnings`
  - top-level `warnings[]`
- `snapshot/schema_profile.json`
  - per-source `profiles[].warnings[]`

Typical snapshot warning cases:
- malformed JSON/JSONL rows in source files
- protobuf files indexed as metadata-only in v1 (`decode skipped`)
- unreadable source directories (warning + continuation)

### Actions

1. Isolate the suspect input file and inspect it directly:

```bash
logit inspect /path/to/source-or-artifact --json
```

2. If the source is JSONL, check invalid rows and line counts (`line_counts.invalid_json_rows`).
3. If output is too sparse, compare:
   - discovery source count (`discovery/sources.json`)
   - snapshot profiled counts (`snapshot/index.json`)
   - normalize emitted count (`stats.json`)
4. Re-run `normalize` with `--fail-fast` to force the first parser/collection failure to surface immediately.

## 5. Validation Failures (`validate/report.json`)

Symptoms:
- command exits non-zero with `validation failed with N error(s)`
- `validate: failed errors=<N> warnings=<N> next=inspect_report`

### Read the Report First

```bash
cat "$OUT_DIR/validate/report.json"
```

Key fields:
- `status`: `pass` | `warn` | `fail`
- `interpreted_exit_code`
- `total_records`
- `records_validated`
- `errors`
- `warnings`
- `per_agent_summary`
- `issues[]` (line-level diagnostics)

Issue categories:
- `invalid_json`
- `schema_violation`
- `invariant_violation`

Severity:
- `error`
- `warning`

### Baseline vs Strict

- Baseline mode can keep some data-quality issues as warnings.
- `--strict` escalates policy-sensitive issues to errors.

If a run passes in baseline but fails in strict:
1. Compare `issues[]` between the two modes.
2. Fix invariant-quality problems first (timestamps, empty hashes/content policy).
3. Re-run `validate --strict`.

## 6. Exit-Code and CLI Failure Interpretation

`logit` exit codes:
- `0`: success
- `1`: runtime failure (I/O, path/config, command execution)
- `2`: validation failure (`validate` found invalid records)
- `64`: usage / argument parsing failure

Fast interpretation:
- Exit `64`: CLI invocation problem (missing args/flags)
- Exit `1`: environment/path/file problem (permissions, missing files, invalid runtime path)
- Exit `2`: data quality/schema/invariant problem in `events.jsonl`

## 7. Practical Recovery Playbooks

### A. `normalize` emitted zero events

1. Check `discovery/sources.json` for expected paths.
2. Check `snapshot/index.json` to confirm files are parseable/profiled.
3. Inspect `normalize` warnings count and unsupported-adapter warnings.
4. Run:

```bash
logit inspect "$OUT_DIR/events.jsonl" --json
```

If `normalized_event_summary.normalized_rows=0`, the issue is upstream (discovery/parsing), not validation.

### B. `validate` fails on line N

1. Read `validate/report.json` and locate `issues[]` entry for `line=N`.
2. Inspect the normalized artifact around that line:

```bash
nl -ba "$OUT_DIR/events.jsonl" | sed -n 'N-2,N+2p'
```

3. If schema violation: compare the row with `agentlog.v1.schema.json`.
4. If invariant violation: inspect semantic fields (`timestamp_*`, hashes, content fields).

### C. Snapshot warnings are high, but normalize succeeds

This is often acceptable in v1 if warnings are from:
- malformed rows skipped during profiling
- protobuf metadata-only indexing
- unreadable optional directories

Action:
- review `snapshot/index.json` warnings
- verify normalize/validate outputs are still within expected quality thresholds

## 8. What to Include in a Bug Report / Hand-off

Include:
- exact command(s) run
- runtime path context (`--home-dir`, `--cwd`, `--out-dir`, `--source-root`)
- relevant CLI output lines
- `discovery/sources.json`
- `stats.json`
- `validate/report.json` (if validation issue)
- minimal offending source file sample (redacted if needed)

This keeps follow-up deterministic and reproducible for the next agent.
