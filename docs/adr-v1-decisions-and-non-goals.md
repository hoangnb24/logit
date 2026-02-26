# ADR: V1 Decisions and Non-Goals for Agent-Queryable Centralized Data

Status: accepted for `bd-16g`  
Date: 2026-02-26  
Related:
- `bd-3lb` (epic)
- `bd-3a4` (architecture/contracts baseline)

## Context

`logit` is moving from normalization-only outputs to a centralized, queryable data plane that agents can use to answer arbitrary usage and performance questions. Without explicit V1 decisions, implementation streams can diverge on query shape, output contract, refresh model, and data retention tradeoffs.

This ADR freezes the V1 scope decisions and explicitly records non-goals so implementation can proceed deterministically.

## Decision

V1 is locked to the following choices:

1. Query interface: SQL passthrough with read-only guardrails.
2. Output contract: JSON-only responses for machine-first interoperability.
3. Freshness model: manual refresh (operator/agent initiated), not background live ingestion.
4. Data fidelity default: full-fidelity retention in the centralized layer by default.

## Rationale

### 1. SQL passthrough

- Arbitrary questions are hard to anticipate as fixed KPIs/endpoints.
- SQL passthrough keeps analytical flexibility while guardrails enforce safety.
- This aligns with the project goal: agent self-service without constant product-surface expansion.

### 2. JSON-only outputs

- Primary consumers are agents and automation pipelines.
- A single machine-stable envelope simplifies parsing, validation, tests, and contracts.
- Avoids dual maintenance burden and semantic drift between human and machine renderings.

### 3. Manual refresh

- Deterministic explicit refresh points simplify debugging and reproducibility.
- Avoids premature complexity from daemons/watchers/schedulers during V1.
- Keeps freshness state observable through explicit ingest run artifacts.

### 4. Full-fidelity default

- Analytical workflows (quality, durations, session behavior, tool usage) need maximal detail.
- Default truncation/aggregation can destroy evidence needed for debugging and post-hoc analysis.
- Privacy controls remain available, but the baseline pipeline optimizes for diagnostic completeness.

## Explicit V1 Non-Goals

The following are out of scope for V1:

1. Live tailing or continuous background ingestion.
2. Non-JSON output modes as first-class query interfaces.
3. Hardcoded KPI-only query endpoints replacing SQL exploration.
4. Aggressive lossy summarization as the default ingest behavior.
5. Multi-tenant remote service operation; V1 remains local-first CLI-centric.

## Consequences

### Positive

- Fast path to agent-usable query capability with strong flexibility.
- Deterministic operation model suitable for reproducible debugging.
- Clear contracts for downstream beads (guardrails, schema/catalog, semantic views, benchmarks).

### Tradeoffs

- Manual refresh can produce stale results between refresh runs.
- SQL passthrough requires strict validator and bounded execution controls.
- Full-fidelity defaults can increase local storage footprint.

## Guardrails Required by This ADR

To satisfy the decision safely, implementation must provide:

1. Read-only SQL validation with explicit rejection of mutating/multi-statement payloads.
2. Stable JSON response/error envelopes with machine-parseable metadata.
3. Observable refresh reporting (counts, warnings, timing, watermark state).
4. Deterministic schema/version lifecycle and idempotent ingest semantics.

## Operational Interpretation for Agents

1. Run refresh before answering freshness-sensitive questions.
2. Treat JSON output as the only normative command contract.
3. Prefer exploratory SQL over requests for bespoke report endpoints unless explicitly needed.
4. Assume full-fidelity rows are available unless a run reports policy-driven redaction.

## Review Trigger

Revisit this ADR only if one or more of the following changes:

1. Requirement for continuous/live freshness in core workflows.
2. Requirement for human-first output contracts in primary agent paths.
3. Strong storage/privacy constraints that invalidate full-fidelity default.
