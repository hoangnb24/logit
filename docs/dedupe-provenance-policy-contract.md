# Dedupe and Provenance Policy Contract

Status: canonical for `bd-3jm`  
Applies to: `agentlog.v1` normalization and downstream dedupe engine (`bd-9ej`)

## 1. Purpose

Prevent duplicate inflation when multiple artifacts encode the same semantic event, while preserving full traceability back to every contributing source record.

## 2. Terms

- `raw_hash`: hash over raw source-record payload bytes (after canonical byte normalization).
- `canonical_hash`: hash over canonical semantic fields used for dedupe identity.
- `dedupe_key`: key used to decide whether records are equivalent.
- `provenance_entry`: one source-origin tuple attached to a normalized event.

## 3. Hash Material and Algorithms

v1 normative defaults:
- hash algorithm: `sha256`
- digest encoding: lowercase hex
- JSON canonicalization for hash inputs: stable key order, UTF-8, no insignificant whitespace

`raw_hash` material:
- exact source record payload segment as ingested (after line-ending normalization)

`canonical_hash` material:
- tuple of normalized semantic fields:
  - `event_type`
  - `role`
  - normalized content payload (`content_text` or canonical empty marker)
  - `tool_name` / normalized tool payload when relevant
  - normalized timestamp bucket material (see §5 fallback keys)

## 4. Dedupe Identity Hierarchy

The engine MUST use this priority order:

1. `canonical_hash` exact match
2. If missing/unstable canonical material, fallback key level A:
   - `(source_kind, conversation_id, turn_id, role, normalized_content_hash)`
3. Fallback key level B:
   - `(source_kind, source_path, source_record_locator)`
4. No match => new record

Never dedupe on `raw_hash` alone across different adapters, because wrappers and metadata drift can produce semantically identical events with different raw payloads.

## 5. Timestamp-Aware Fallback Strategy

When timestamp precision differs:
- compute `timestamp_bucket_ms` at 1-second granularity for fallback matching
- include `timestamp_bucket_ms` in fallback level A only when timestamp_quality is `exact` or `derived`
- omit timestamp from fallback comparisons for `fallback` quality timestamps to avoid false non-matches

## 6. Merge Semantics on Match

When a duplicate is detected:

1. Keep a single primary normalized record.
2. Preserve deterministic winner selection order:
   - higher timestamp quality (`exact` > `derived` > `fallback`)
   - richer structured metadata count
   - stable lexical tie-break on `event_id`
3. Append all source origins into provenance list (`provenance_entries`).
4. Increment `dedupe_count` and preserve `dedupe_members` IDs for auditability.

No source origin may be discarded.

## 7. Provenance Contract

Each emitted normalized event MUST carry:
- `source_kind`
- `source_path`
- `source_record_locator`
- `raw_hash`
- `adapter_name`
- `adapter_version` (if available)

If merged:
- `provenance_entries`: array of all contributing source tuples
- `dedupe_count`: integer >= 1
- `dedupe_strategy`: one of `canonical_hash`, `fallback_a`, `fallback_b`

## 8. Guardrails

1. Cross-adapter dedupe is allowed only through canonical/fallback semantic keys, not filesystem locators.
2. Empty or missing text must use canonical empty marker in hash material to avoid language-dependent null handling.
3. Dedupe must be deterministic across repeated runs on identical input and config.
4. Any uncertain merge (conflicting strong signals) should prefer no-merge + warning.

## 9. Validator Hooks

Validation should assert:
- `dedupe_count` matches size of `provenance_entries` where applicable.
- `dedupe_strategy` is present when `dedupe_count > 1`.
- no duplicate `event_id` remains post-dedupe in final output.

## 10. Compatibility

- This policy is `agentlog.v1` normative behavior.
- Changes to key hierarchy, winner selection rules, or hash material are breaking semantic changes and require versioned migration notes.
