# agentlog.v1 Canonical Field Semantics and Invariants

Status: canonical for `bd-nn2`  
Scope: `logit` normalize/snapshot pipeline for Codex, Claude, Gemini, Amp, and OpenCode inputs

## 1. Contract Goals

This document freezes the meaning of every canonical normalized field so adapter implementations, schema generation, normalization, and validation behave identically.

Core guarantees:
- deterministic record identity and ordering
- explicit provenance for every emitted event
- stable optionality semantics (no adapter-specific interpretation drift)
- machine-checkable vocabularies for key classifier fields

## 2. Record Shape Rules

- Output format is newline-delimited JSON (`.jsonl`), one object per line.
- Keys are `snake_case` ASCII.
- Unknown keys are forbidden in strict mode.
- Empty string is treated as invalid for identifier-like fields.
- `null` is not used for scalar fields; missing key means "unknown/not available".
- Arrays default to empty array when present; omit when not applicable.

## 3. Global Invariants

Every normalized record MUST satisfy:

1. `schema_version` is exactly `agentlog.v1`.
2. `event_id` is globally unique within a run.
3. `raw_hash` is a deterministic hash of the source record payload after canonical byte normalization.
4. `canonical_hash` is a deterministic hash of canonical semantic material (for dedupe) and MUST be stable across repeated runs on identical input.
5. `sequence_global` is strictly increasing for records in a single output file.
6. `timestamp_utc` and `timestamp_unix_ms` represent the same instant when both are present.
7. `record_format`, `event_type`, and `role` MUST be members of their controlled vocabularies.
8. Provenance trio (`source_kind`, `source_path`, `source_record_locator`) is always present.

## 4. Controlled Vocabularies

### 4.1 `record_format`

Allowed values:
- `message` - conversational or human/assistant text event
- `tool_call` - tool invocation request
- `tool_result` - tool execution response/result
- `system` - system/runtime/meta event
- `diagnostic` - logs/telemetry not treated as conversation content

### 4.2 `event_type`

Allowed values:
- `prompt`
- `response`
- `system_notice`
- `tool_invocation`
- `tool_output`
- `status_update`
- `error`
- `metric`
- `artifact_reference`
- `debug_log`

### 4.3 `role`

Allowed values:
- `user`
- `assistant`
- `system`
- `tool`
- `runtime`

## 5. Field Catalog

Legend:
- R = required in every record
- C = conditionally required
- O = optional

