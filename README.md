# logit

`logit` is a Rust CLI for local, multi-agent log intelligence.

It discovers local agent artifacts (Codex, Claude, Gemini, Amp, OpenCode), produces safe snapshot evidence, normalizes events into canonical `agentlog.v1` JSONL, and validates output quality/contracts.

## Status

Current implementation includes:
- CLI surface: `snapshot`, `normalize`, `inspect`, `validate`
- Runtime/global flags: `--home-dir`, `--cwd`, `--out-dir`
- Canonical schema/model generation for `agentlog.v1`
- Snapshot artifact emission (`snapshot/index.json`, `snapshot/samples.jsonl`, `snapshot/schema_profile.json`)
- Normalize orchestration + artifact emission (`events.jsonl`, `agentlog.v1.schema.json`, `stats.json`)
- Validation report artifact emission (`validate/report.json`) with strict/baseline modes
- Optional SQLite schema/writer/parity support
- V1 agent-query data plane baseline contract (`docs/agent-query-data-plane-v1-contract.md`)

## Requirements

- Rust nightly toolchain (see `rust-toolchain.toml`, edition 2024)
- Cargo only (no alternate package manager)

## Build

```bash
cargo build --workspace
```

## Quickstart

Set an output directory (or omit to use `$HOME/.logit/output`):

```bash
OUT_DIR=/tmp/logit-out
```

Run snapshot:

```bash
cargo run -p logit -- --out-dir "$OUT_DIR" snapshot --source-root "$(pwd)" --sample-size 5
```

Run normalize:

```bash
cargo run -p logit -- --out-dir "$OUT_DIR" normalize --source-root "$(pwd)"
```

Run validate:

```bash
cargo run -p logit -- --out-dir "$OUT_DIR" validate "$OUT_DIR/events.jsonl" --strict
```

Inspect an artifact:

```bash
cargo run -p logit -- inspect "$OUT_DIR/events.jsonl" --json
```

## Command Guide

### Global runtime flags

These apply to `snapshot`, `normalize`, and `validate`:

- `--home-dir <PATH>`: overrides home directory for runtime path resolution
- `--cwd <PATH>`: overrides current working directory for relative path resolution
- `--out-dir <PATH>`: overrides artifact output root

Defaults when omitted:
- `home_dir`: `$HOME`
- `cwd`: process current directory
- `out_dir`: `<home_dir>/.logit/output`

### `snapshot`

```bash
logit snapshot --source-root /work/repo --sample-size 5
```

Behavior:
- discovers candidate sources by adapter
- profiles record/key structure and event-kind frequency
- writes snapshot artifacts under `<out_dir>/snapshot`
- applies snapshot redaction/truncation pipeline by default

### `normalize`

```bash
logit normalize --source-root /work/repo --fail-fast
```

Behavior:
- runs normalize orchestrator over prioritized discovered sources
- emits canonical artifacts in `<out_dir>`:
  - `events.jsonl`
  - `agentlog.v1.schema.json`
  - `stats.json`
- emits discovery evidence artifacts in `<out_dir>/discovery`:
  - `sources.json`
  - `zsh_history_usage.json`

Note:
- current orchestrator support is focused on implemented adapters (Codex + Claude event ingestion paths), while unsupported adapters are surfaced as non-fatal warnings.

### `validate`

```bash
logit validate /tmp/logit-out/events.jsonl --strict
```

Behavior:
- validates JSONL rows against generated `agentlog.v1` schema
- runs invariant checks (timestamps, hash integrity, content policy checks)
- writes machine-readable report artifact at `<out_dir>/validate/report.json`

### `inspect`

```bash
logit inspect /tmp/logit-out/events.jsonl --json
```

Behavior:
- baseline inspect command surface for read-only introspection entrypoint
- validates CLI parsing and target selection (`--json` output mode toggle)

### `ingest refresh`

```bash
logit ingest refresh
```

Behavior:
- materializes normalized `events.jsonl` into local SQLite mart (`mart.sqlite`)
- emits JSON envelope output only (success and failure paths)
- writes ingest report artifact at `<out_dir>/ingest/report.json`

### `query sql`

```bash
logit query sql "select event_type, count(*) as n from agentlog_events group by event_type"
```

Behavior:
- executes a single read-only SQL statement against the local mart
- enforces read-only guardrails (`SELECT`, `WITH ... SELECT`, `EXPLAIN ... SELECT`)
- returns JSON envelope with runtime metadata (`duration_ms`, `row_count`, `truncated`, `row_cap`, `params_count`)

Defaults and operator knobs:
- default `--row-cap` is `1000` (`--row-cap` must be greater than `0`)
- `--params <JSON>` supports scalar or array bound parameters
- tune for responsiveness by lowering `--row-cap` and narrowing SQL predicates before widening result scope

SLO and tuning baseline:
- canonical targets/defaults are defined in `docs/agent-query-data-plane-v1-contract.md` section 8.1

### `query schema`

```bash
logit query schema
```

Behavior:
- emits machine-readable table/view/column metadata for the local SQLite mart
- supports agent query planning without hardcoded schema assumptions
- use `--include-internal` to include internal schema objects (for debugging/migration inspection)

### `query catalog`

```bash
logit query catalog
```

