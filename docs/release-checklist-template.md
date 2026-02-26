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
```

- [ ] `cargo fmt --check` passed
- [ ] `cargo check --workspace --all-targets` passed
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passed
- [ ] `cargo test --workspace --all-targets` passed

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

## 6. Acceptance Evidence Template (Fill In)

Use this table to prove each acceptance criterion with concrete evidence references.

| Acceptance Criterion | Verification Command / Source | Evidence Location | Result | Notes / Deviations |
| --- | --- | --- | --- | --- |
| CLI parses and routes commands correctly | `cargo test --test cli_surface` | `<link-to-test-log-or-artifact>` | Pass / Fail |  |
| Snapshot artifacts are emitted and parseable | `cargo test --test snapshot_artifacts --test snapshot_redaction` | `<link-to-test-log-or-artifact>` | Pass / Fail |  |
| Normalize emits canonical artifacts and discovery evidence | `cargo test --test normalize_orchestrator --test normalize_artifacts` | `<link-to-test-log-or-artifact>` | Pass / Fail |  |
| Validate report includes schema/invariant outcomes and stable exit behavior | `cargo test --test validate_artifacts --test validate_invariants --test validate_schema --test cli_progress` | `<link-to-test-log-or-artifact>` | Pass / Fail |  |
| End-to-end workflow remains consistent | `cargo test --test workflow_integration` | `<link-to-test-log-or-artifact>` | Pass / Fail |  |
| Workspace quality gates hold | `cargo fmt --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets` | `<link-to-gate-log-or-ci-run>` | Pass / Fail |  |

### Evidence Summary

- Release ID:
- Evidence reviewed by:
- Outstanding risks accepted:
- Follow-up issues created:
