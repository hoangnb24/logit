# CLI Command and Flag Parity Matrix

Status: canonical for `bd-3bu`  
Depends on implemented CLI in:
- `crates/logit/src/cli/app.rs`
- `crates/logit/src/cli/commands/*.rs`

## Global Behavior

Global flags are accepted before any subcommand and apply to runtime path resolution where relevant.

| Global flag | Type | Required | Applies to | Semantics |
|---|---|---|---|---|
| `--home-dir <PATH>` | path | No | `snapshot`, `normalize`, `validate`, `ingest`, `query` | Overrides home directory used for runtime path resolution. |
| `--cwd <PATH>` | path | No | `snapshot`, `normalize`, `validate`, `ingest`, `query` | Overrides working directory used for relative path resolution. |
| `--out-dir <PATH>` | path | No | `snapshot`, `normalize`, `validate`, `ingest`, `query` | Overrides artifact output directory root. |

Defaults when omitted:
- `home_dir`: `$HOME` environment variable
- `cwd`: process current directory
- `out_dir`: `<home_dir>/.logit/output`

## Command Matrix

| Command | Positional args | Required flags | Optional flags | Output expectation |
|---|---|---|---|---|
| `snapshot` | none | none | `--source-root <PATH>`, `--sample-size <N>` | Prints stage progress and writes snapshot artifacts under `<out_dir>/snapshot`. |
| `normalize` | none | none | `--source-root <PATH>`, `--fail-fast` | Prints stage progress and writes canonical artifacts (`events.jsonl`, schema, stats) and discovery artifacts. |
| `inspect` | `<PATH>` target | none | `--json` | Prints text or JSON inspection output to stdout; does not write runtime artifacts. |
| `validate` | `<INPUT>` | none | `--strict` | Prints validation summary and writes `validate/report.json`. |
| `ingest refresh` | none | none | `--source-root <PATH>`, `--fail-fast` | Emits JSON envelope to stdout and writes `ingest/report.json`; materializes `mart.sqlite`. |
| `query sql` | `<SQL>` | none | `--params <JSON>`, `--row-cap <N>` | Emits JSON envelope to stdout containing row payload + runtime metadata. |
| `query schema` | none | none | `--include-internal` | Emits JSON envelope to stdout containing table/view/column metadata. |
| `query catalog` | none | none | `--verbose` | Emits JSON envelope to stdout containing semantic concepts/relations. |
| `query benchmark` | none | none | `--corpus <PATH>`, `--row-cap <N>` | Emits JSON envelope to stdout and writes benchmark artifact under `<out_dir>/benchmarks`. |

## Flag Parity Notes

1. Runtime path behavior is centralized through global flags for `snapshot`, `normalize`, `validate`, `ingest`, and `query`.
2. `inspect` parses global flags but does not consume runtime path context for execution behavior.
3. All command-specific flags are long-form and stable snake/kebab naming.
4. Boolean mode toggles are explicit:
   - `normalize`: `--fail-fast`
   - `inspect`: `--json`
   - `validate`: `--strict`
   - `query schema`: `--include-internal`
   - `query catalog`: `--verbose`

## Compatibility Expectations

- New commands should follow the same matrix format and explicitly declare:
  - positional requirements
  - optional flags
  - whether global runtime flags are consumed
- Breaking CLI shape changes must update this matrix and associated CLI parse tests.
- Exit code taxonomy contract:
  - `0` success
  - `1` runtime failure
  - `2` validation failure
  - `64` usage/parsing failure