Behavior:
- emits semantic catalog for agent-facing concepts (`tool_calls`, `sessions`, `adapters`, `quality`)
- includes recommended dimensions/metrics and join guidance for exploratory analysis
- use `--verbose` to include per-concept field catalogs

### `query benchmark`

```bash
logit query benchmark --corpus fixtures/benchmarks/answerability_question_corpus_v1.json
```

Behavior:
- executes the canonical answerability corpus deterministically across query interfaces (`query.sql` plans, plus schema/catalog preflight checks)
- validates per-question result shape against `expected_answer_contract.must_include` (and ordering contracts when present)
- emits JSON envelope output only, including per-question pass/fail and aggregate score summary
- writes benchmark artifact at `<out_dir>/benchmarks/answerability_report_v1.json`

Defaults and operator knobs:
- default corpus path resolves relative to `--cwd`: `fixtures/benchmarks/answerability_question_corpus_v1.json`
- default benchmark `--row-cap` is `200` (`--row-cap` must be greater than `0`)

### Freshness and Stale-Data Expectations (Centralized Query Workflow)

- `ingest refresh` is the only action that advances mart freshness in v1 (no background auto-refresh).
- Query commands (`query sql`, `query schema`, `query catalog`, `query benchmark`) operate on the current local mart snapshot under `--out-dir`.
- For freshness-sensitive answers or release sign-off:
  - run `ingest refresh` first
  - inspect ingest run/watermark metadata (`ingest/report.json`, `ingest_runs`, `ingest_watermarks`)
  - record the freshness context alongside benchmark/query evidence

Guardrail rationale:
- `query sql` is intentionally read-only and single-statement to keep automation safe and deterministic.
- bounded defaults (`--row-cap`) reduce latency and memory risk for unattended agent loops.
- when results are truncated, prefer narrower predicates/time windows before raising caps.

## Exit Codes

- `0`: success
- `1`: runtime failure (I/O, path/config resolution, command execution failure)
- `2`: validation failure (`validate` found invalid records)
- `64`: usage/argument parsing failure

## Artifact Layout (Default)

If `--out-dir` is omitted, artifacts are written under `$HOME/.logit/output`:

- normalize:
  - `events.jsonl`
  - `agentlog.v1.schema.json`
  - `stats.json`
- snapshot:
  - `snapshot/index.json`
  - `snapshot/samples.jsonl`
  - `snapshot/schema_profile.json`
- discovery:
  - `discovery/sources.json`
  - `discovery/zsh_history_usage.json`
- validate:
  - `validate/report.json`
- ingest:
  - `ingest/report.json`
  - `mart.sqlite`
- benchmark:
  - `benchmarks/answerability_report_v1.json`

## Quality Gates

Run these before landing substantial changes:

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo test --workspace --all-targets

# UBS merge gate: scan only changed Rust/TOML files
changed=$(git diff --name-only --cached -- '*.rs' '*.toml')
[ -z "$changed" ] || ubs --ci --fail-on-warning $changed
```

Use full-repository UBS scans (`ubs --ci --fail-on-warning .`) as advisory baseline audits.
When broad legacy findings appear, file follow-up beads instead of blocking unrelated changes.

## CI Matrix and Determinism

GitHub Actions workflow: `.github/workflows/ci.yml`

- Trigger: pull requests and pushes to `main` (plus manual `workflow_dispatch`)
- Quality matrix lanes (`ubuntu-latest`):
  - `cargo fmt --check`
  - `cargo check --workspace --all-targets --locked`
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`
- UBS lane (`ubuntu-latest`):
  - installs UBS and runs `ubs --ci --fail-on-warning` on changed `*.rs`/`*.toml` files only
- Test matrix lanes:
  - `cargo test --workspace --all-targets --locked -- --test-threads=1`
  - runs on `ubuntu-latest` and `macos-latest`
- Deterministic CI defaults:
  - `CARGO_INCREMENTAL=0`
  - `RUST_TEST_THREADS=1`
  - `TZ=UTC`
  - `RUST_BACKTRACE=1`

## Troubleshooting

For failure-mode runbooks (discovery gaps, parse failures, validation diagnostics), see:

- `docs/troubleshooting-and-failure-cookbook.md`
- `docs/cli-command-examples.md` (persona workflow recipes for debugger, analyst, maintainer)

## Architecture Contracts

- `docs/architecture-and-data-model.md` (current pipeline/module architecture)
- `docs/agent-query-data-plane-v1-contract.md` (v1 ingest/query baseline decisions and non-goals)

## Release Readiness

For reusable release-gate and acceptance-evidence templates, see:

- `docs/release-checklist-template.md`

## Repository Layout

- `crates/logit/src/cli` command-line parsing and dispatch
- `crates/logit/src/adapters` source-specific adapter parsers
- `crates/logit/src/discovery` known-path registry and source prioritization
- `crates/logit/src/snapshot` snapshot evidence generation
- `crates/logit/src/normalize` canonical normalization orchestration and artifacts
- `crates/logit/src/validate` schema/invariant validation and reports
- `crates/logit/src/sqlite` SQLite schema/writer/parity support
- `crates/logit/src/models` canonical `agentlog.v1` data model
- `crates/logit/src/utils` shared utilities (hashing, redaction, time, content, history)
# logit
