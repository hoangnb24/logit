# Timestamp Normalization Hierarchy and Ordering Contract

Status: canonical for `bd-1mo`  
Applies to: `logit` normalization pipeline (`agentlog.v1`)

## 1. Goal

Make timestamp handling deterministic across heterogeneous sources by defining:
- accepted timestamp input shapes
- source-priority hierarchy
- normalization to canonical UTC fields
- behavior for null/malformed timestamps
- total-order comparator for final output

## 2. Canonical Output Fields

Every normalized event must emit:
- `timestamp_utc` (RFC3339 UTC, ends with `Z`)
- `timestamp_unix_ms` (u64-equivalent epoch milliseconds)
- `timestamp_quality` (`exact` | `derived` | `fallback`)

For analytics, `timestamp_unix_ms` is the canonical sort/aggregation clock; `timestamp_utc` is its stable human-readable rendering.

## 3. Accepted Input Timestamp Shapes

v1 parser support:
1. RFC3339 / ISO-8601 strings with timezone offset
2. RFC3339 / ISO-8601 strings without offset (interpreted as local only if source contract explicitly says local; otherwise invalid)
3. Integer epoch seconds
4. Integer epoch milliseconds
5. Integer epoch microseconds
6. Integer epoch nanoseconds
7. Numeric strings representing any epoch unit above

Rejected inputs:
- ambiguous natural language dates
- locale-dependent date strings
- floating epoch values with fractional ambiguity

## 4. Source Priority Hierarchy

When multiple candidate timestamps are present, choose the first valid source in this order:

1. Event-native timestamp field (record-level authoritative timestamp)
2. Message-level created/sent timestamp
3. Tool call/result timestamp attached to the event
4. Session-level timestamp + deterministic local sequence derivation
5. File metadata fallback (`mtime`) if available
6. Run-level fallback anchor (`run_started_at_utc`) with deterministic sequence offset

Priority is strict and deterministic; lower-priority values are ignored once a higher-priority valid value is selected.

## 5. Unit and Timezone Normalization

Normalization steps:
1. Parse candidate according to recognized shape.
2. Infer epoch unit by magnitude when not explicit:
   - `< 1e11` => seconds
   - `< 1e14` => milliseconds
   - `< 1e17` => microseconds
   - otherwise nanoseconds
3. Convert to UTC instant.
4. Emit:
   - `timestamp_unix_ms` via floor conversion to milliseconds
   - `timestamp_utc` as RFC3339 UTC string at millisecond precision

## 6. Null and Malformed Handling

If all candidate timestamp sources are null/malformed:
- derive deterministic fallback:
  - anchor = `run_started_at_utc`
  - offset_ms = stable sequence index within source traversal
- set `timestamp_quality = fallback`
- emit warning code in record warnings list

If a parseable but lower-confidence source is used (for example, session-derived):
- set `timestamp_quality = derived`

If direct authoritative source parsed successfully:
- set `timestamp_quality = exact`

## 7. Deterministic Total Ordering

Final global sort comparator (ascending):

1. `timestamp_unix_ms`
2. `timestamp_quality_rank` where `exact < derived < fallback`
3. `source_kind`
4. `source_path`
5. `source_record_locator`
6. `sequence_source` (if present)
7. `canonical_hash`
8. `event_id`

Comparator must produce a strict total order (no nondeterministic ties).

## 8. Clock-Skew and Future/Ancient Guards

Validation thresholds (v1 defaults):
- reject/unusable as `exact` if timestamp is before `1970-01-01T00:00:00Z`
- mark warning if timestamp is > 24h in future relative to run start
- still emit event using fallback/derived semantics when needed

## 9. Provenance Requirements

Implementations must preserve timestamp provenance in metadata:
- selected source class (`event`, `message`, `tool`, `session`, `mtime`, `run_fallback`)
- raw input value (stringified) when safe
- parse outcome code

This metadata supports debugging and validator explainability.

## 10.1 Usage/Performance Semantics

When building usage/performance metrics from normalized events:
- treat `timestamp_quality` as a confidence signal, not just a parse detail
- preserve quality segmentation (`exact`, `derived`, `fallback`) in query/report outputs
- avoid mixing fallback-heavy cohorts with exact-only cohorts without explicit labeling
- use the global comparator from §7 before any duration or latency derivation

Recommended report dimensions:
- event counts by `timestamp_quality`
- percentage of fallback timestamps per adapter/session
- warning rates for malformed timestamp inputs

## 11. Compatibility

- This contract is `agentlog.v1` behavior.
- Changing priority order, unit inference, or comparator keys is a breaking semantic change and requires version bump documentation.
