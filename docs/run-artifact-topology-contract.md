# Run Artifact Topology Contract

Status: canonical for `bd-2n2`  
Schema family: `agentlog.v1` ecosystem  
Default output directory: `<home_dir>/.logit/output`

## 1. Contract Purpose

This contract defines stable on-disk artifact paths so automation can locate outputs without heuristics.

The artifact topology is:
- deterministic by command and `--out-dir`
- shared across runs (fixed file paths, overwritten by subsequent runs)
- machine-readable for downstream stages (`validate`, `ingest`, `query`, release evidence)

## 2. Output Root and Resolution

Output root is resolved from runtime flags:
- `--out-dir <PATH>` when provided
- otherwise `<home_dir>/.logit/output`

`home_dir` and `cwd` resolution semantics are defined by `crates/logit/src/config/mod.rs`.

## 3. Artifact Layout

```text
<out_dir>/
  events.jsonl
  agentlog.v1.schema.json
  stats.json
  mart.sqlite
  snapshot/
    index.json
    samples.jsonl
    schema_profile.json
  discovery/
    sources.json
    zsh_history_usage.json
  validate/
    report.json
  ingest/
    report.json
  benchmarks/
    answerability_report_v1.json
```

## 4. Command-to-Artifact Mapping

### 4.1 `snapshot`

Writes:
- `snapshot/index.json`
- `snapshot/samples.jsonl`
- `snapshot/schema_profile.json`

`snapshot/samples.jsonl` is newline-delimited JSON (one JSON object per line).

### 4.2 `normalize`

Writes:
- `events.jsonl`
- `agentlog.v1.schema.json`
- `stats.json`
- `discovery/sources.json`
- `discovery/zsh_history_usage.json`

`events.jsonl` is newline-delimited canonical `agentlog.v1` rows.

### 4.3 `validate`

Writes:
- `validate/report.json`

### 4.4 `ingest refresh`

Writes:
- `mart.sqlite`
- `ingest/report.json`

`mart.sqlite` contains canonical tables/views plus ingest metadata tables (`ingest_runs`, `ingest_watermarks`).

### 4.5 `query benchmark`

Writes:
- `benchmarks/answerability_report_v1.json`

Other `query` commands (`query sql`, `query schema`, `query catalog`) emit JSON envelopes to stdout and do not write additional artifact files.

### 4.6 `inspect`

`inspect` emits text/JSON inspection output to stdout and does not write runtime artifact files.

## 5. Determinism and Safety Invariants

1. Artifact file names and relative locations are fixed.
2. Directory creation is command-driven and idempotent (`create_dir_all` semantics).
3. JSON/JSONL artifacts are encoded as UTF-8.
4. Commands fail with explicit runtime errors when required files cannot be read/written.
5. `query` and `ingest` command responses are JSON envelope objects on stdout.

## 6. Compatibility Policy

- Additive artifacts are permitted when existing paths and payload contracts remain stable.
- Breaking path/layout changes require explicit contract revision and corresponding README/docs updates.
