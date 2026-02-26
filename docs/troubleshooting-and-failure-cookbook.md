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

## 9. Query SQL Diagnostics and Answerability Recovery

This section is for `logit query sql` troubleshooting once the SQLite mart workflow is in use.

### 9.1 Preflight Before Blaming SQL

1. Confirm the mart exists for the runtime path context:

```bash
ls -la "$OUT_DIR/mart.sqlite"
```

2. If missing (or suspiciously stale), run:

```bash
logit ingest refresh
```

3. Re-run the query with a conservative cap first:

```bash
logit query sql "SELECT 1 AS ok" --row-cap 10
```

If this succeeds, the query surface and mart path are healthy; the issue is likely SQL shape, params, or data expectations.

### 9.2 Query Error Envelopes: Common Codes and Actions

`query sql` returns JSON-only error envelopes. Check `error.code` first.

- `sql_guardrail_violation`
  - Cause: non-read-only SQL, unsupported statement form, or multi-statement payload.
  - Action: use exactly one statement and restrict to:
    - `SELECT ...`
    - `WITH ... SELECT ...`
    - `EXPLAIN SELECT ...`
    - `EXPLAIN QUERY PLAN SELECT ...`
  - Inspect `error.details.violation.reason` for `empty_statement`, `multi_statement`, `mutating_statement`, or `unsupported_statement`.

- `query_row_cap_invalid`
  - Cause: `--row-cap 0` (or otherwise invalid cap).
  - Action: use a positive integer (`--row-cap 200` is a good autonomous default).

- `query_params_invalid`
  - Cause: `--params` is not valid JSON or contains non-scalar entries.
  - Action: use scalar JSON (`42`, `"abc"`, `true`, `null`) or an array of scalars (`[1,"x",true]`).

- `query_mart_unavailable`
  - Cause: SQLite mart cannot be opened at the resolved `out_dir`.
  - Action: verify runtime paths and run `logit ingest refresh` to materialize `mart.sqlite`.

- `query_execution_failed`
  - Cause: SQL prepared/executed but failed at runtime (missing table/view/column, SQL syntax, type issue).
  - Action:
    1. Read `error.details.cause`.
    2. Reduce query to a minimal `SELECT` to isolate the failing expression.
    3. Use `EXPLAIN QUERY PLAN` on the simplified query after syntax is fixed.

### 9.3 Slow or Truncated Answers: Use `meta` Diagnostics

On success, inspect:
- `meta.duration_ms`
- `meta.truncated`
- `meta.row_cap`
- `meta.params_count`
- `meta.diagnostics.latency_bucket`
- `meta.diagnostics.likely_full_scan`
- `meta.diagnostics.truncation_reason`
- `meta.diagnostics.statement_kind`

#### If `meta.truncated=true`

Do this in order:
1. Narrow the time window (`WHERE ... timestamp ...`).
2. Reduce selected columns.
3. Aggregate earlier (`GROUP BY`) instead of returning raw rows.
4. Add deterministic ordering + `LIMIT`.
5. Only then raise `--row-cap`.

Why:
- v1 defaults prefer predictable latency/memory behavior over unbounded result sets.

#### If `duration_ms` is high or `latency_bucket` is `slow` / `very_slow`

1. Re-run with `EXPLAIN QUERY PLAN` to inspect access path shape.
2. Check `meta.diagnostics.likely_full_scan`; if `true`, add predicates or tighter limits.
3. Prefer semantic views (`v_tool_calls`, `v_sessions`, `v_adapters`, `v_quality`) over raw wide-table scans where possible.
4. Parameterize repeated templates with `--params` to keep SQL stable while changing filters.

### 9.4 Freshness and Reliability Questions: Query the Right Tables

When an agent asks "is this data fresh?" or "can I trust this analysis?", answer from ingest metadata first.

Useful tables:
- `ingest_runs` (run history/status/counters)
- `ingest_watermarks` (source-level freshness + staleness)

Quick checks:

```bash
# Most recent ingest runs
logit query sql "SELECT ingest_run_id, status, started_at_utc, finished_at_utc, events_written, warnings_count, errors_count
FROM ingest_runs
ORDER BY started_at_utc DESC, ingest_run_id DESC
LIMIT 10" --row-cap 50

# Current stale sources
logit query sql "SELECT source_key, source_kind, staleness_state, refreshed_at_utc
FROM ingest_watermarks
WHERE staleness_state != 'fresh'
ORDER BY refreshed_at_utc ASC, source_key ASC" --row-cap 200
```

Interpretation guidance:
- recent `partial_failure` / `failed` runs can invalidate downstream confidence even if SQL executes successfully
- `stale` watermarks mean answers may be structurally correct but operationally outdated
- elevated `warnings_count` / `errors_count` can indicate degraded reliability for trend or quality analyses

### 9.5 Agent Recovery Pattern When an Answer Is "Not Yet Good Enough"

If a first-pass query fails answerability:
1. Preserve the original user question verbatim.
2. Record the failed query + envelope (`error.code` or `meta` diagnostics).
3. Rephrase into a narrower sub-question (time window, adapter, session, or tool scope).
4. Re-run with a smaller `--row-cap`.
5. If still slow/truncated, switch to an aggregate/summary query first, then drill into one segment.

This keeps agent loops deterministic and avoids repeatedly widening query cost before basic shape correctness is confirmed.

This keeps follow-up deterministic and reproducible for the next agent.
