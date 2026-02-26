# Run Artifact Topology and Manifest Contract

Status: canonical for `bd-2n2`  
Schema family: `agentlog.v1` ecosystem  
Root output directory: `~/.logit/output`

## 1. Contract Purpose

This contract freezes on-disk output layout and metadata conventions for `logit` runs so:
- automation can locate artifacts without heuristics
- downstream stages (`normalize`, `validate`, docs/tests) can rely on stable paths
- repeated executions remain auditable and deterministic

## 2. Root Layout

```text
~/.logit/output/
  runs/
    <run_id>/
      manifest.json
      command.json
      normalize/
        events.jsonl
        agentlog.v1.schema.json
        stats.json
      snapshot/
        index.json
        samples.jsonl
        schema_profile.json
      validate/
        report.json
      logs/
        warnings.jsonl
        errors.jsonl
  indexes/
    runs.jsonl
```

Rules:
- `runs/<run_id>/` is immutable once the run is marked complete.
- Every run directory MUST contain `manifest.json` and `command.json`.
- Stage directories (`normalize`, `snapshot`, `validate`) are created only when that stage/command executes.
- `indexes/runs.jsonl` is append-only and contains one summary record per completed run.

## 3. Run ID and Naming Contract

`run_id` format:
- `YYYYMMDDTHHMMSSZ_<suffix>`
- `<suffix>` is 8 lowercase hex chars from deterministic hash of `(command, start_time_ns, pid, cwd)`

Examples:
- `20260225T072706Z_a13f9b2c`
- `20260225T101512Z_74e1c0ad`

Naming invariants:
1. Filenames are lowercase with `.json`, `.jsonl`, or no extension directory names.
2. No spaces in directory or file names.
3. Stage artifact filenames are fixed (never user-configurable).

## 4. Required Per-Run Metadata Files

## 4.1 `manifest.json`

Canonical run manifest for discovery and automation.

Required keys:
- `manifest_version`: string, exactly `logit.run-manifest.v1`
- `schema_version`: string, exactly `agentlog.v1`
- `run_id`: string, matches directory name
- `command`: string (`snapshot` | `normalize` | `inspect` | `validate`)
- `status`: string (`running` | `success` | `partial_failure` | `failed`)
- `started_at_utc`: RFC3339 UTC timestamp
- `finished_at_utc`: RFC3339 UTC timestamp or omitted while running
- `host`: object with `os`, `arch`, `hostname` (hostname optional)
- `paths`: object with `output_root`, `run_dir`, `source_roots` (array)
- `adapters`: array of adapter names attempted (`codex`,`claude`,`gemini`,`amp`,`opencode`)
- `artifact_map`: object mapping logical artifact names to relative paths
- `counts`: object with at least `records_emitted`, `warnings`, `errors`

Optional keys:
- `git`: object (`commit`, `dirty`) when repository context is detected
- `duration_ms`: integer, present once finished
- `failure_summary`: object for non-success statuses

## 4.2 `command.json`

Machine-readable execution request snapshot.

Required keys:
- `command`: subcommand name
- `argv`: full argv array
- `effective_config`: resolved runtime config object (fully expanded paths)
- `requested_at_utc`: RFC3339 UTC timestamp

Optional keys:
- `env_hints`: whitelisted environment values used for behavior (no secrets)

## 5. Stage Artifact Contracts

## 5.1 `normalize/`
- `events.jsonl`: canonical normalized events (`agentlog.v1` records)
- `agentlog.v1.schema.json`: schema describing event record contract
- `stats.json`: aggregate counts, adapter contributions, quality metrics

## 5.2 `snapshot/`
- `index.json`: discovered source inventory and artifact pointers
- `samples.jsonl`: representative sample records/events (possibly redacted)
- `schema_profile.json`: per-source key/type profile summary

## 5.3 `validate/`
- `report.json`: machine-readable validation output including exit-code interpretation fields

## 5.4 `logs/`
- `warnings.jsonl`: structured warnings generated during run
- `errors.jsonl`: structured per-record or run-level errors

## 6. `artifact_map` Semantics

`manifest.json.artifact_map` keys are logical identifiers and values are paths relative to `runs/<run_id>/`.

Required logical keys by command:
- `normalize`: `events_jsonl`, `schema_json`, `stats_json`
- `snapshot`: `snapshot_index_json`, `snapshot_samples_jsonl`, `snapshot_schema_profile_json`
- `validate`: `validate_report_json`

If a key is required for command mode but artifact is not produced, run status MUST be `failed` or `partial_failure` with failure details.

## 7. Determinism and Safety Invariants

1. Path generation must be independent of locale.
2. Re-running identical inputs may produce different `run_id`, but artifact filenames and JSON key ordering policies must remain stable.
3. `manifest.json` must be atomically finalized (write temp + rename) to avoid half-written terminal state.
4. Artifact files must never be written outside `~/.logit/output` unless explicit override is requested.
5. Absolute source paths may appear in manifests; secret-bearing environment values must not.

## 8. Backfill/Compatibility Rules

- This contract defines v1 topology only.
- Any breaking layout change requires `logit.run-manifest.v2` and explicit migration notes.
- Additive fields in JSON files are permitted when they do not alter required semantics.
