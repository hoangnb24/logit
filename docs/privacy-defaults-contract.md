# Privacy Defaults Contract: Snapshot Redaction vs Normalize Full-Text

Status: canonical for `bd-1gt`  
Applies to: `logit snapshot`, `logit normalize`, and related emitted artifacts

## 1. Purpose

Define default data-handling behavior so operators and implementers know exactly:
- what is retained verbatim
- what is redacted or transformed
- where sensitive values can still appear

This contract is normative for v1.

## 2. Data Handling Principles

1. Source artifacts are read-only; `logit` never mutates source logs.
2. Snapshot output is safety-first and redaction-first.
3. Normalize output is analysis-first and full-text by default.
4. Redaction is deterministic: identical input yields identical redacted output.
5. Privacy behavior is explicit in emitted artifacts (`snapshot_truncated`, `pii_redacted`, `redaction_classes`) and warning/report surfaces.

## 3. Default Policy Matrix

| Command | Artifact family | Default content policy |
|---|---|---|
| `snapshot` | `snapshot/samples.jsonl` | Redacted + truncated excerpts |
| `snapshot` | `snapshot/index.json`, `snapshot/schema_profile.json` | Metadata-only (no full message text) |
| `normalize` | `events.jsonl` | Full text retained where available |
| `normalize` | `stats.json`, schema artifact | Aggregates/structure only |
| `validate` | validation report | No raw full-text echo by default; summarize paths/line refs |

## 4. Sensitive Pattern Classes (v1 baseline)

Redaction engine must match at least:
- API keys and bearer-like tokens
- passwords/secrets in assignment-like text
- private key PEM blocks
- access tokens in URL query fragments
- email addresses
- phone numbers (international and US-like patterns)

Pattern updates are additive and versioned; pattern class names must be stable for reporting.

## 5. Snapshot Policy (Default: Safe)

For snapshot artifacts:
1. Text fields are redacted before truncation.
2. Truncation limit is deterministic (same policy, same input -> same excerpt).
3. Raw tool arguments/results in samples are treated as text and redacted.
4. Binary payloads are never inlined; only metadata references are emitted.
5. Redaction token is canonical (`[REDACTED]`).

Allowed snapshot text exposure:
- non-sensitive tokens not matching redaction rules
- short context snippets after redaction for debugging shape/type issues

## 6. Normalize Policy (Default: Full Fidelity)

For normalize artifacts:
1. `content_text` is retained by default for semantic completeness.
2. `content_excerpt` may be generated for quick inspection but does not replace full text.
3. Sensitive-value redaction is NOT applied by default in normalize mode.

Rationale:
- normalization is the canonical semantic dataset
- downstream dedupe/provenance/quality checks require unmodified conversational content

## 7. Runtime Policy Surface (Normative)

The v1 runtime surface enforces:
- snapshot redaction/truncation enabled by default
- normalize full-text retention by default
- no CLI/config switch that disables snapshot redaction
- no CLI/config switch that suppresses normalize `content_text`

Policy changes require explicit code and contract updates.

## 8. Invariants and Guardrails

1. Snapshot artifacts must not contain unredacted values that match configured sensitive-pattern classes.
2. Redacted snapshot samples must preserve deterministic truncation behavior.
3. Normalize artifacts retain `content_text` when adapters emit textual content.
4. Snapshot sample rows include redaction/truncation markers when mutation occurs.
5. Validation reports remain machine-readable and avoid replaying full raw payloads.

## 9. Known Limitations (Explicit)

- Pattern-based redaction is heuristic and may miss novel secret formats.
- False positives are possible for token-like random strings.
- Legacy artifacts produced before this contract may not comply.

## 10. Compatibility

- This contract is v1 behavior.
- Breaking policy semantics (for example, changing normalize full-text defaults or snapshot redaction defaults) requires explicit contract revision and migration notes.