| Field | Type | Req | Semantics | Allowed values / constraints |
|---|---|---|---|---|
| `schema_version` | string | R | Canonical schema identifier. | Exactly `agentlog.v1`. |
| `event_id` | string | R | Deterministic event identifier for this normalized record. | Non-empty; unique per run. |
| `run_id` | string | R | Identifier of normalize execution producing this record. | Non-empty UUID/opaque string. |
| `sequence_global` | integer | R | Global deterministic sort index for output ordering. | Integer `>= 0`; unique per output file. |
| `sequence_source` | integer | O | Source-local ordering index if available. | Integer `>= 0`. |
| `source_kind` | string | R | High-level origin family of source data. | One of `codex`,`claude`,`gemini`,`amp`,`opencode`. |
| `source_path` | string | R | Filesystem path to source artifact used for this record. | Non-empty path string. |
| `source_record_locator` | string | R | Stable locator inside source artifact. | Example: `line:42`, `json_pointer:/events/3`. |
| `source_record_hash` | string | O | Raw hash of source-record slice before canonical mapping. | Lowercase hex digest. |
| `adapter_name` | string | R | Adapter emitting this record. | One of `codex`,`claude`,`gemini`,`amp`,`opencode`. |
| `adapter_version` | string | O | Adapter contract version used. | Semver string preferred. |
| `record_format` | string | R | Structural class of normalized record. | Controlled vocabulary in §4.1. |
| `event_type` | string | R | Semantic event classifier. | Controlled vocabulary in §4.2. |
| `role` | string | R | Actor role for semantic attribution. | Controlled vocabulary in §4.3. |
| `timestamp_utc` | string | R | Canonical event instant in UTC ISO-8601. | RFC 3339 UTC (`...Z`). |
| `timestamp_unix_ms` | integer | R | Epoch milliseconds equivalent to `timestamp_utc`. | Integer `>= 0`. |
| `timestamp_quality` | string | R | How timestamp confidence was derived. | One of `exact`,`derived`,`fallback`. |
| `session_id` | string | O | Source session identifier. | Non-empty string. |
| `conversation_id` | string | O | Conversation/thread identifier across events. | Non-empty string. |
| `turn_id` | string | O | Turn/message grouping identifier. | Non-empty string. |
| `parent_event_id` | string | O | Parent event relationship for threaded structures. | Must reference an existing `event_id` when present. |
| `actor_id` | string | O | Stable actor identity if source provides it. | Non-empty string. |
| `actor_name` | string | O | Human-readable actor label. | Non-empty UTF-8 string. |
| `provider` | string | O | LLM provider/vendor for the event. | Non-empty lowercase token when normalized. |
| `model` | string | O | Model identifier used for generation/tool step. | Non-empty string. |
| `content_text` | string | O | Full normalized textual content when policy permits retention. | UTF-8 text; may be omitted by policy. |
| `content_excerpt` | string | O | Deterministic short excerpt for preview/diagnostics. | Max length policy-defined; no multiline normalization drift. |
| `content_mime` | string | O | MIME-like descriptor for content payload. | Example: `text/plain`, `application/json`. |
| `tool_name` | string | C | Tool identifier for tool records. | Required when `record_format` in `{tool_call,tool_result}`. |
| `tool_call_id` | string | C | Stable tool-call correlation ID. | Required for `tool_call` and `tool_result` pairs when available. |
| `tool_arguments_json` | string | O | Canonical JSON string of tool arguments. | Must parse as JSON object/array if present. |
| `tool_result_text` | string | C | Textual tool output summary/body. | Required when `record_format = tool_result` and text output exists. |
| `input_tokens` | integer | O | Input/prompt token count from source/provider. | Integer `>= 0`. |
| `output_tokens` | integer | O | Output/completion token count from source/provider. | Integer `>= 0`. |
| `total_tokens` | integer | O | Total token count. | Integer `>= 0`; if present with both parts, equals sum. |
| `cost_usd` | number | O | USD-equivalent cost for this event when known. | Decimal `>= 0`. |
| `tags` | array<string> | O | Normalized labels for filtering/analytics. | Unique, lowercase slug tokens. |
| `flags` | array<string> | O | Behavioral flags (quality/privacy/runtime). | Unique values; implementation-defined controlled list. |
| `pii_redacted` | boolean | O | Indicates content was transformed by redaction policy. | `true` only when mutation happened. |
| `warnings` | array<string> | O | Non-fatal normalization warnings. | Human-readable stable codes/messages. |
| `errors` | array<string> | O | Record-scoped errors if partial-failure mode keeps record. | Stable codes/messages; empty when no errors. |
| `raw_hash` | string | R | Hash of raw source-record payload basis. | Lowercase hex digest. |
| `canonical_hash` | string | R | Hash of canonical semantic payload basis for dedupe. | Lowercase hex digest. |
| `metadata` | object | O | Extra adapter metadata not promoted to canonical top-level fields. | JSON object; keys must not shadow canonical fields. |

## 6. Conditional Invariants

1. If `record_format = tool_call`, then `event_type = tool_invocation` and `role` is `assistant` or `tool`.
2. If `record_format = tool_result`, then `event_type = tool_output` and `role = tool`.
3. If `record_format = diagnostic`, then `role = runtime` and conversational fields (`content_text`, `conversation_id`) may be omitted.
4. If both `input_tokens` and `output_tokens` are present, `total_tokens` MUST be absent or equal to their sum.
5. If `pii_redacted = true`, then at least one of `content_text` or `content_excerpt` MUST exist and represent the redacted value.
6. `warnings` and `errors` entries MUST be deterministic for identical source input.

## 7. Optionality Semantics (Normative)

- Required fields MUST appear on every record.
- Optional fields SHOULD be omitted (not `null`) when unknown/unavailable.
- Conditionally required fields MUST appear whenever their predicate is true.
- Adapter-specific data that does not map to canonical top-level fields MUST go into `metadata`.

## 8. Compatibility and Change Policy

- `agentlog.v1` is additive within v1: adding optional fields is allowed; renaming/removing canonical fields is not.
- Tightening controlled vocabularies requires explicit migration notes and validator updates.
- Any future major semantic break increments schema version (for example, `agentlog.v2`).
