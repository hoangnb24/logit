# Privacy, Safety, and Known Limitations

Status: operator-facing guidance for `bd-2tt`  
Companion contracts:
- `docs/privacy-defaults-contract.md`
- `docs/agentlog-v1-field-optionality-matrix.md`
- `docs/timestamp-normalization-contract.md`
- `docs/dedupe-provenance-policy-contract.md`

## 1. What This Doc Covers

This document explains practical data-handling behavior in `logit`:
- what is redacted by default
- where full text is preserved
- how validation severity behaves
- what v1 intentionally does not guarantee yet

Use this as the operational safety guide; use contract docs for normative field-level rules.

## 2. Default Data Handling by Stage

| Stage | Default policy | Operator implication |
|---|---|---|
| `snapshot` | redacted + truncated sample text | safest artifact set for sharing/debugging |
| `normalize` | full-text retention where available | highest analytic fidelity, highest sensitivity risk |
| `validate` | reports line-level issues without replaying full raw payloads | safe to share in most workflows |
| `inspect` | read-only inspection entrypoint | no source mutation, but input selection still matters |

## 3. Snapshot Safety Defaults

Snapshot mode is safety-first by default:
- sensitive strings are redacted deterministically
- text excerpts are truncated deterministically
- binary payloads are not inlined as raw blobs
- redaction marker token is stable (`[REDACTED]`)

Baseline sensitive-pattern classes include:
- bearer/api tokens
- secret assignment strings (`password=...`, `token: ...`, etc.)
- private key PEM blocks
- URL query-token fragments
- email addresses
- phone-like values

Operational guidance:
- prefer snapshot artifacts for collaboration/debugging
- treat snapshot output as reduced-risk, not zero-risk

## 4. Normalize Fidelity Defaults

Normalize mode favors canonical analytic completeness:
- `content_text` is retained by default
- `content_excerpt` may exist for convenience but does not replace full text
- adapter-specific details may be preserved in metadata

Operational guidance:
- treat normalize outputs as sensitive datasets
- store and share `events.jsonl` with the same controls as source logs
- if compliance requires lower retention, use policy overrides documented in privacy contract (`normalize_text_policy`)

## 5. Validation Safety Behavior

Validation combines:
1. schema checks (shape/required fields)
2. invariant checks (timestamp/hash/content quality semantics)

Mode behavior:
- baseline mode: policy-quality issues can remain warnings
- strict mode: policy-sensitive issues escalate to errors

Exit code contract:
- `0` success
- `1` runtime failure
- `2` validation failure
- `64` argument/usage failure

## 6. Timestamp and Provenance Safety Notes

Timestamp handling is deterministic:
- canonical fields always emitted (`timestamp_utc`, `timestamp_unix_ms`, `timestamp_quality`)
- source hierarchy determines confidence (`exact`, `derived`, `fallback`)
- fallback timestamps are explicit and warning-backed rather than hidden

Provenance and hash fields (`source_*`, `adapter_*`, `raw_hash`, `canonical_hash`) improve auditability and incident triage.

## 7. Intentional v1 Limitations

Known limitations (by design in v1 scope):
- redaction is pattern-based and may miss novel secret formats
- redaction can produce false positives on token-like random strings
- not all adapters/artifact families are feature-parity complete in normalize fan-in
- inspect command is intentionally baseline and not yet a full deep-inspection UX
- compatibility shims for legacy behavior are intentionally avoided; contracts evolve directly

## 8. Safe Operating Recommendations

1. Use `snapshot` outputs for lower-risk artifact sharing.
2. Keep `normalize` outputs in controlled local/private storage unless explicitly sanitized.
3. Run `validate --strict` before automation handoff or release evidence capture.
4. Keep contract docs in sync when policy semantics change.
5. Treat warnings as operational signals, not cosmetic noise.

## 9. Release-Readiness Checklist Inputs

Before marking a run as release-ready:
- snapshot artifacts emitted and inspectable
- normalize artifacts emitted and schema-compatible
- validation report present at `validate/report.json`
- strict-mode validation status reviewed
- known limitation caveats documented for stakeholders
