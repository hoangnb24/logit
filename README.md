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
