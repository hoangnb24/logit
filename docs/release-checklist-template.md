# Release Checklist Template

Status: canonical release checklist for `bd-1fy`

Use this checklist before tagging or publishing a release candidate.

## 1. Scope and Inputs

- [ ] Release identifier chosen (`vX.Y.Z` or `rc-*`)
- [ ] Commit SHA captured
- [ ] Change summary prepared (major features/fixes/risk areas)
- [ ] Runtime assumptions documented (`home_dir`, `out_dir`, fixture/source roots)

## 2. Mandatory Quality Gates

Run and capture command output references:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

# UBS merge gate on changed Rust/TOML files in release diff
changed=$(git diff --name-only <BASE_SHA>...HEAD -- '*.rs' '*.toml')
[ -z "$changed" ] || ubs --ci --fail-on-warning $changed
```

- [ ] `cargo fmt --check` passed
- [ ] `cargo check --workspace --all-targets` passed
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passed
- [ ] `cargo test --workspace --all-targets` passed
- [ ] UBS changed-files gate passed (or no Rust/TOML changes in release diff)

## 3. Workflow Evidence (Snapshot → Normalize → Validate)

Recommended sequence:

```bash
OUT_DIR=/tmp/logit-release-check

cargo run -p logit -- --out-dir "$OUT_DIR" snapshot --source-root "$(pwd)" --sample-size 5
cargo run -p logit -- --out-dir "$OUT_DIR" normalize --source-root "$(pwd)"
cargo run -p logit -- --out-dir "$OUT_DIR" validate "$OUT_DIR/events.jsonl" --strict
```

- [ ] Snapshot completed and produced:
  - [ ] `snapshot/index.json`
  - [ ] `snapshot/samples.jsonl`
  - [ ] `snapshot/schema_profile.json`
- [ ] Normalize completed and produced:
  - [ ] `events.jsonl`
  - [ ] `agentlog.v1.schema.json`
  - [ ] `stats.json`
  - [ ] `discovery/sources.json`
  - [ ] `discovery/zsh_history_usage.json`
- [ ] Validate completed and produced:
  - [ ] `validate/report.json`

## 3.1 Centralized Data Plane Workflow Evidence (Ingest + Query)

Recommended sequence:

```bash
OUT_DIR=/tmp/logit-release-check

# Materialize mart + ingest metadata
cargo run -p logit -- --out-dir "$OUT_DIR" ingest refresh

# Query guardrail/surface smoke checks
cargo run -p logit -- --out-dir "$OUT_DIR" query sql "select 1 as value"
cargo run -p logit -- --out-dir "$OUT_DIR" query schema
cargo run -p logit -- --out-dir "$OUT_DIR" query catalog

# Answerability harness
cargo run -p logit -- --out-dir "$OUT_DIR" query benchmark \
  --corpus "$(pwd)/fixtures/benchmarks/answerability_question_corpus_v1.json"
```

- [ ] `ingest refresh` completed with JSON envelope success (`ok=true`)
- [ ] Ingest report artifact exists:
  - [ ] `ingest/report.json`
- [ ] Query surface smoke checks completed:
  - [ ] `query sql` success envelope contains deterministic metadata
  - [ ] `query schema` reports table/view inventory
  - [ ] `query catalog` reports semantic concepts/relations
- [ ] Benchmark harness completed and produced:
  - [ ] `benchmarks/answerability_report_v1.json`
  - [ ] per-question pass/fail outcomes are present
  - [ ] aggregate `score_pct` is present

Release-policy notes:
- Freshness-sensitive acceptance must reference ingest metadata (`ingest_runs`, `ingest_watermarks`), not wall-clock assumptions alone.
- If benchmark release gates are configured, record threshold policy and final gate decision in section 5.

## 4. Acceptance Signals

- [ ] `validate/report.json` status is acceptable for target release policy
- [ ] Critical invariants verified:
  - [ ] schema validity
  - [ ] timestamp quality distribution reviewed
  - [ ] dedupe behavior reviewed (duplicates removed + provenance)
  - [ ] adapter/source contribution counts reviewed
- [ ] Known warnings/errors triaged and disposition recorded

## 5. Release Record (Fill In)

- Release ID:
- Commit SHA:
- Evidence root (`OUT_DIR`):
- Gate summary:
  - fmt:
  - check:
  - clippy:
  - test:
  - validate status:
- Risk notes:
- Go/No-Go:
- Benchmark gate policy (if used):
  - threshold:
  - observed score:
  - gate result:
  - justification:

## 6. Acceptance Evidence Template (Fill In)

Use this table to prove each acceptance criterion with concrete evidence references.

| Acceptance Criterion | Verification Command / Source | Evidence Location | Result | Notes / Deviations |
| --- | --- | --- | --- | --- |
| CLI parses and routes commands correctly | `cargo test --test cli_surface` | `<link-to-test-log-or-artifact>` | Pass / Fail |  |
| Snapshot artifacts are emitted and parseable | `cargo test --test snapshot_artifacts --test snapshot_redaction` | `<link-to-test-log-or-artifact>` | Pass / Fail |  |
| Normalize emits canonical artifacts and discovery evidence | `cargo test --test normalize_orchestrator --test normalize_artifacts` | `<link-to-test-log-or-artifact>` | Pass / Fail |  |
| Validate report includes schema/invariant outcomes and stable exit behavior | `cargo test --test validate_artifacts --test validate_invariants --test validate_schema --test cli_progress` | `<link-to-test-log-or-artifact>` | Pass / Fail |  |
| End-to-end workflow remains consistent | `cargo test --test workflow_integration` | `<link-to-test-log-or-artifact>` | Pass / Fail |  |
| Ingest/query centralized data plane contract is stable | `cargo test --test cli_progress --test exit_code_contract` | `<link-to-test-log-or-artifact>` | Pass / Fail | Verify `query.sql`, `query.schema`, `query.catalog`, and `query.benchmark` envelope contracts. |
| Answerability benchmark artifact is emitted and parseable | `cargo run -p logit -- --out-dir <OUT_DIR> query benchmark --corpus <CORPUS_PATH>` | `<OUT_DIR>/benchmarks/answerability_report_v1.json` | Pass / Fail | Capture `score_pct`, per-domain summaries, and failed question IDs. |
| Release gate decision is explicitly recorded | `<team gate procedure>` | `<link-to-gate-record>` | Pass / Fail | Record threshold policy and final Go/No-Go rationale. |
| Workspace quality gates hold | `cargo fmt --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets` | `<link-to-gate-log-or-ci-run>` | Pass / Fail |  |
| UBS changed-files gate holds | `ubs --ci --fail-on-warning <changed-rust-toml-files>` | `<link-to-ubs-log-or-ci-run>` | Pass / Fail |  |

### Evidence Summary

- Release ID:
- Evidence reviewed by:
- Outstanding risks accepted:
- Follow-up issues created:
