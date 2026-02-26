# Agent-Queryable Data Plane V1 Baseline Contract

Status: canonical for `bd-3a4`  
Parent epic: `bd-3lb`  
Scope: ingest refresh and read-only query architecture for local `logit` analytics

## 1. Purpose

Freeze the v1 architecture decisions for a centralized, agent-queryable local data plane so downstream implementation streams can proceed without ambiguity.

This contract is the normative baseline for:
- ingest refresh lifecycle semantics
- query command and response semantics
- freshness and staleness model
- tool/time/identity interpretation requirements
- explicit v1 non-goals

## 2. Architectural Topology (V1)

Deterministic data flow:

1. Source artifacts are normalized into canonical `agentlog.v1` JSONL (`events.jsonl`).
2. `ingest refresh` materializes canonical records into a local SQLite mart plus ingest metadata.
3. `query` commands execute read-only access paths against the mart and return JSON-only envelopes.

V1 ingestion is explicitly pull-based (manual refresh), never background streaming.

## 3. Decision Lock (V1)

The following decisions are locked for v1:

1. SQL passthrough is supported for read-only statements only.
2. Query and ingest command outputs are JSON-only.
3. Data freshness advances only through explicit `ingest refresh` execution.
4. Full-fidelity defaults are preferred over lossy summarization in stored analytics rows.

Breaking any locked decision requires an explicit contract update and dependent issue re-planning.

## 4. Command Surface Baseline

V1 command namespace contract:
- `logit ingest refresh`
- `logit query sql`
- `logit query schema`
- `logit query catalog`
- `logit query benchmark`

Command behavior requirements:
- shared runtime flags must remain consistent with existing CLI path controls
- deterministic exit behavior must be preserved for automation
- command results must always be emitted in the shared JSON envelope family defined by this baseline

## 5. JSON Envelope Baseline

Every query/ingest command response must return exactly one top-level JSON object.

Success envelope baseline fields:
- `ok` (boolean, true)
- `command` (string)
- `generated_at_utc` (RFC3339 UTC)
- `data` (object or array payload)
- `meta` (object; stable machine-facing metadata, may include row counts, timings, freshness state)
- `warnings` (array of structured warning objects; empty when none)

Error envelope baseline fields:
- `ok` (boolean, false)
- `command` (string)
- `generated_at_utc` (RFC3339 UTC)
- `error` (object with stable `code`, `message`, optional `details`)
- `meta` (object)
- `warnings` (array)

Error code stability is part of the contract surface; silent ad hoc text errors are not allowed.

## 6. Freshness and Watermark Semantics

V1 freshness is defined by ingest metadata, not by wall-clock assumptions alone.

Baseline requirements:
- each refresh produces a durable ingest run record with start/end timestamps and status
- per-source watermarks capture the last successfully ingested frontier
- query responses include freshness metadata in `meta` sufficient for autonomous decision-making
- stale/unknown freshness states are explicit and machine-readable

No implicit auto-refresh behavior is allowed in v1.

## 7. Tool, Time, Identity, and Quality Semantics

This baseline locks interpretation rules that downstream contracts may refine but not contradict.

Tool semantics:
- tool records must preserve tool identity and correlation keys needed for call/result pairing
- derived tool analytics must carry provenance metadata when values are inferred

Time semantics:
- canonical event time remains `agentlog.v1` timestamp contract-driven
- duration derivation follows explicit precedence (explicit timestamps, paired derivation, heuristic fallback) with provenance/confidence carried forward

Identity semantics:
- session/conversation/turn grouping semantics must remain stable across normalize, mart materialization, and query layers
- adapter attribution and source provenance are mandatory for every materialized analytic fact

Quality markers:
- timestamp quality and derivation confidence markers must survive into queryable storage and envelopes
- degraded quality is surfaced as structured warnings instead of silent coercion

## 8. Safety, Determinism, and Operability Constraints

- query execution is read-only by contract
- schema and migration operations must be deterministic and idempotent for existing workspaces
- response ordering for deterministic query modes must be stable
- all contract breaches must produce machine-readable errors (not partial silent success)

### 8.1 Query SLO, Defaults, and Operator Tuning Baseline (V1)

These targets are the operator-facing responsiveness baseline for `logit query sql`.

SLO targets (release-gate expectations):
- p50 `duration_ms` <= 250ms for selective read-only queries on a warm local mart
- p95 `duration_ms` <= 1500ms for benchmark answerability queries at default limits
- no benchmark query should exceed 10_000ms without an explicit warning or tuning follow-up

Default execution limits:
- `row_cap` default is `1000` and must be `> 0`
- query surface accepts exactly one read-only statement (`SELECT`, `WITH ... SELECT`, `EXPLAIN ... SELECT`)
- responses must always include `meta.duration_ms`, `meta.row_count`, `meta.truncated`, `meta.row_cap`, and `meta.params_count`
- freshness remains manual-refresh driven (`ingest refresh`), never implicit background refresh

Operator tuning knobs (current):
- `--row-cap <N>`: trade completeness for latency/memory; reduce first when response time degrades
- `--params <JSON>`: prefer parameterized predicates over literal-heavy SQL to keep query templates stable for automation
- SQL-shape tuning: constrain projections, predicates, and sort scope (`WHERE`, narrow `SELECT`, explicit `LIMIT`) before raising caps

Implementation guidance:
- for autonomous loops, start with `--row-cap 200` and increase only when `truncated=true` blocks answerability
- treat `duration_ms > 2000` as an operability warning; tighten predicates/cap first, then consider index work
- when `truncated=true`, rerun with narrower predicate windows or deterministic pagination strategy instead of unbounded cap growth

### 8.2 Answerability Benchmark Harness Baseline (V1)

`logit query benchmark` is the canonical automated harness for question-corpus answerability checks.

Baseline requirements:
- default corpus path resolves to `fixtures/benchmarks/answerability_question_corpus_v1.json` relative to runtime `cwd`
- benchmark execution iterates question IDs deterministically and uses stable mapped query plans
- benchmark responses include per-question pass/fail diagnostics plus aggregate score summary
- benchmark artifact is written to `<out_dir>/benchmarks/answerability_report_v1.json` with schema version `logit.answerability-benchmark-report.v1`
- harness output is machine-readable and consumable by downstream release-gating tasks without reinterpretation
- benchmark artifact/report includes a machine-readable `release_gate` object with explicit threshold inputs, observed results, and final `passed` decision

V1 release-gate threshold baseline:
- `minimum_total_score_pct = 95.0`
- `minimum_domain_score_pct = 90.0`
- `maximum_failed_questions = 0`
- threshold failures must be reported as structured gate failure details (not inferred from prose)

## 9. Explicit V1 Non-Goals

Out of scope for v1:
- live tailing or streaming ingest
- background daemons or continuous refresh schedulers
- non-JSON output modes for ingest/query commands
- write-capable user query endpoints

These are potential post-v1 roadmap items and must not leak into v1 behavior.

## 10. Compatibility and Change Policy

- This contract is normative for v1 implementation streams blocked by `bd-3a4`.
- Additive clarifications are allowed if they do not alter locked decisions.
- Any semantic break requires explicit contract revision and dependent task impact review.
