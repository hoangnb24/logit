# agentlog.v1 Required vs Optional Matrix

Status: canonical for `bd-2hp`  
Companion contracts:
- `docs/agentlog-v1-contract.md`
- `docs/privacy-defaults-contract.md`
- `docs/timestamp-normalization-contract.md`
- `docs/dedupe-provenance-policy-contract.md`

## Global Null Semantics

- Scalar fields use omission for unknown/unavailable values; `null` is not emitted.
- Arrays may be omitted when not applicable; if present, they may be empty.
- Objects may be omitted when not applicable.
- Conditional fields become required when predicate is true.

## Matrix

Legend:
- `R` required always
- `C` conditionally required
- `O` optional

| Field | Req | Condition (if C) | Null/omission semantics |
|---|---|---|---|
| `schema_version` | R |  | Always present; exactly `agentlog.v1`. |
| `event_id` | R |  | Always present; unique per run output. |
| `run_id` | R |  | Always present. |
| `sequence_global` | R |  | Always present; monotonic index. |
| `sequence_source` | O |  | Omit when source-local ordering not available. |
| `source_kind` | R |  | Always present. |
| `source_path` | R |  | Always present. |
| `source_record_locator` | R |  | Always present. |
| `source_record_hash` | O |  | Omit when source slice hashing unavailable. |
| `adapter_name` | R |  | Always present. |
| `adapter_version` | O |  | Omit when adapter version not exposed. |
| `record_format` | R |  | Always present; controlled vocab value. |
| `event_type` | R |  | Always present; controlled vocab value. |
| `role` | R |  | Always present; controlled vocab value. |
| `timestamp_utc` | R |  | Always present (exact/derived/fallback). |
| `timestamp_unix_ms` | R |  | Always present (exact/derived/fallback). |
| `timestamp_quality` | R |  | Always present; one of `exact`,`derived`,`fallback`. |
| `session_id` | O |  | Omit when no session concept exists in source. |
| `conversation_id` | O |  | Omit when no conversation/thread identifier exists. |
| `turn_id` | O |  | Omit when turn grouping is absent. |
| `parent_event_id` | O |  | Omit when event has no parent relation. |
| `actor_id` | O |  | Omit when stable actor ID unavailable. |
| `actor_name` | O |  | Omit when actor display name unavailable. |
| `provider` | O |  | Omit when provider is unknown/not applicable. |
| `model` | O |  | Omit when model identifier unavailable. |
| `content_text` | O |  | Omit when no text exists or policy suppresses full text. |
| `content_excerpt` | O |  | Omit when excerpt is not generated/applicable. |
| `content_mime` | O |  | Omit when MIME type cannot be determined. |
| `tool_name` | C | `record_format` in `{tool_call, tool_result}` | Must be present for tool records; omitted otherwise. |
| `tool_call_id` | C | Tool correlation exists for call/result pair | Must be present when available for paired tool events. |
| `tool_arguments_json` | O |  | Omit when arguments unavailable or non-structured. |
| `tool_result_text` | C | `record_format = tool_result` and textual output exists | Must be present when condition true; omitted otherwise. |
| `input_tokens` | O |  | Omit when usage stats unavailable. |
| `output_tokens` | O |  | Omit when usage stats unavailable. |
| `total_tokens` | O |  | Omit when unavailable; if present with both parts, equal sum. |
| `cost_usd` | O |  | Omit when cost unavailable. |
| `tags` | O |  | Omit when none; if present, array of unique lowercase tokens. |
| `flags` | O |  | Omit when none; if present, array of unique values. |
| `pii_redacted` | O |  | Omit when no redaction mutation occurred. |
| `warnings` | O |  | Omit when none; if present, array of stable warning codes/messages. |
| `errors` | O |  | Omit when none; if present, array of stable error codes/messages. |
| `raw_hash` | R |  | Always present. |
| `canonical_hash` | R |  | Always present. |
| `metadata` | O |  | Omit when no extra adapter metadata remains. |

## Conditional Field Notes

1. `tool_name` and `tool_result_text` are never emitted for non-tool records.
2. `content_text` is omitted when adapters do not emit textual payloads for the source record.
3. `content_excerpt` may be present when an adapter emits text and excerpt derivation succeeds.

## Validation Expectations

Validators should fail records that:
- omit any `R` field
- violate `C` predicates
- emit scalar `null` where omission is required semantics
