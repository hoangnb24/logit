# CLI Command Examples and Output Expectations

Status: canonical for `bd-1x3`  
Companion docs:
- `docs/cli-flag-parity-matrix.md`

## 1. Common Invocation Pattern

Global runtime flags (when needed) precede subcommand:

```bash
logit --home-dir /home/me --cwd /work/repo --out-dir /tmp/logit-out <subcommand> [flags]
```

If omitted:
- `home_dir` defaults to `$HOME`
- `cwd` defaults to process current directory
- `out_dir` defaults to `<home_dir>/.logit/output`

## 2. `snapshot` Examples

Example:

```bash
logit snapshot --source-root /work/repo --sample-size 5
```

Expected behavior:
- command parses successfully
- snapshot config is constructed with sample size `5`
- runtime output path context is resolved deterministically

## 3. `normalize` Examples

Example:

```bash
logit --out-dir /tmp/logit-out normalize --source-root /work/repo --fail-fast
```

Expected behavior:
- command parses successfully
- normalize plan is created with `fail_fast=true`
- schema artifact path is derived under resolved output directory

## 4. `inspect` Examples

Example:

```bash
logit inspect /tmp/logit-out/events.jsonl --json
```

Expected behavior:
- target path is captured as inspect input
- JSON output mode toggle is enabled (`--json`)
- no runtime output path resolution required for inspect execution

## 5. `validate` Examples

Example:

```bash
logit validate /tmp/logit-out/events.jsonl --strict
```

Expected behavior:
- input path is captured as validation target
- strict mode toggle is enabled (`--strict`)
- validation mode selection is deterministic (`Strict` vs `Baseline`)
- machine-readable report artifact is written to resolved output layout at `validate/report.json`

## 6. Error/Failure Expectations

Representative failures:
- invalid runtime path inputs (for commands that consume runtime paths) produce explicit errors
- missing required positional args (`inspect`, `validate`) fail during argument parsing
- unknown flags fail argument parsing with clap-generated usage guidance

Process exit code contract:
- `0`: success
- `1`: runtime failure (I/O, path/config resolution, or command execution failure)
- `2`: validation failure (`validate` found invalid records)
- `64`: usage/argument parsing failure

## 7. Persona Workflow Recipes

The recipes below map user goals to concrete command flows, expected artifacts, and fast triage checks.

### 7.1 Debugger Persona

Goal:
- explain why a normalize/validate run failed and isolate the bad source quickly

Command sequence:

```bash
# 1) Capture local source shape safely
logit --out-dir /tmp/logit-out snapshot --source-root /work/repo --sample-size 5

# 2) Run normalize in default (non-fail-fast) mode to keep partial output
logit --out-dir /tmp/logit-out normalize --source-root /work/repo

# 3) Run strict validation to surface contract breaks
logit --out-dir /tmp/logit-out validate /tmp/logit-out/events.jsonl --strict
```

Expected artifacts:
- `/tmp/logit-out/snapshot/index.json`
- `/tmp/logit-out/snapshot/samples.jsonl`
- `/tmp/logit-out/events.jsonl`
- `/tmp/logit-out/stats.json`
- `/tmp/logit-out/validate/report.json`

Troubleshooting tips:
- if normalize prints adapter health errors/warnings, use those adapter/path diagnostics first
- use `snapshot/samples.jsonl` to inspect representative malformed rows before adapter-level debugging
- use strict-mode findings in `validate/report.json` to distinguish schema failures vs semantic invariant failures

### 7.2 Analyst Persona

Goal:
- produce stable normalized data for downstream analysis and quality checks

Command sequence:

```bash
# 1) Normalize from a known source root
logit --out-dir /tmp/logit-out normalize --source-root /work/repo

# 2) Validate in baseline mode for routine quality checks
logit --out-dir /tmp/logit-out validate /tmp/logit-out/events.jsonl

# 3) Inspect event stream shape quickly
logit inspect /tmp/logit-out/events.jsonl --json
```

Expected artifacts:
- `/tmp/logit-out/events.jsonl`
- `/tmp/logit-out/agentlog.v1.schema.json`
- `/tmp/logit-out/stats.json`
- `/tmp/logit-out/discovery/sources.json`
- `/tmp/logit-out/validate/report.json`

Troubleshooting tips:
- verify `stats.json` `counts.records_emitted` matches analysis expectations before loading into other tools
- use `discovery/sources.json` when expected adapter paths are missing from the run
- if validation fails, use line-level report diagnostics to isolate bad records early

