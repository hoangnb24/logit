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

## 8. Maintainer Notes

- These examples are intended as stable CLI contract guidance.
- Any flag or positional changes must update:
  - `docs/cli-flag-parity-matrix.md`
  - this examples document
  - CLI parse tests (`crates/logit/tests/cli_surface.rs`)
