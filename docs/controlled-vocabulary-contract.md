# Controlled Vocabulary and Unknown-Value Fallback Contract

Status: canonical for `bd-244`  
Applies to `agentlog.v1` fields:
- `record_format`
- `event_type`
- `role`
- `source_kind`
- `adapter_name`
- `timestamp_quality`

Reference implementation types:
- `RecordFormat`, `EventType`, `ActorRole` in `crates/logit/src/models/agentlog.rs`
- `AgentSource`, `TimestampQuality` in `crates/logit/src/models/agentlog.rs`

## 1. Allowed Values

## 1.1 `record_format`

Allowed canonical values:
- `message`
- `tool_call`
- `tool_result`
- `system`
- `diagnostic`

## 1.2 `event_type`

Allowed canonical values:
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

## 1.3 `role`

Allowed canonical values:
- `user`
- `assistant`
- `system`
- `tool`
- `runtime`

## 1.4 `source_kind` and `adapter_name`

Allowed canonical values:
- `codex`
- `claude`
- `gemini`
- `amp`
- `opencode`

## 1.5 `timestamp_quality`

Allowed canonical values:
- `exact`
- `derived`
- `fallback`

## 2. Normalization Rules

1. Source labels are normalized case-insensitively before mapping.
2. Known synonyms may be mapped to canonical values:
   - role: `human` -> `user`, `model` -> `assistant`
   - event_type: `log` -> `debug_log`, `notice` -> `system_notice`
3. Canonical output must always serialize as snake_case values above.
4. `source_kind` and `adapter_name` serialize as canonical adapter slugs from §1.4.
5. `timestamp_quality` is never free-form; values are restricted to §1.5.

## 3. Unknown-Value Fallback Policy

When source values do not map directly:

### 3.1 `record_format` unknown
- Fallback to `diagnostic`
- Emit warning code: `unknown_record_format`
- Preserve raw value in `metadata.original_record_format`

### 3.2 `event_type` unknown
- If record is diagnostic-like, fallback to `debug_log`
- Otherwise fallback to `status_update`
- Emit warning code: `unknown_event_type`
- Preserve raw value in `metadata.original_event_type`

### 3.3 `role` unknown
- For tool result/call records fallback to `tool`
- For runtime/log/diagnostic records fallback to `runtime`
- Otherwise fallback to `system`
- Emit warning code: `unknown_role`
- Preserve raw value in `metadata.original_role`

### 3.4 `source_kind` / `adapter_name` unknown
- Unknown adapter/source family values MUST NOT be emitted.
- Parser should surface deterministic diagnostics and skip invalid records (or fail in strict paths).
- Implementations may preserve raw input in metadata for debugging before record rejection.

### 3.5 `timestamp_quality` unknown
- Unknown quality values MUST NOT be emitted.
- If confidence class cannot be computed, emit deterministic fallback semantics and set `timestamp_quality = fallback`.
- Emit warning code: `unknown_timestamp_quality`.

## 4. Cross-Field Consistency Constraints

1. `record_format = tool_call` implies `event_type = tool_invocation`.
2. `record_format = tool_result` implies `event_type = tool_output`.
3. `record_format = diagnostic` prefers `role = runtime`.
4. Unknown-value fallback must still satisfy these constraints after mapping.
5. In v1, `adapter_name` and `source_kind` must resolve to the same canonical value.

## 5. Strict Mode Behavior

In strict validation mode:
- unknown raw values are still normalized via fallback for continuity
- validation report severity is elevated for fallback usage
- repeated fallback usage should be surfaced as policy drift diagnostics

Strict mode does not permit emitting out-of-vocabulary canonical values.

## 6. Determinism Requirements

1. Same raw input value must always map to the same canonical value and warning code.
2. Mapping order and synonym table are deterministic and versioned.
3. Fallback metadata keys are stable (`original_record_format`, `original_event_type`, `original_role`).
4. Quality and adapter/source normalization diagnostics use stable warning identifiers.

## 7. Compatibility

- This contract is `agentlog.v1` behavior.
- Adding a new canonical vocabulary value is additive but must be reflected in:
  - model enums
  - schema generation
  - this contract document
- Changing fallback targets is a breaking semantic policy change and requires migration notes.
