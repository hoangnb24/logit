# logit Architecture and Data Model

Status: canonical architecture overview for `bd-ubx`  
Related contracts:
- `docs/agentlog-v1-contract.md`
- `docs/agentlog-v1-field-optionality-matrix.md`
- `docs/controlled-vocabulary-contract.md`
- `docs/dedupe-provenance-policy-contract.md`
- `docs/privacy-defaults-contract.md`
- `docs/timestamp-normalization-contract.md`
- `docs/run-artifact-topology-contract.md`
- `docs/discovery-path-precedence.md`

## 1. Purpose and Architectural Principles

`logit` is a local-first CLI that transforms heterogeneous agent artifacts into deterministic, inspectable, and validateable outputs.

Primary principles:
- determinism over convenience (stable ordering, stable output paths, explicit tie-breakers)
- observability over silent behavior (warnings and report artifacts over hidden fallbacks)
- canonical contracts over adapter-specific drift (`agentlog.v1` as single normalization target)
- safety defaults in snapshot mode (redaction + truncation), while preserving normalize fidelity

## 2. Pipeline Topology

The runtime pipeline has four operator-facing stages:

1. `snapshot`  
Produces evidence-oriented source summaries and representative samples.

2. `normalize`  
Fans in adapter outputs and emits canonical `agentlog.v1` artifacts.

3. `validate`  
Applies schema + invariant checks and emits machine-readable validation reports.

4. `inspect`  
Provides a read-only inspection entrypoint (baseline CLI surface).

Optional persistence stage:
- SQLite mirror + parity checks to support queryable local analytics while preserving canonical parity.

## 3. Module Boundaries

| Module | Responsibility | Key outputs |
|---|---|---|
| `crates/logit/src/cli` | argument parsing, command routing, runtime-flag plumbing | stable command surface (`snapshot`, `normalize`, `inspect`, `validate`) |
| `crates/logit/src/config` | runtime path resolution (`home_dir`, `cwd`, `out_dir`) | deterministic path context |
| `crates/logit/src/discovery` | known-path registry, source classification, history-informed prioritization | `discovery/sources.json`, `discovery/zsh_history_usage.json` |
| `crates/logit/src/adapters` | source-specific parsing and canonical mapping pre-normalize | adapter parse results + warnings |
| `crates/logit/src/snapshot` | source profiling, sample extraction, redaction/truncation | `snapshot/index.json`, `snapshot/samples.jsonl`, `snapshot/schema_profile.json` |
| `crates/logit/src/normalize` | orchestrator fan-in, dedupe/sort, schema + stats emission | `events.jsonl`, `agentlog.v1.schema.json`, `stats.json` |
| `crates/logit/src/validate` | schema/invariant checks and severity policy | `validate/report.json` |
| `crates/logit/src/sqlite` | SQLite schema, writer, parity verification | local DB mirror + parity report inputs |
| `crates/logit/src/models` | canonical `agentlog.v1` Rust types + schema generation | canonical type contracts |
| `crates/logit/src/utils` | cross-cutting helpers (time/hash/content/redaction/history) | deterministic helper primitives |

## 4. Adapter Strategy

### 4.1 Discovery and Selection

- Each adapter exposes known default source roots (sessions/history/logs/config).
- Ordered per-adapter candidate paths and precedence values are specified in `docs/discovery-path-precedence.md`.
- Discovery applies deterministic precedence and optional zsh-history scoring.
- Filtering supports adapter, source-kind, and path-substring selection.

### 4.2 Parsing Philosophy

- Adapter parsers are resilient to malformed rows/records.
- Invalid or partial records produce warnings; parse continues where possible.
- Structured source-specific data that does not map to canonical top-level fields is preserved in metadata fields (or adapter-specific parse result structs before final mapping).

### 4.3 Current Maturity

- Codex, Claude, Gemini, Amp, and OpenCode parsing components exist with fixture-backed tests.
- Normalize orchestrator currently consumes the implemented ingestion paths and surfaces unsupported paths as explicit non-fatal warnings (rather than silent omission).

## 5. Canonical Data Model (`agentlog.v1`)

`agentlog.v1` is the single normalized event contract shared across adapters and downstream stages.

### 5.1 Why a Canonical Model

- enables cross-agent comparison and analytics
- supports deterministic dedupe and global ordering
- avoids downstream logic coupling to source-specific JSON shapes

### 5.2 Event Identity and Provenance

Every normalized event retains:
- canonical identity (`event_id`, `run_id`, sequence fields)
- source provenance (`source_kind`, `source_path`, `source_record_locator`, optional source hash)
- adapter provenance (`adapter_name`, optional adapter version)
- integrity hashes (`raw_hash`, `canonical_hash`)

This allows auditability without requiring raw-source re-parsing.

### 5.3 Temporal and Ordering Semantics

Timestamps are normalized to:
- `timestamp_utc`
- `timestamp_unix_ms`
- `timestamp_quality` (`exact`, `derived`, `fallback`)

Ordering and dedupe behavior are defined by contract docs and implemented with deterministic tie-breakers.

### 5.4 Content and Structured Metadata

`content_text` and `content_excerpt` carry human-readable normalized content, while source-specific structure can remain in metadata where direct canonical promotion is not appropriate.

Examples of modeled structured behavior:
- Amp typed content parts and file-change telemetry
- OpenCode part/message joins with orphan tracking
- validate per-agent summaries and invariant diagnostics

## 6. Artifact Contract and Stage Outputs

Default output root: `<home_dir>/.logit/output` (or `--out-dir` override).

Expected artifacts:
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

These file/location guarantees are part of the run artifact topology contract.

## 7. Validation and Safety Model

Validation has two layers:

1. schema-level checks  
required fields, enum/value shape, JSON parseability.

2. invariant-level checks  
timestamp consistency, hash non-emptiness, semantic quality constraints.

Mode behavior:
- baseline mode allows certain quality issues as warnings
- strict mode escalates policy-sensitive issues to errors

Exit code behavior is deterministic and documented for automation safety.

## 8. How to Extend Safely

When adding a new adapter shape or stage capability:
- preserve deterministic ordering and output contracts
- add fixture-first tests for happy path + malformed path
- map source-specific fields conservatively to canonical model
- surface unsupported/malformed behavior as explicit warnings
- update relevant contract docs when semantics change

This keeps operational behavior reproducible and reviewable across contributors and automated agents.