### 7.3 Maintainer Persona

Goal:
- verify release readiness and deterministic behavior before landing changes

Command sequence:

```bash
# 1) Run quality gates
cargo fmt --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

# 1b) UBS gate on changed Rust/TOML files (blocking)
changed=$(git diff --name-only --cached -- '*.rs' '*.toml')
[ -z "$changed" ] || ubs --ci --fail-on-warning $changed

# 1c) Optional baseline UBS audit (advisory; create follow-up beads for broad findings)
ubs --ci --fail-on-warning .

# 2) Run an end-to-end artifact pass
logit --out-dir /tmp/logit-out snapshot --source-root /work/repo --sample-size 5
logit --out-dir /tmp/logit-out normalize --source-root /work/repo
logit --out-dir /tmp/logit-out validate /tmp/logit-out/events.jsonl --strict
```

Expected artifacts:
- full artifact set under `/tmp/logit-out` (`snapshot`, `discovery`, normalize artifacts, `validate/report.json`)
- deterministic command summaries/checkpoints in CLI output

Troubleshooting tips:
- compare emitted artifact topology against `docs/run-artifact-topology-contract.md`
- use `docs/troubleshooting-and-failure-cookbook.md` for known failure classes and recovery paths
- if behavior changed intentionally, update this document and `README.md` in the same patch
- if UBS baseline audits surface broad legacy findings, keep the changed-files gate green and file follow-up beads

### 7.4 Agent Query Persona (Centralized Data Plane)

Goal:
- answer usage/performance/freshness/reliability questions from the local SQLite mart with deterministic JSON envelopes

Command sequence:

```bash
# 1) Refresh the mart explicitly (freshness is manual in v1)
logit ingest refresh

# 2) Start with a conservative row cap for autonomous loops
logit query sql "SELECT tool_name, COUNT(*) AS invocation_count
FROM v_tool_calls
WHERE call_timestamp_unix_ms >= ((strftime('%s','now') - 7*24*60*60) * 1000)
GROUP BY tool_name
ORDER BY invocation_count DESC, tool_name ASC
LIMIT 20" --row-cap 200

# 3) Troubleshoot shape/perf with EXPLAIN QUERY PLAN (still read-only and guardrail-allowed)
logit query sql "EXPLAIN QUERY PLAN
SELECT tool_name, COUNT(*) AS invocation_count
FROM v_tool_calls
WHERE call_timestamp_unix_ms >= ((strftime('%s','now') - 7*24*60*60) * 1000)
GROUP BY tool_name
ORDER BY invocation_count DESC, tool_name ASC
LIMIT 20"
```

Expected query envelope checks:
- `ok=true`, `command="query.sql"`
- `meta.row_count`, `meta.truncated`, `meta.row_cap`, `meta.duration_ms`, `meta.params_count`
- `meta.diagnostics.statement_kind`, `meta.diagnostics.latency_bucket`
- `meta.diagnostics.returned_rows`, `meta.diagnostics.truncation_reason`

Troubleshooting tips:
- if `error.code="query_mart_unavailable"`, run `logit ingest refresh` first (or verify `--out-dir`)
- if `meta.truncated=true`, narrow predicates/time windows before increasing `--row-cap`
- if `meta.diagnostics.likely_full_scan=true`, add `WHERE`/`LIMIT` or switch to an aggregate view
- treat freshness answers as ingest-metadata questions (`ingest_runs`, `ingest_watermarks`), not wall-clock guesses

### 7.5 Agent Prompt + SQL Template Pack (V1)

Use these templates when an agent needs a fast, deterministic first answer. Start with `--row-cap 200` unless the expected result is a very small scalar/table.

#### A. Usage: Top Tools Over Last 7 Days

Prompt template:
- "Answer: which tools were invoked most frequently in the last 7 days. Return `tool_name` and `invocation_count`, ordered descending. Use `v_tool_calls`, and explain if results are truncated."

SQL template:

```bash
logit query sql "SELECT
  COALESCE(tool_name, '(unknown)') AS tool_name,
  COUNT(*) AS invocation_count
FROM v_tool_calls
WHERE call_event_id IS NOT NULL
  AND call_timestamp_unix_ms >= ((strftime('%s','now') - 7*24*60*60) * 1000)
GROUP BY COALESCE(tool_name, '(unknown)')
ORDER BY invocation_count DESC, tool_name ASC
LIMIT 25" --row-cap 200
```

#### B. Performance: Slow-Call Percentage by Adapter

Prompt template:
- "Compute the percentage of tool calls above 2000ms by adapter (`slow_call_pct`) using `v_tool_calls`. Ignore rows without durations and include the denominator count for debugging."

SQL template:

```bash
logit query sql "SELECT
  adapter_name,
  COUNT(*) AS measured_call_count,
  ROUND(
    100.0 * SUM(CASE WHEN duration_ms > 2000 THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0),
    2
  ) AS slow_call_pct
FROM v_tool_calls
WHERE duration_ms IS NOT NULL
GROUP BY adapter_name
ORDER BY slow_call_pct DESC, adapter_name ASC" --row-cap 200
```

#### C. Freshness: Stale Sources and Staleness Age

Prompt template:
- "List currently stale sources with `source_key`, `source_kind`, `staleness_state`, and approximate `staleness_age_ms` derived from `refreshed_at_utc`. Sort stalest first."

SQL template:

```bash
logit query sql "SELECT
  source_key,
  source_kind,
  staleness_state,
  CAST((julianday('now') - julianday(refreshed_at_utc)) * 86400000 AS INTEGER) AS staleness_age_ms,
  refreshed_at_utc
FROM ingest_watermarks
WHERE staleness_state = 'stale'
ORDER BY staleness_age_ms DESC, source_key ASC" --row-cap 200
```

#### D. Reliability: Fallback Timestamp Quality by Adapter

Prompt template:
- "Show timestamp-quality reliability by adapter. Return fallback timestamp counts and total events so I can estimate data quality risk."

SQL template:

```bash
logit query sql "SELECT
  adapter_name,
  SUM(CASE WHEN timestamp_quality = 'fallback' THEN event_count ELSE 0 END) AS fallback_timestamp_count,
  SUM(event_count) AS total_events
FROM v_quality
GROUP BY adapter_name
ORDER BY fallback_timestamp_count DESC, adapter_name ASC" --row-cap 200
```

#### E. Freshness/Operations: Recent Ingest Success Rate

Prompt template:
- "Compute refresh success rate over the last 30 ingest runs and include counts by status for debugging."

SQL template:

```bash
logit query sql "WITH recent AS (
  SELECT status
  FROM ingest_runs
  ORDER BY started_at_utc DESC, ingest_run_id DESC
  LIMIT 30
)
SELECT
  COUNT(*) AS run_count,
  SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) AS success_count,
  ROUND(100.0 * SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) / NULLIF(COUNT(*), 0), 2)
    AS refresh_success_rate_pct,
  SUM(CASE WHEN status = 'partial_failure' THEN 1 ELSE 0 END) AS partial_failure_count,
  SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed_count
FROM recent" --row-cap 50
```

### 7.6 Operator Rollout Recipe (Answerability Gate Evidence)

Goal:
- produce release-review evidence showing current ingest freshness context and answerability benchmark outcomes

Command sequence:

```bash
OUT_DIR=/tmp/logit-release-check
CORPUS="$(pwd)/fixtures/benchmarks/answerability_question_corpus_v1.json"

# 1) Refresh the mart and capture ingest evidence
logit --out-dir "$OUT_DIR" ingest refresh

# 2) Run benchmark harness for corpus-wide answerability scoring
logit --out-dir "$OUT_DIR" query benchmark --corpus "$CORPUS"

# 3) Optional: inspect benchmark artifact directly
logit inspect "$OUT_DIR/benchmarks/answerability_report_v1.json" --json
```

Evidence to capture in release notes/checklist:
- benchmark artifact path: `$OUT_DIR/benchmarks/answerability_report_v1.json`
- aggregate score: `summary.score_pct`
- per-domain scores: `summary.per_domain[*].score_pct`
- failed questions (if any): `questions[*]` where `passed=false`
- ingest recency context: latest `ingest_runs` status and watermark staleness state

Troubleshooting tips:
- if benchmark fails with `query_benchmark_corpus_invalid`, validate corpus path/schema first
- if benchmark fails with `query_mart_unavailable`, run `ingest refresh` and verify `--out-dir`
- if many questions fail due `answer_contract_mismatch`, inspect missing fields/order checks before raising row caps
- treat stale watermark states as release-risk signals even when benchmark score is otherwise high

## 9. Maintainer Notes

- These examples are intended as stable CLI contract guidance.
- Any flag or positional changes must update:
  - `docs/cli-flag-parity-matrix.md`
  - this examples document
  - CLI parse tests (`crates/logit/tests/cli_surface.rs`)
