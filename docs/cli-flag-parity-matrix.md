# CLI Command and Flag Parity Matrix

Status: canonical for `bd-3bu`  
Depends on implemented CLI in:
- `crates/logit/src/cli/app.rs`
- `crates/logit/src/cli/commands/*.rs`

## Global Behavior

Global flags are accepted before any subcommand and apply to runtime path resolution where relevant.

| Global flag | Type | Required | Applies to | Semantics |
|---|---|---|---|---|
| `--home-dir <PATH>` | path | No | snapshot, normalize, validate | Overrides home directory used for runtime path resolution. |
| `--cwd <PATH>` | path | No | snapshot, normalize, validate | Overrides working directory used for relative path resolution. |
| `--out-dir <PATH>` | path | No | snapshot, normalize, validate | Overrides artifact output directory root. |

Defaults when omitted:
- `home_dir`: `$HOME` environment variable
- `cwd`: process current directory
- `out_dir`: `<home_dir>/.logit/output`

## Subcommand Matrix

| Subcommand | Positional args | Required flags | Optional flags | Output expectation |
|---|---|---|---|---|
| `snapshot` | none | none | `--source-root <PATH>`, `--sample-size <N>` | Initializes snapshot config and resolves runtime output path context. |
| `normalize` | none | none | `--source-root <PATH>`, `--fail-fast` | Builds normalize plan and writes schema artifact to resolved output layout. |
| `inspect` | `<PATH>` target | none | `--json` | Validates inspect surface parsing and target selection mode. |
| `validate` | `<INPUT>` | none | `--strict` | Validates input and writes machine-readable report artifact to resolved output layout (`validate/report.json`). |

## Flag Parity Notes

1. Runtime path behavior is centralized through global flags for `snapshot`, `normalize`, and `validate`.
2. `inspect` does not currently consume runtime path flags for execution behavior.
3. All command-specific flags are long-form and stable snake/kebab naming.
4. Boolean mode toggles are explicit:
   - `normalize`: `--fail-fast`
   - `inspect`: `--json`
   - `validate`: `--strict`

## Compatibility Expectations

- New subcommands should follow the same matrix format and explicitly declare:
  - positional requirements
  - optional flags
  - whether global runtime flags are consumed
- Breaking CLI shape changes must update this matrix and associated CLI parse tests.
- Exit code taxonomy contract:
  - `0` success
  - `1` runtime failure
  - `2` validation failure
  - `64` usage/parsing failure
