# Answerability Question Corpus Contract

Status: canonical for `bd-10n`  
Parent stream: `bd-33o` (user question benchmark suite and answerability gates)  
Corpus artifact: `fixtures/benchmarks/answerability_question_corpus_v1.json`

## 1. Purpose

Define the canonical v1 question corpus used to evaluate whether the queryable data plane can answer real operator and agent questions.

The corpus is intentionally machine-consumable and deterministic so future beads can build:
- benchmark harness execution (`bd-1uq`)
- scorecards and release gates (`bd-2gy`)

## 2. Coverage Requirements

The canonical corpus must include representative questions across:
- usage
- performance
- freshness
- reliability

Each domain must include multiple questions that cover both aggregate and diagnostic answer forms.

## 3. Record Shape

Top-level fields in the corpus artifact:
- `schema_version`
- `corpus_id`
- `generated_at_utc`
- `all_data_synthetic`
- `domains`
- `questions`

Each question entry must contain:
- `id` (stable unique identifier)
- `domain` (one of `usage|performance|freshness|reliability`)
- `question` (canonical natural-language prompt)
- `expected_answer_contract` (machine-readable expected answer form)
- `queryability_assumptions` (explicit assumptions for harness/debugging)
- `rationale` (why this question matters for user outcomes)

## 4. Expected Answer Contract Semantics

`expected_answer_contract` defines acceptance form, not one exact numeric value.

Required fields:
- `answer_kind` (for example: `scalar`, `table`, `time_series`, `ranked_list`, `distribution`, `boolean`)
- `must_include` (required columns/fields in the answer)

Optional fields:
- `ordering` (deterministic ordering expectation)
- `threshold` (explicit threshold criteria when applicable)

This allows deterministic automated validation while remaining robust to fixture evolution.

## 5. Data and Safety Constraints

- Corpus content is synthetic and must not include private user data.
- Question IDs are immutable once published.
- Additive questions are allowed in v1; destructive renames/removals require migration notes.
- Domain labels and answer-kind enums must remain stable for harness compatibility.

## 6. Operational Use

Benchmark harnesses should:
1. Iterate question IDs deterministically.
2. Execute mapped query plans for each question.
3. Validate returned shape against `expected_answer_contract`.
4. Emit per-question pass/fail plus aggregate answerability scoring.

Release gates should consume harness output without reinterpreting question semantics.
