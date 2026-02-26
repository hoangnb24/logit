# Fixture Corpus (`bd-29y`)

This directory holds deterministic, synthetic fixture data for `logit`.

## Goals

- Cover all five supported sources: Codex, Claude, Gemini, Amp, OpenCode.
- Provide both happy-path and edge-case source shapes.
- Keep data stable and free of sensitive real user content.

## Conventions

- All fixture content is synthetic and scrubbed.
- `*.jsonl` files are newline-delimited JSON records.
- `*.log` files are raw diagnostic text fixtures.
- Edge-case fixtures are intentionally irregular and should be consumed by negative tests.
- File paths are listed in `fixtures/manifest.json`.

## Source Layout

- `fixtures/codex`
- `fixtures/claude`
- `fixtures/gemini`
- `fixtures/amp`
- `fixtures/opencode`
