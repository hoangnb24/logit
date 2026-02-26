use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Error, Result, bail};
use clap::{Args, Subcommand};
use rusqlite::params_from_iter;
use rusqlite::types::Value as SqlValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;

use crate::config::RuntimePaths;
use crate::models::{QueryEnvelope, QueryEnvelopeCommandFailure};

#[derive(Debug, Clone, Args)]
pub struct QueryArgs {
    #[command(subcommand)]
    pub command: QueryCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum QueryCommand {
    Sql(QuerySqlArgs),
    Schema(QuerySchemaArgs),
    Catalog(QueryCatalogArgs),
    Benchmark(QueryBenchmarkArgs),
}

#[derive(Debug, Clone, Args)]
pub struct QuerySqlArgs {
    #[arg(value_name = "SQL")]
    pub sql: String,

    #[arg(long, value_name = "JSON")]
    pub params: Option<String>,

    #[arg(long, default_value_t = 1_000)]
    pub row_cap: usize,
}

#[derive(Debug, Clone, Args)]
pub struct QuerySchemaArgs {
    #[arg(long, default_value_t = false)]
    pub include_internal: bool,
}

#[derive(Debug, Clone, Args)]
pub struct QueryCatalogArgs {
    #[arg(long, default_value_t = false)]
    pub verbose: bool,
}

#[derive(Debug, Clone, Args)]
pub struct QueryBenchmarkArgs {
    #[arg(long, value_name = "PATH")]
    pub corpus: Option<PathBuf>,

    #[arg(long, default_value_t = 200)]
    pub row_cap: usize,
}

pub fn run(args: &QueryArgs, runtime_paths: &RuntimePaths) -> Result<()> {
    match &args.command {
        QueryCommand::Sql(sql_args) => run_sql_query(sql_args, runtime_paths),
        QueryCommand::Schema(schema_args) => run_schema_query(schema_args, runtime_paths),
        QueryCommand::Catalog(catalog_args) => run_catalog_query(catalog_args),
        QueryCommand::Benchmark(benchmark_args) => {
            run_benchmark_query(benchmark_args, runtime_paths)
        }
    }
}

fn run_sql_query(args: &QuerySqlArgs, runtime_paths: &RuntimePaths) -> Result<()> {
    let sql_profile = analyze_sql_profile(&args.sql);

    if let Err(violation) = validate_read_only_sql(&args.sql) {
        let envelope =
            QueryEnvelope::error("query.sql", "sql_guardrail_violation", &violation.message)
                .with_meta("implemented", json!(true))
                .with_meta("guardrail_checked", json!(true))
                .with_meta(
                    "diagnostics",
                    sql_profile.runtime_diagnostics(0, args.row_cap, 0, false),
                )
                .with_error_details(violation.details);
        return Err(Error::new(QueryEnvelopeCommandFailure::new(envelope)));
    }

    if args.row_cap == 0 {
        let envelope = QueryEnvelope::error(
            "query.sql",
            "query_row_cap_invalid",
            "row_cap must be greater than zero",
        )
        .with_meta("implemented", json!(true))
        .with_meta("guardrail_checked", json!(true))
        .with_meta(
            "diagnostics",
            sql_profile.runtime_diagnostics(0, args.row_cap, 0, false),
        )
        .with_error_details(json!({ "row_cap": args.row_cap }));
        return Err(Error::new(QueryEnvelopeCommandFailure::new(envelope)));
    }

    let params = parse_query_params(args.params.as_deref()).map_err(|error| {
        Error::new(QueryEnvelopeCommandFailure::new(
            QueryEnvelope::error("query.sql", "query_params_invalid", "invalid query params")
                .with_meta("implemented", json!(true))
                .with_meta("guardrail_checked", json!(true))
                .with_meta(
                    "diagnostics",
                    sql_profile.runtime_diagnostics(0, args.row_cap, 0, false),
                )
                .with_error_details(json!({ "cause": format!("{error:#}") })),
        ))
    })?;

    let sqlite_path = runtime_paths.out_dir.join("mart.sqlite");
    let connection = crate::sqlite::open_sqlite_connection(&sqlite_path).map_err(|error| {
        Error::new(QueryEnvelopeCommandFailure::new(
            QueryEnvelope::error(
                "query.sql",
                "query_mart_unavailable",
                "unable to open sqlite mart",
            )
            .with_meta("implemented", json!(true))
            .with_meta("guardrail_checked", json!(true))
            .with_meta(
                "diagnostics",
                sql_profile.runtime_diagnostics(0, args.row_cap, 0, false),
            )
            .with_error_details(json!({
                "sqlite_path": sqlite_path.display().to_string(),
                "cause": format!("{error:#}")
            })),
        ))
    })?;

    let started = std::time::Instant::now();
    let result = execute_read_only_query(&connection, &args.sql, &params, args.row_cap).map_err(
        |error| {
            let duration_ms = started.elapsed().as_millis() as u64;
            Error::new(QueryEnvelopeCommandFailure::new(
                QueryEnvelope::error(
                    "query.sql",
                    "query_execution_failed",
                    "query execution failed",
                )
                .with_meta("implemented", json!(true))
                .with_meta("guardrail_checked", json!(true))
                .with_meta("row_cap", json!(args.row_cap))
                .with_meta("duration_ms", json!(duration_ms))
                .with_meta(
                    "diagnostics",
                    sql_profile.runtime_diagnostics(duration_ms, args.row_cap, 0, false),
                )
                .with_error_details(json!({ "cause": format!("{error:#}") })),
            ))
        },
    )?;
    let duration_ms = started.elapsed().as_millis() as u64;

    let envelope = QueryEnvelope::ok("query.sql", json!({ "rows": result.rows }))
        .with_meta("implemented", json!(true))
        .with_meta("guardrail_checked", json!(true))
        .with_meta("row_count", json!(result.row_count))
        .with_meta("truncated", json!(result.truncated))
        .with_meta("row_cap", json!(args.row_cap))
        .with_meta("duration_ms", json!(duration_ms))
        .with_meta("params_count", json!(params.len()))
        .with_meta(
            "diagnostics",
            sql_profile.runtime_diagnostics(
                duration_ms,
                args.row_cap,
                result.row_count,
                result.truncated,
            ),
        );

    let encoded = serde_json::to_string(&envelope).map_err(|error| {
        Error::new(QueryEnvelopeCommandFailure::new(
            QueryEnvelope::error(
                "query.sql",
                "query_response_encode_failed",
                "failed to encode query response",
            )
            .with_error_details(json!({ "cause": format!("{error:#}") })),
        ))
    })?;
    println!("{encoded}");

    Ok(())
}

const ANSWERABILITY_CORPUS_SCHEMA_VERSION: &str = "logit.answerability-corpus.v1";
const ANSWERABILITY_BENCHMARK_REPORT_SCHEMA_VERSION: &str =
    "logit.answerability-benchmark-report.v1";
const DEFAULT_ANSWERABILITY_CORPUS_PATH: &str =
    "fixtures/benchmarks/answerability_question_corpus_v1.json";
const DEFAULT_BENCHMARK_ARTIFACT_PATH: &str = "benchmarks/answerability_report_v1.json";
const ANSWERABILITY_MIN_TOTAL_SCORE_PCT: f64 = 95.0;
const ANSWERABILITY_MIN_DOMAIN_SCORE_PCT: f64 = 90.0;
const ANSWERABILITY_MAX_FAILED_QUESTIONS: usize = 0;

#[derive(Debug, Clone, Deserialize)]
struct AnswerabilityCorpus {
    schema_version: String,
    corpus_id: String,
    generated_at_utc: String,
    all_data_synthetic: bool,
    domains: Vec<String>,
    questions: Vec<AnswerabilityQuestion>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnswerabilityQuestion {
    id: String,
    domain: String,
    question: String,
    expected_answer_contract: AnswerabilityExpectedAnswerContract,
    queryability_assumptions: Vec<String>,
    rationale: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AnswerabilityExpectedAnswerContract {
    answer_kind: String,
    must_include: Vec<String>,
    ordering: Option<String>,
    threshold: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AnswerabilityBenchmarkPreflight {
    schema_table_count: usize,
    schema_view_count: usize,
    semantic_concept_count: usize,
    semantic_relation_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct AnswerabilityBenchmarkQuestionResult {
    id: String,
    domain: String,
    question: String,
    answer_kind: String,
    query_interface: String,
    sql: String,
    must_include: Vec<String>,
    ordering: Option<String>,
    threshold: Option<String>,
    queryability_assumptions: Vec<String>,
    rationale: String,
    passed: bool,
    row_count: usize,
    truncated: bool,
    duration_ms: u64,
    column_names: Vec<String>,
    missing_required_fields: Vec<String>,
    warnings: Vec<String>,
    failure_code: Option<String>,
    failure_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AnswerabilityBenchmarkDomainSummary {
    domain: String,
    total_questions: usize,
    passed_questions: usize,
    failed_questions: usize,
    score_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
struct AnswerabilityBenchmarkSummary {
    total_questions: usize,
    passed_questions: usize,
    failed_questions: usize,
    score_pct: f64,
    per_domain: Vec<AnswerabilityBenchmarkDomainSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct AnswerabilityReleaseGate {
    minimum_total_score_pct: f64,
    minimum_domain_score_pct: f64,
    maximum_failed_questions: usize,
    observed_total_score_pct: f64,
    observed_failed_questions: usize,
    failing_domains: Vec<String>,
    passed: bool,
    failed_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AnswerabilityBenchmarkReport {
    schema_version: String,
    corpus_schema_version: String,
    corpus_id: String,
    corpus_generated_at_utc: String,
    all_data_synthetic: bool,
    corpus_domains: Vec<String>,
    row_cap: usize,
    preflight: AnswerabilityBenchmarkPreflight,
    summary: AnswerabilityBenchmarkSummary,
    release_gate: AnswerabilityReleaseGate,
    questions: Vec<AnswerabilityBenchmarkQuestionResult>,
}

#[derive(Debug, Clone, Copy)]
struct AnswerabilityBenchmarkQueryPlan {
    query_interface: &'static str,
    sql: &'static str,
}

fn run_benchmark_query(args: &QueryBenchmarkArgs, runtime_paths: &RuntimePaths) -> Result<()> {
    if args.row_cap == 0 {
        let envelope = QueryEnvelope::error(
            "query.benchmark",
            "query_row_cap_invalid",
            "row_cap must be greater than zero",
        )
        .with_meta("implemented", json!(true))
        .with_meta("row_cap", json!(args.row_cap));
        return Err(Error::new(QueryEnvelopeCommandFailure::new(envelope)));
    }

    let corpus_path = args
        .corpus
        .clone()
        .unwrap_or_else(|| runtime_paths.cwd.join(DEFAULT_ANSWERABILITY_CORPUS_PATH));
    let corpus = load_answerability_corpus(&corpus_path).map_err(|error| {
        let envelope = QueryEnvelope::error(
            "query.benchmark",
            "query_benchmark_corpus_invalid",
            "failed to load answerability benchmark corpus",
        )
        .with_meta("implemented", json!(true))
        .with_meta("corpus_path", json!(corpus_path.display().to_string()))
        .with_error_details(json!({ "cause": format!("{error:#}") }));
        Error::new(QueryEnvelopeCommandFailure::new(envelope))
    })?;

    let sqlite_path = runtime_paths.out_dir.join("mart.sqlite");
    let connection = crate::sqlite::open_sqlite_connection(&sqlite_path).map_err(|error| {
        let envelope = QueryEnvelope::error(
            "query.benchmark",
            "query_mart_unavailable",
            "unable to open sqlite mart",
        )
        .with_meta("implemented", json!(true))
        .with_meta("sqlite_path", json!(sqlite_path.display().to_string()))
        .with_error_details(json!({ "cause": format!("{error:#}") }));
        Error::new(QueryEnvelopeCommandFailure::new(envelope))
    })?;
    crate::sqlite::ensure_sqlite_schema(&connection).map_err(|error| {
        let envelope = QueryEnvelope::error(
            "query.benchmark",
            "query_schema_introspection_failed",
            "failed to ensure sqlite schema before benchmark execution",
        )
        .with_meta("implemented", json!(true))
        .with_meta("sqlite_path", json!(sqlite_path.display().to_string()))
        .with_error_details(json!({ "cause": format!("{error:#}") }));
        Error::new(QueryEnvelopeCommandFailure::new(envelope))
    })?;

    let preflight = build_benchmark_preflight(&connection).map_err(|error| {
        let envelope = QueryEnvelope::error(
            "query.benchmark",
            "query_benchmark_preflight_failed",
            "benchmark preflight failed",
        )
        .with_meta("implemented", json!(true))
        .with_meta("sqlite_path", json!(sqlite_path.display().to_string()))
        .with_error_details(json!({ "cause": format!("{error:#}") }));
        Error::new(QueryEnvelopeCommandFailure::new(envelope))
    })?;

    let mut questions = corpus.questions.clone();
    questions.sort_by(|left, right| left.id.cmp(&right.id));
    let question_reports = questions
        .iter()
        .map(|question| run_answerability_question(question, &connection, args.row_cap))
        .collect::<Vec<_>>();
    let summary = build_benchmark_summary(&question_reports);
    let release_gate = evaluate_release_gate(&summary);

    let report = AnswerabilityBenchmarkReport {
        schema_version: ANSWERABILITY_BENCHMARK_REPORT_SCHEMA_VERSION.to_string(),
        corpus_schema_version: corpus.schema_version.clone(),
        corpus_id: corpus.corpus_id.clone(),
        corpus_generated_at_utc: corpus.generated_at_utc.clone(),
        all_data_synthetic: corpus.all_data_synthetic,
        corpus_domains: corpus.domains.clone(),
        row_cap: args.row_cap,
        preflight,
        summary: summary.clone(),
        release_gate: release_gate.clone(),
        questions: question_reports,
    };

    let artifact_path = query_benchmark_artifact_path(&runtime_paths.out_dir);
    write_benchmark_artifact(&artifact_path, &report).map_err(|error| {
        let envelope = QueryEnvelope::error(
            "query.benchmark",
            "query_benchmark_artifact_write_failed",
            "failed to write benchmark artifact",
        )
        .with_meta("artifact_path", json!(artifact_path.display().to_string()))
        .with_error_details(json!({ "cause": format!("{error:#}") }));
        Error::new(QueryEnvelopeCommandFailure::new(envelope))
    })?;

    let data = serde_json::to_value(&report).map_err(|error| {
        Error::new(QueryEnvelopeCommandFailure::new(
            QueryEnvelope::error(
                "query.benchmark",
                "query_benchmark_encode_failed",
                "failed to encode benchmark report",
            )
            .with_error_details(json!({ "cause": format!("{error:#}") })),
        ))
    })?;
    let mut envelope = QueryEnvelope::ok("query.benchmark", data)
        .with_meta("implemented", json!(true))
        .with_meta("corpus_path", json!(corpus_path.display().to_string()))
        .with_meta("artifact_path", json!(artifact_path.display().to_string()))
        .with_meta("row_cap", json!(args.row_cap))
        .with_meta("question_count", json!(summary.total_questions))
        .with_meta("passed_count", json!(summary.passed_questions))
        .with_meta("failed_count", json!(summary.failed_questions))
        .with_meta("score_pct", json!(summary.score_pct))
        .with_meta("release_gate_passed", json!(release_gate.passed))
        .with_meta(
            "release_gate_failed_checks_count",
            json!(release_gate.failed_checks.len()),
        );
    if !release_gate.passed {
        envelope = envelope
            .with_warning(
                "query_benchmark_release_gate_failed",
                "answerability release gate thresholds not met",
            )
            .with_warning_details(json!({ "failed_checks": release_gate.failed_checks }));
    }
    let encoded = serde_json::to_string(&envelope).map_err(|error| {
        Error::new(QueryEnvelopeCommandFailure::new(
            QueryEnvelope::error(
                "query.benchmark",
                "query_response_encode_failed",
                "failed to encode query response",
            )
            .with_error_details(json!({ "cause": format!("{error:#}") })),
        ))
    })?;
    println!("{encoded}");

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct CatalogFieldDescriptor {
    name: String,
    description: String,
}

#[derive(Debug, Clone, Serialize)]
struct CatalogJoinDescriptor {
    to_concept: String,
    relation: String,
    join_type: String,
    join_on: String,
    rationale: String,
}

#[derive(Debug, Clone, Serialize)]
struct CatalogConceptDescriptor {
    concept_id: String,
    description: String,
    primary_relation: String,
    grain: String,
    key_fields: Vec<String>,
    suggested_dimensions: Vec<String>,
    suggested_metrics: Vec<String>,
    default_ordering: Vec<String>,
    joins: Vec<CatalogJoinDescriptor>,
    field_catalog: Option<Vec<CatalogFieldDescriptor>>,
}

#[derive(Debug, Clone, Serialize)]
struct CatalogRelationDescriptor {
    name: String,
    kind: String,
    purpose: String,
}

fn run_catalog_query(args: &QueryCatalogArgs) -> Result<()> {
    let concepts = vec![
        tool_calls_concept(args.verbose),
        sessions_concept(args.verbose),
        adapters_concept(args.verbose),
        quality_concept(args.verbose),
    ];

    let relations = vec![
        catalog_relation(
            "v_tool_calls",
            "view",
            "tool call/result pairing, duration, and pairing status",
        ),
        catalog_relation(
            "v_sessions",
            "view",
            "session-level rollups for event and tool activity",
        ),
        catalog_relation(
            "v_adapters",
            "view",
            "adapter-level usage/reliability aggregate counters",
        ),
        catalog_relation(
            "v_quality",
            "view",
            "timestamp-quality and warning/error quality rollups",
        ),
        catalog_relation(
            "ingest_runs",
            "table",
            "refresh run lifecycle/status and ingest counters",
        ),
        catalog_relation(
            "ingest_watermarks",
            "table",
            "per-source freshness/staleness and watermark frontiers",
        ),
    ];

    let concept_count = concepts.len();
    let relation_count = relations.len();
    let envelope = QueryEnvelope::ok(
        "query.catalog",
        json!({
            "schema_version": "logit.semantic-catalog.v1",
            "concepts": concepts,
            "relations": relations,
        }),
    )
    .with_meta("implemented", json!(true))
    .with_meta("verbose", json!(args.verbose))
    .with_meta("concept_count", json!(concept_count))
    .with_meta("relation_count", json!(relation_count));

    let encoded = serde_json::to_string(&envelope).map_err(|error| {
        Error::new(QueryEnvelopeCommandFailure::new(
            QueryEnvelope::error(
                "query.catalog",
                "query_response_encode_failed",
                "failed to encode query response",
            )
            .with_error_details(json!({ "cause": format!("{error:#}") })),
        ))
    })?;
    println!("{encoded}");

    Ok(())
}

fn tool_calls_concept(verbose: bool) -> CatalogConceptDescriptor {
    CatalogConceptDescriptor {
        concept_id: "tool_calls".to_string(),
        description: "One row per tool-call pairing outcome for latency and pairing diagnostics"
            .to_string(),
        primary_relation: "v_tool_calls".to_string(),
        grain: "run_id + tool_call_id pairing outcome".to_string(),
        key_fields: strings(&["run_id", "tool_call_id", "call_event_id", "result_event_id"]),
        suggested_dimensions: strings(&[
            "adapter_name",
            "tool_name",
            "pairing_status",
            "session_id",
            "conversation_id",
        ]),
        suggested_metrics: strings(&[
            "duration_ms",
            "COUNT(*) AS tool_call_count",
            "SUM(CASE WHEN pairing_status='missing_result' THEN 1 ELSE 0 END)",
        ]),
        default_ordering: strings(&["call_timestamp_unix_ms DESC", "tool_call_id ASC"]),
        joins: vec![catalog_join(
            "sessions",
            "v_sessions",
            "left",
            "v_tool_calls.run_id = v_sessions.run_id AND v_tool_calls.session_id = v_sessions.session_id",
            "Attach session rollups to tool-level behavior",
        )],
        field_catalog: verbose.then(|| {
            catalog_fields(&[
                ("run_id", "Ingest run identifier"),
                ("session_id", "Session identifier"),
                ("conversation_id", "Conversation identifier"),
                ("tool_name", "Resolved tool name"),
                (
                    "pairing_status",
                    "paired | missing_result | invalid_order | orphan_result",
                ),
                ("duration_ms", "Paired call/result duration in milliseconds"),
                ("duration_source", "Duration provenance marker"),
                ("duration_quality", "Duration confidence marker"),
            ])
        }),
    }
}

fn sessions_concept(verbose: bool) -> CatalogConceptDescriptor {
    CatalogConceptDescriptor {
        concept_id: "sessions".to_string(),
        description: "Session-level activity and composition rollups".to_string(),
        primary_relation: "v_sessions".to_string(),
        grain: "run_id + session_id".to_string(),
        key_fields: strings(&["run_id", "session_id"]),
        suggested_dimensions: strings(&["session_id", "run_id"]),
        suggested_metrics: strings(&[
            "event_count",
            "duration_ms",
            "tool_call_count",
            "distinct_tool_count",
            "error_count",
        ]),
        default_ordering: strings(&["duration_ms DESC", "event_count DESC", "session_id ASC"]),
        joins: vec![catalog_join(
            "tool_calls",
            "v_tool_calls",
            "left",
            "v_sessions.run_id = v_tool_calls.run_id AND v_sessions.session_id = v_tool_calls.session_id",
            "Drill down from session outliers into tool-level causes",
        )],
        field_catalog: verbose.then(|| {
            catalog_fields(&[
                ("event_count", "Total events in the session"),
                (
                    "duration_ms",
                    "Session span from first to last event timestamp",
                ),
                ("tool_call_count", "Tool call events in session"),
                ("tool_result_count", "Tool result events in session"),
                ("prompt_count", "Prompt events in session"),
                ("response_count", "Response events in session"),
                ("error_count", "Error events in session"),
            ])
        }),
    }
}

fn adapters_concept(verbose: bool) -> CatalogConceptDescriptor {
    CatalogConceptDescriptor {
        concept_id: "adapters".to_string(),
        description: "Adapter-level usage volume and reliability counters".to_string(),
        primary_relation: "v_adapters".to_string(),
        grain: "adapter_name".to_string(),
        key_fields: strings(&["adapter_name"]),
        suggested_dimensions: strings(&["adapter_name"]),
        suggested_metrics: strings(&[
            "event_count",
            "session_count",
            "tool_call_count",
            "warning_record_count",
            "error_record_count",
            "pii_redacted_count",
        ]),
        default_ordering: strings(&["event_count DESC", "adapter_name ASC"]),
        joins: vec![catalog_join(
            "quality",
            "v_quality",
            "left",
            "v_adapters.adapter_name = v_quality.adapter_name",
            "Break adapter metrics down by timestamp quality and quality markers",
        )],
        field_catalog: verbose.then(|| {
            catalog_fields(&[
                ("event_count", "Total records emitted by adapter"),
                ("run_count", "Distinct ingest runs with adapter activity"),
                ("session_count", "Distinct sessions with adapter activity"),
                ("tool_call_count", "Tool call records attributed to adapter"),
                ("warning_record_count", "Records that contain warnings"),
                ("error_record_count", "Records that contain errors"),
                ("pii_redacted_count", "Records marked as PII-redacted"),
            ])
        }),
    }
}

fn quality_concept(verbose: bool) -> CatalogConceptDescriptor {
    CatalogConceptDescriptor {
        concept_id: "quality".to_string(),
        description: "Timestamp quality and quality-marker rollups by adapter".to_string(),
        primary_relation: "v_quality".to_string(),
        grain: "adapter_name + timestamp_quality".to_string(),
        key_fields: strings(&["adapter_name", "timestamp_quality"]),
        suggested_dimensions: strings(&["adapter_name", "timestamp_quality"]),
        suggested_metrics: strings(&[
            "event_count",
            "warning_record_count",
            "error_record_count",
            "flagged_record_count",
            "pii_redacted_count",
        ]),
        default_ordering: strings(&["adapter_name ASC", "timestamp_quality ASC"]),
        joins: vec![catalog_join(
            "adapters",
            "v_adapters",
            "left",
            "v_quality.adapter_name = v_adapters.adapter_name",
            "Attach quality rollups to adapter-level volume context",
        )],
        field_catalog: verbose.then(|| {
            catalog_fields(&[
                ("timestamp_quality", "exact | derived | fallback"),
                ("event_count", "Event count at this quality tier"),
                ("warning_record_count", "Rows with warning payloads"),
                ("error_record_count", "Rows with error payloads"),
                ("flagged_record_count", "Rows with flags metadata"),
                ("pii_redacted_count", "Rows with PII redaction marker"),
            ])
        }),
    }
}

fn catalog_join(
    to_concept: &str,
    relation: &str,
    join_type: &str,
    join_on: &str,
    rationale: &str,
) -> CatalogJoinDescriptor {
    CatalogJoinDescriptor {
        to_concept: to_concept.to_string(),
        relation: relation.to_string(),
        join_type: join_type.to_string(),
        join_on: join_on.to_string(),
        rationale: rationale.to_string(),
    }
}

fn catalog_relation(name: &str, kind: &str, purpose: &str) -> CatalogRelationDescriptor {
    CatalogRelationDescriptor {
        name: name.to_string(),
        kind: kind.to_string(),
        purpose: purpose.to_string(),
    }
}

fn catalog_fields(field_pairs: &[(&str, &str)]) -> Vec<CatalogFieldDescriptor> {
    field_pairs
        .iter()
        .map(|(name, description)| CatalogFieldDescriptor {
            name: (*name).to_string(),
            description: (*description).to_string(),
        })
        .collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[derive(Debug, Clone, Serialize)]
struct SchemaColumnDescriptor {
    ordinal: i64,
    name: String,
    declared_type: Option<String>,
    nullable: bool,
    default_value_sql: Option<String>,
    primary_key_position: i64,
}

#[derive(Debug, Clone, Serialize)]
struct SchemaObjectDescriptor {
    name: String,
    kind: String,
    internal: bool,
    columns: Vec<SchemaColumnDescriptor>,
}

fn run_schema_query(args: &QuerySchemaArgs, runtime_paths: &RuntimePaths) -> Result<()> {
    let sqlite_path = runtime_paths.out_dir.join("mart.sqlite");
    let connection = crate::sqlite::open_sqlite_connection(&sqlite_path).map_err(|error| {
        Error::new(QueryEnvelopeCommandFailure::new(
            QueryEnvelope::error(
                "query.schema",
                "query_mart_unavailable",
                "unable to open sqlite mart",
            )
            .with_meta("implemented", json!(true))
            .with_meta("include_internal", json!(args.include_internal))
            .with_error_details(json!({
                "sqlite_path": sqlite_path.display().to_string(),
                "cause": format!("{error:#}")
            })),
        ))
    })?;
    crate::sqlite::ensure_sqlite_schema(&connection).map_err(|error| {
        Error::new(QueryEnvelopeCommandFailure::new(
            QueryEnvelope::error(
                "query.schema",
                "query_schema_introspection_failed",
                "failed to ensure sqlite schema before introspection",
            )
            .with_meta("implemented", json!(true))
            .with_meta("include_internal", json!(args.include_internal))
            .with_error_details(json!({
                "sqlite_path": sqlite_path.display().to_string(),
                "cause": format!("{error:#}")
            })),
        ))
    })?;

    let objects = load_schema_descriptors(&connection, args.include_internal).map_err(|error| {
        Error::new(QueryEnvelopeCommandFailure::new(
            QueryEnvelope::error(
                "query.schema",
                "query_schema_introspection_failed",
                "failed to introspect sqlite schema",
            )
            .with_meta("implemented", json!(true))
            .with_meta("include_internal", json!(args.include_internal))
            .with_error_details(json!({
                "sqlite_path": sqlite_path.display().to_string(),
                "cause": format!("{error:#}")
            })),
        ))
    })?;

    let (tables, views): (Vec<_>, Vec<_>) = objects
        .into_iter()
        .partition(|object| object.kind == "table");

    let table_count = tables.len();
    let view_count = views.len();
    let envelope = QueryEnvelope::ok(
        "query.schema",
        json!({
            "tables": tables,
            "views": views,
        }),
    )
    .with_meta("implemented", json!(true))
    .with_meta("include_internal", json!(args.include_internal))
    .with_meta("sqlite_path", json!(sqlite_path.display().to_string()))
    .with_meta("table_count", json!(table_count))
    .with_meta("view_count", json!(view_count))
    .with_meta("object_count", json!(table_count + view_count));

    // Recompute counts from the payload values to avoid drift if fields are reordered later.
    let encoded = serde_json::to_string(&envelope).map_err(|error| {
        Error::new(QueryEnvelopeCommandFailure::new(
            QueryEnvelope::error(
                "query.schema",
                "query_response_encode_failed",
                "failed to encode query response",
            )
            .with_error_details(json!({ "cause": format!("{error:#}") })),
        ))
    })?;
    println!("{encoded}");

    Ok(())
}

fn load_answerability_corpus(path: &Path) -> Result<AnswerabilityCorpus> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read answerability corpus: {}", path.display()))?;
    let corpus: AnswerabilityCorpus = serde_json::from_str(&raw).with_context(|| {
        format!(
            "failed to parse answerability corpus JSON: {}",
            path.display()
        )
    })?;

    if corpus.schema_version != ANSWERABILITY_CORPUS_SCHEMA_VERSION {
        bail!(
            "unsupported corpus schema version `{}` (expected `{ANSWERABILITY_CORPUS_SCHEMA_VERSION}`)",
            corpus.schema_version
        );
    }
    if corpus.questions.is_empty() {
        bail!("answerability corpus must contain at least one question");
    }

    let mut question_ids = BTreeSet::new();
    for question in &corpus.questions {
        if question.id.trim().is_empty() {
            bail!("corpus contains question with empty id");
        }
        if !question_ids.insert(question.id.clone()) {
            bail!("corpus contains duplicate question id `{}`", question.id);
        }
        if question.expected_answer_contract.must_include.is_empty() {
            bail!(
                "question `{}` must declare at least one `must_include` field",
                question.id
            );
        }
    }

    Ok(corpus)
}

fn build_benchmark_preflight(
    connection: &rusqlite::Connection,
) -> Result<AnswerabilityBenchmarkPreflight> {
    let schema_objects = load_schema_descriptors(connection, false)?;
    let (schema_table_count, schema_view_count) =
        schema_objects
            .iter()
            .fold((0usize, 0usize), |(tables, views), object| {
                match object.kind.as_str() {
                    "table" => (tables + 1, views),
                    "view" => (tables, views + 1),
                    _ => (tables, views),
                }
            });

    let concepts = [
        tool_calls_concept(false),
        sessions_concept(false),
        adapters_concept(false),
        quality_concept(false),
    ];
    let relations = [
        catalog_relation(
            "v_tool_calls",
            "view",
            "tool call/result pairing, duration, and pairing status",
        ),
        catalog_relation(
            "v_sessions",
            "view",
            "session-level rollups for event and tool activity",
        ),
        catalog_relation(
            "v_adapters",
            "view",
            "adapter-level usage/reliability aggregate counters",
        ),
        catalog_relation(
            "v_quality",
            "view",
            "timestamp-quality and warning/error quality rollups",
        ),
        catalog_relation(
            "ingest_runs",
            "table",
            "refresh run lifecycle/status and ingest counters",
        ),
        catalog_relation(
            "ingest_watermarks",
            "table",
            "per-source freshness/staleness and watermark frontiers",
        ),
    ];

    Ok(AnswerabilityBenchmarkPreflight {
        schema_table_count,
        schema_view_count,
        semantic_concept_count: concepts.len(),
        semantic_relation_count: relations.len(),
    })
}

fn run_answerability_question(
    question: &AnswerabilityQuestion,
    connection: &rusqlite::Connection,
    row_cap: usize,
) -> AnswerabilityBenchmarkQuestionResult {
    let Some(plan) = answerability_query_plan(question.id.as_str()) else {
        return AnswerabilityBenchmarkQuestionResult {
            id: question.id.clone(),
            domain: question.domain.clone(),
            question: question.question.clone(),
            answer_kind: question.expected_answer_contract.answer_kind.clone(),
            query_interface: "query.sql".to_string(),
            sql: String::new(),
            must_include: question.expected_answer_contract.must_include.clone(),
            ordering: question.expected_answer_contract.ordering.clone(),
            threshold: question.expected_answer_contract.threshold.clone(),
            queryability_assumptions: question.queryability_assumptions.clone(),
            rationale: question.rationale.clone(),
            passed: false,
            row_count: 0,
            truncated: false,
            duration_ms: 0,
            column_names: Vec::new(),
            missing_required_fields: Vec::new(),
            warnings: Vec::new(),
            failure_code: Some("query_benchmark_plan_missing".to_string()),
            failure_message: Some(
                "no benchmark query plan is mapped for this question id".to_string(),
            ),
        };
    };

    if let Err(violation) = validate_read_only_sql(plan.sql) {
        return AnswerabilityBenchmarkQuestionResult {
            id: question.id.clone(),
            domain: question.domain.clone(),
            question: question.question.clone(),
            answer_kind: question.expected_answer_contract.answer_kind.clone(),
            query_interface: plan.query_interface.to_string(),
            sql: plan.sql.to_string(),
            must_include: question.expected_answer_contract.must_include.clone(),
            ordering: question.expected_answer_contract.ordering.clone(),
            threshold: question.expected_answer_contract.threshold.clone(),
            queryability_assumptions: question.queryability_assumptions.clone(),
            rationale: question.rationale.clone(),
            passed: false,
            row_count: 0,
            truncated: false,
            duration_ms: 0,
            column_names: Vec::new(),
            missing_required_fields: Vec::new(),
            warnings: Vec::new(),
            failure_code: Some("sql_guardrail_violation".to_string()),
            failure_message: Some(violation.message),
        };
    }

    let started = std::time::Instant::now();
    let execution = execute_read_only_query(connection, plan.sql, &[], row_cap);
    let duration_ms = started.elapsed().as_millis() as u64;

    let execution = match execution {
        Ok(result) => result,
        Err(error) => {
            return AnswerabilityBenchmarkQuestionResult {
                id: question.id.clone(),
                domain: question.domain.clone(),
                question: question.question.clone(),
                answer_kind: question.expected_answer_contract.answer_kind.clone(),
                query_interface: plan.query_interface.to_string(),
                sql: plan.sql.to_string(),
                must_include: question.expected_answer_contract.must_include.clone(),
                ordering: question.expected_answer_contract.ordering.clone(),
                threshold: question.expected_answer_contract.threshold.clone(),
                queryability_assumptions: question.queryability_assumptions.clone(),
                rationale: question.rationale.clone(),
                passed: false,
                row_count: 0,
                truncated: false,
                duration_ms,
                column_names: Vec::new(),
                missing_required_fields: Vec::new(),
                warnings: Vec::new(),
                failure_code: Some("query_execution_failed".to_string()),
                failure_message: Some(format!("{error:#}")),
            };
        }
    };

    let missing_required_fields = question
        .expected_answer_contract
        .must_include
        .iter()
        .filter(|field| {
            !execution
                .column_names
                .iter()
                .any(|column| column.as_str() == field.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();

    let mut warnings = Vec::new();
    if execution.truncated {
        warnings.push("query result truncated by row_cap".to_string());
    }
    if execution.row_count == 0 {
        warnings.push("query returned zero rows; shape validated by projected columns".to_string());
    }

    let (ordering_ok, ordering_warning) = validate_ordering_contract(
        &execution.rows,
        question.expected_answer_contract.ordering.as_deref(),
    );
    if let Some(message) = ordering_warning {
        warnings.push(message);
    }

    let passed = missing_required_fields.is_empty() && ordering_ok;
    let failure_code = (!passed).then_some("answer_contract_mismatch".to_string());
    let failure_message = (!passed).then(|| {
        let mut failures = Vec::new();
        if !missing_required_fields.is_empty() {
            failures.push(format!(
                "missing required fields: {}",
                missing_required_fields.join(", ")
            ));
        }
        if !ordering_ok {
            failures.push("ordering contract not satisfied".to_string());
        }
        failures.join("; ")
    });

    AnswerabilityBenchmarkQuestionResult {
        id: question.id.clone(),
        domain: question.domain.clone(),
        question: question.question.clone(),
        answer_kind: question.expected_answer_contract.answer_kind.clone(),
        query_interface: plan.query_interface.to_string(),
        sql: plan.sql.to_string(),
        must_include: question.expected_answer_contract.must_include.clone(),
        ordering: question.expected_answer_contract.ordering.clone(),
        threshold: question.expected_answer_contract.threshold.clone(),
        queryability_assumptions: question.queryability_assumptions.clone(),
        rationale: question.rationale.clone(),
        passed,
        row_count: execution.row_count,
        truncated: execution.truncated,
        duration_ms,
        column_names: execution.column_names,
        missing_required_fields,
        warnings,
        failure_code,
        failure_message,
    }
}

fn build_benchmark_summary(
    question_reports: &[AnswerabilityBenchmarkQuestionResult],
) -> AnswerabilityBenchmarkSummary {
    let total_questions = question_reports.len();
    let passed_questions = question_reports
        .iter()
        .filter(|report| report.passed)
        .count();
    let failed_questions = total_questions.saturating_sub(passed_questions);

    let mut domain_counters: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for report in question_reports {
        let entry = domain_counters
            .entry(report.domain.clone())
            .or_insert((0, 0));
        entry.0 += 1;
        if report.passed {
            entry.1 += 1;
        }
    }

    let per_domain = domain_counters
        .into_iter()
        .map(
            |(domain, (domain_total, domain_passed))| AnswerabilityBenchmarkDomainSummary {
                domain,
                total_questions: domain_total,
                passed_questions: domain_passed,
                failed_questions: domain_total.saturating_sub(domain_passed),
                score_pct: percentage(domain_passed, domain_total),
            },
        )
        .collect::<Vec<_>>();

    AnswerabilityBenchmarkSummary {
        total_questions,
        passed_questions,
        failed_questions,
        score_pct: percentage(passed_questions, total_questions),
        per_domain,
    }
}

fn evaluate_release_gate(summary: &AnswerabilityBenchmarkSummary) -> AnswerabilityReleaseGate {
    let failing_domains = summary
        .per_domain
        .iter()
        .filter(|domain| domain.score_pct < ANSWERABILITY_MIN_DOMAIN_SCORE_PCT)
        .map(|domain| domain.domain.clone())
        .collect::<Vec<_>>();

    let mut failed_checks = Vec::new();
    if summary.score_pct < ANSWERABILITY_MIN_TOTAL_SCORE_PCT {
        failed_checks.push(format!(
            "overall score {:.2} is below minimum {:.2}",
            summary.score_pct, ANSWERABILITY_MIN_TOTAL_SCORE_PCT
        ));
    }
    if summary.failed_questions > ANSWERABILITY_MAX_FAILED_QUESTIONS {
        failed_checks.push(format!(
            "failed question count {} exceeds maximum {}",
            summary.failed_questions, ANSWERABILITY_MAX_FAILED_QUESTIONS
        ));
    }
    if !failing_domains.is_empty() {
        failed_checks.push(format!(
            "domains below minimum score {:.2}: {}",
            ANSWERABILITY_MIN_DOMAIN_SCORE_PCT,
            failing_domains.join(", ")
        ));
    }

    AnswerabilityReleaseGate {
        minimum_total_score_pct: ANSWERABILITY_MIN_TOTAL_SCORE_PCT,
        minimum_domain_score_pct: ANSWERABILITY_MIN_DOMAIN_SCORE_PCT,
        maximum_failed_questions: ANSWERABILITY_MAX_FAILED_QUESTIONS,
        observed_total_score_pct: summary.score_pct,
        observed_failed_questions: summary.failed_questions,
        failing_domains,
        passed: failed_checks.is_empty(),
        failed_checks,
    }
}

fn percentage(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    ((numerator as f64 * 10_000.0) / denominator as f64).round() / 100.0
}

fn query_benchmark_artifact_path(out_dir: &Path) -> PathBuf {
    out_dir.join(DEFAULT_BENCHMARK_ARTIFACT_PATH)
}

fn write_benchmark_artifact(path: &Path, report: &AnswerabilityBenchmarkReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create benchmark artifact dir: {}",
                parent.display()
            )
        })?;
    }
    let encoded = serde_json::to_string_pretty(report)
        .context("failed to encode answerability benchmark artifact")?;
    std::fs::write(path, encoded)
        .with_context(|| format!("failed to write benchmark artifact: {}", path.display()))?;
    Ok(())
}

fn validate_ordering_contract(rows: &[Value], ordering: Option<&str>) -> (bool, Option<String>) {
    let Some(ordering_spec) = ordering else {
        return (true, None);
    };
    if rows.len() <= 1 {
        return (true, None);
    }

    let Some((ordering_field, descending)) = parse_ordering_spec(ordering_spec) else {
        return (
            false,
            Some(format!(
                "ordering contract `{ordering_spec}` is unsupported by benchmark validator"
            )),
        );
    };

    for pair in rows.windows(2) {
        let Some(left) = pair[0].as_object() else {
            return (
                false,
                Some("ordering check expected object rows but found non-object row".to_string()),
            );
        };
        let Some(right) = pair[1].as_object() else {
            return (
                false,
                Some("ordering check expected object rows but found non-object row".to_string()),
            );
        };

        let Some(left_value) = left.get(ordering_field) else {
            return (
                false,
                Some(format!(
                    "ordering field `{ordering_field}` missing from one or more result rows"
                )),
            );
        };
        let Some(right_value) = right.get(ordering_field) else {
            return (
                false,
                Some(format!(
                    "ordering field `{ordering_field}` missing from one or more result rows"
                )),
            );
        };

        let Some(comparison) = compare_ordering_values(left_value, right_value) else {
            return (
                false,
                Some(format!(
                    "ordering field `{ordering_field}` has non-comparable values"
                )),
            );
        };

        let out_of_order = if descending {
            comparison == Ordering::Less
        } else {
            comparison == Ordering::Greater
        };
        if out_of_order {
            return (
                false,
                Some(format!(
                    "rows are not sorted by `{ordering_field}` in expected {} order",
                    if descending {
                        "descending"
                    } else {
                        "ascending"
                    }
                )),
            );
        }
    }

    (true, None)
}

fn parse_ordering_spec(ordering_spec: &str) -> Option<(&str, bool)> {
    if let Some(field) = ordering_spec.strip_suffix("_asc") {
        Some((field, false))
    } else {
        ordering_spec
            .strip_suffix("_desc")
            .map(|field| (field, true))
    }
}

fn compare_ordering_values(left: &Value, right: &Value) -> Option<Ordering> {
    if let (Some(left_num), Some(right_num)) = (left.as_f64(), right.as_f64()) {
        return left_num.partial_cmp(&right_num);
    }
    if let (Some(left_str), Some(right_str)) = (left.as_str(), right.as_str()) {
        return Some(left_str.cmp(right_str));
    }
    if let (Some(left_bool), Some(right_bool)) = (left.as_bool(), right.as_bool()) {
        return Some(left_bool.cmp(&right_bool));
    }
    (left == right).then_some(Ordering::Equal)
}

fn answerability_query_plan(question_id: &str) -> Option<AnswerabilityBenchmarkQueryPlan> {
    let sql = match question_id {
        "q-usage-001" => SQL_Q_USAGE_001,
        "q-usage-002" => SQL_Q_USAGE_002,
        "q-usage-003" => SQL_Q_USAGE_003,
        "q-usage-004" => SQL_Q_USAGE_004,
        "q-performance-001" => SQL_Q_PERFORMANCE_001,
        "q-performance-002" => SQL_Q_PERFORMANCE_002,
        "q-performance-003" => SQL_Q_PERFORMANCE_003,
        "q-performance-004" => SQL_Q_PERFORMANCE_004,
        "q-freshness-001" => SQL_Q_FRESHNESS_001,
        "q-freshness-002" => SQL_Q_FRESHNESS_002,
        "q-freshness-003" => SQL_Q_FRESHNESS_003,
        "q-freshness-004" => SQL_Q_FRESHNESS_004,
        "q-reliability-001" => SQL_Q_RELIABILITY_001,
        "q-reliability-002" => SQL_Q_RELIABILITY_002,
        "q-reliability-003" => SQL_Q_RELIABILITY_003,
        "q-reliability-004" => SQL_Q_RELIABILITY_004,
        _ => return None,
    };

    Some(AnswerabilityBenchmarkQueryPlan {
        query_interface: "query.sql",
        sql,
    })
}

const SQL_Q_USAGE_001: &str = r#"
WITH bounds AS (
    SELECT MAX(timestamp_unix_ms) AS max_ts
    FROM agentlog_events
),
windowed AS (
    SELECT COALESCE(tool_name, 'unknown') AS tool_name
    FROM v_tool_calls, bounds
    WHERE call_event_id IS NOT NULL
      AND (
            bounds.max_ts IS NULL
            OR call_timestamp_unix_ms >= bounds.max_ts - 604800000
      )
)
SELECT tool_name, COUNT(*) AS invocation_count
FROM windowed
GROUP BY tool_name
ORDER BY invocation_count DESC, tool_name ASC
LIMIT 50
"#;

const SQL_Q_USAGE_002: &str = r#"
WITH bounds AS (
    SELECT MAX(timestamp_unix_ms) AS max_ts
    FROM agentlog_events
),
windowed AS (
    SELECT timestamp_unix_ms, session_id
    FROM agentlog_events, bounds
    WHERE session_id IS NOT NULL
      AND session_id != ''
      AND (
            bounds.max_ts IS NULL
            OR timestamp_unix_ms >= bounds.max_ts - 1209600000
      )
)
SELECT strftime('%Y-%m-%d', timestamp_unix_ms / 1000, 'unixepoch') AS day_utc,
       COUNT(DISTINCT session_id) AS unique_sessions
FROM windowed
GROUP BY day_utc
ORDER BY day_utc ASC
"#;

const SQL_Q_USAGE_003: &str = r#"
WITH totals AS (
    SELECT COUNT(*) AS total_events
    FROM agentlog_events
),
by_adapter AS (
    SELECT adapter_name, COUNT(*) AS event_count
    FROM agentlog_events
    GROUP BY adapter_name
)
SELECT by_adapter.adapter_name,
       by_adapter.event_count,
       CASE
           WHEN totals.total_events = 0 THEN 0.0
           ELSE ROUND((by_adapter.event_count * 100.0) / totals.total_events, 2)
       END AS event_share_pct
FROM by_adapter
CROSS JOIN totals
ORDER BY by_adapter.event_count DESC, by_adapter.adapter_name ASC
"#;

const SQL_Q_USAGE_004: &str = r#"
SELECT conversation_id, COUNT(*) AS tool_call_count
FROM v_tool_calls
WHERE call_event_id IS NOT NULL
  AND conversation_id IS NOT NULL
  AND conversation_id != ''
GROUP BY conversation_id
ORDER BY tool_call_count DESC, conversation_id ASC
LIMIT 50
"#;

const SQL_Q_PERFORMANCE_001: &str = r#"
WITH bounds AS (
    SELECT MAX(call_timestamp_unix_ms) AS max_ts
    FROM v_tool_calls
),
windowed AS (
    SELECT COALESCE(tool_name, 'unknown') AS tool_name, duration_ms
    FROM v_tool_calls, bounds
    WHERE pairing_status = 'paired'
      AND duration_ms IS NOT NULL
      AND (
            bounds.max_ts IS NULL
            OR call_timestamp_unix_ms >= bounds.max_ts - 86400000
      )
),
ranked AS (
    SELECT
        tool_name,
        duration_ms,
        ROW_NUMBER() OVER (PARTITION BY tool_name ORDER BY duration_ms ASC) AS rn,
        COUNT(*) OVER (PARTITION BY tool_name) AS cnt
    FROM windowed
)
SELECT
    tool_name,
    MIN(CASE WHEN rn >= ((cnt + 1) / 2) THEN duration_ms END) AS p50_duration_ms,
    MIN(CASE WHEN rn >= ((cnt * 95 + 99) / 100) THEN duration_ms END) AS p95_duration_ms
FROM ranked
GROUP BY tool_name
ORDER BY tool_name ASC
"#;

const SQL_Q_PERFORMANCE_002: &str = r#"
SELECT session_id,
       SUM(duration_ms) AS total_tool_duration_ms
FROM v_tool_calls
WHERE pairing_status = 'paired'
  AND session_id IS NOT NULL
  AND session_id != ''
  AND duration_ms IS NOT NULL
GROUP BY session_id
ORDER BY total_tool_duration_ms DESC, session_id ASC
LIMIT 50
"#;

const SQL_Q_PERFORMANCE_003: &str = r#"
SELECT adapter_name,
       ROUND(
           100.0 * SUM(CASE WHEN duration_ms > 2000 THEN 1 ELSE 0 END) / COUNT(*),
           2
       ) AS slow_call_pct
FROM v_tool_calls
WHERE pairing_status = 'paired'
  AND duration_ms IS NOT NULL
GROUP BY adapter_name
ORDER BY slow_call_pct DESC, adapter_name ASC
"#;

const SQL_Q_PERFORMANCE_004: &str = r#"
WITH top_tools AS (
    SELECT COALESCE(tool_name, 'unknown') AS tool_name, COUNT(*) AS invocation_count
    FROM v_tool_calls
    WHERE pairing_status = 'paired'
      AND duration_ms IS NOT NULL
    GROUP BY COALESCE(tool_name, 'unknown')
    ORDER BY invocation_count DESC, tool_name ASC
    LIMIT 3
),
hourly AS (
    SELECT
        strftime('%Y-%m-%dT%H:00:00Z', call_timestamp_unix_ms / 1000, 'unixepoch') AS hour_utc,
        COALESCE(tool_name, 'unknown') AS tool_name,
        duration_ms
    FROM v_tool_calls
    WHERE pairing_status = 'paired'
      AND duration_ms IS NOT NULL
      AND COALESCE(tool_name, 'unknown') IN (SELECT tool_name FROM top_tools)
),
ranked AS (
    SELECT
        hour_utc,
        tool_name,
        duration_ms,
        ROW_NUMBER() OVER (PARTITION BY hour_utc, tool_name ORDER BY duration_ms ASC) AS rn,
        COUNT(*) OVER (PARTITION BY hour_utc, tool_name) AS cnt
    FROM hourly
)
SELECT
    hour_utc,
    tool_name,
    MIN(CASE WHEN rn >= ((cnt + 1) / 2) THEN duration_ms END) AS median_duration_ms
FROM ranked
GROUP BY hour_utc, tool_name
ORDER BY hour_utc ASC, tool_name ASC
"#;

const SQL_Q_FRESHNESS_001: &str = r#"
SELECT source_kind,
       MAX(refreshed_at_utc) AS last_successful_refresh_at_utc
FROM ingest_watermarks
GROUP BY source_kind
ORDER BY source_kind ASC
"#;

const SQL_Q_FRESHNESS_002: &str = r#"
WITH latest AS (
    SELECT MAX(CAST(strftime('%s', refreshed_at_utc) AS INTEGER)) AS max_refreshed_s
    FROM ingest_watermarks
)
SELECT
    source_key,
    staleness_state,
    CASE
        WHEN latest.max_refreshed_s IS NULL THEN 0
        ELSE (
            latest.max_refreshed_s - COALESCE(CAST(strftime('%s', refreshed_at_utc) AS INTEGER), latest.max_refreshed_s)
        ) * 1000
    END AS staleness_age_ms
FROM ingest_watermarks
CROSS JOIN latest
WHERE staleness_state = 'stale'
ORDER BY staleness_age_ms DESC, source_key ASC
"#;

const SQL_Q_FRESHNESS_003: &str = r#"
WITH recent AS (
    SELECT status
    FROM ingest_runs
    ORDER BY started_at_utc DESC
    LIMIT 30
)
SELECT
    CASE
        WHEN COUNT(*) = 0 THEN 0.0
        ELSE ROUND(100.0 * SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) / COUNT(*), 2)
    END AS refresh_success_rate_pct
FROM recent
"#;

const SQL_Q_FRESHNESS_004: &str = r#"
WITH latest_success AS (
    SELECT events_written
    FROM ingest_runs
    WHERE status = 'success'
    ORDER BY started_at_utc DESC
    LIMIT 1
)
SELECT COALESCE((SELECT events_written FROM latest_success), 0) AS events_written
"#;

const SQL_Q_RELIABILITY_001: &str = r#"
WITH bounds AS (
    SELECT MAX(timestamp_unix_ms) AS max_ts
    FROM agentlog_events
),
windowed AS (
    SELECT warnings_json
    FROM agentlog_events, bounds
    WHERE warnings_json != '[]'
      AND (
            bounds.max_ts IS NULL
            OR timestamp_unix_ms >= bounds.max_ts - 604800000
      )
)
SELECT warnings_json AS warning_category, COUNT(*) AS warning_count
FROM windowed
GROUP BY warnings_json
ORDER BY warning_count DESC, warning_category ASC
LIMIT 50
"#;

const SQL_Q_RELIABILITY_002: &str = r#"
SELECT
    adapter_name,
    SUM(CASE WHEN timestamp_quality = 'fallback' THEN event_count ELSE 0 END) AS fallback_timestamp_count
FROM v_quality
GROUP BY adapter_name
ORDER BY fallback_timestamp_count DESC, adapter_name ASC
"#;

const SQL_Q_RELIABILITY_003: &str = r#"
WITH latest_run AS (
    SELECT ingest_run_id AS run_id
    FROM ingest_runs
    ORDER BY started_at_utc DESC
    LIMIT 1
),
duplicates AS (
    SELECT canonical_hash, COUNT(*) AS duplicate_rows
    FROM agentlog_events
    WHERE run_id = (SELECT run_id FROM latest_run)
    GROUP BY canonical_hash
    HAVING COUNT(*) > 1
)
SELECT
    CASE WHEN COUNT(*) > 0 THEN 1 ELSE 0 END AS has_duplicate_canonical_hashes,
    COALESCE(SUM(duplicate_rows - 1), 0) AS duplicate_count
FROM duplicates
"#;

const SQL_Q_RELIABILITY_004: &str = r#"
SELECT
    CASE
        WHEN COUNT(*) = 0 THEN 0.0
        ELSE ROUND(
            100.0 * SUM(CASE WHEN status IN ('failed', 'partial_failure') THEN 1 ELSE 0 END) / COUNT(*),
            2
        )
    END AS degraded_run_fraction_pct
FROM ingest_runs
"#;

fn load_schema_descriptors(
    connection: &rusqlite::Connection,
    include_internal: bool,
) -> Result<Vec<SchemaObjectDescriptor>> {
    let mut statement = connection
        .prepare(
            "SELECT name, type
             FROM sqlite_schema
             WHERE type IN ('table', 'view')
             ORDER BY CASE type WHEN 'table' THEN 0 ELSE 1 END, name ASC",
        )
        .context("failed to prepare sqlite_schema introspection query")?;

    let object_rows = statement
        .query_map([], |row| {
            Ok((row.get::<usize, String>(0)?, row.get::<usize, String>(1)?))
        })
        .context("failed to execute sqlite_schema introspection query")?;

    let mut objects = Vec::new();
    for row in object_rows {
        let (name, kind) = row.context("failed to decode sqlite_schema row")?;
        let internal = is_internal_schema_object(&name);
        if !include_internal && internal {
            continue;
        }
        let columns = load_schema_columns(connection, &name)?;
        objects.push(SchemaObjectDescriptor {
            name,
            kind,
            internal,
            columns,
        });
    }

    Ok(objects)
}

fn load_schema_columns(
    connection: &rusqlite::Connection,
    object_name: &str,
) -> Result<Vec<SchemaColumnDescriptor>> {
    let pragma_sql = format!("PRAGMA table_info({})", sqlite_single_quoted(object_name));
    let mut statement = connection
        .prepare(&pragma_sql)
        .with_context(|| format!("failed to prepare column introspection for `{object_name}`"))?;

    let column_rows = statement
        .query_map([], |row| {
            Ok(SchemaColumnDescriptor {
                ordinal: row.get::<usize, i64>(0)?,
                name: row.get::<usize, String>(1)?,
                declared_type: row.get::<usize, Option<String>>(2)?,
                nullable: row.get::<usize, i64>(3)? == 0,
                default_value_sql: row.get::<usize, Option<String>>(4)?,
                primary_key_position: row.get::<usize, i64>(5)?,
            })
        })
        .with_context(|| format!("failed to execute column introspection for `{object_name}`"))?;

    column_rows
        .map(|row| row.context("failed to decode schema column row"))
        .collect()
}

fn is_internal_schema_object(object_name: &str) -> bool {
    object_name.starts_with("sqlite_") || object_name == crate::sqlite::SCHEMA_META_TABLE
}

fn sqlite_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[derive(Debug, Clone)]
struct SqlGuardrailViolation {
    message: String,
    details: serde_json::Value,
}

fn validate_read_only_sql(raw_sql: &str) -> std::result::Result<(), SqlGuardrailViolation> {
    let candidate = strip_trailing_semicolons(raw_sql);
    if candidate.is_empty() {
        return Err(guardrail_violation(
            "SQL query is empty; provide a SELECT/CTE/EXPLAIN-SELECT statement",
            json!({"reason":"empty_statement"}),
        ));
    }

    if candidate.contains(';') {
        return Err(guardrail_violation(
            "Multi-statement SQL is not allowed; submit exactly one read-only statement",
            json!({"reason":"multi_statement"}),
        ));
    }

    let normalized = candidate.to_ascii_lowercase();
    if let Some(keyword) = first_mutating_keyword(&normalized) {
        return Err(guardrail_violation(
            format!("Mutating SQL keyword `{keyword}` is not allowed in query.sql"),
            json!({"reason":"mutating_statement","detected_keyword":keyword}),
        ));
    }

    let allowed = normalized.starts_with("select")
        || normalized.starts_with("with")
        || normalized.starts_with("explain select")
        || normalized.starts_with("explain query plan select");
    if !allowed {
        let leading_keyword = leading_keyword(&normalized);
        return Err(guardrail_violation(
            "Only SELECT, WITH ... SELECT, and EXPLAIN ... SELECT statements are allowed",
            json!({"reason":"unsupported_statement","leading_keyword":leading_keyword}),
        ));
    }

    Ok(())
}

fn strip_trailing_semicolons(raw_sql: &str) -> &str {
    let mut candidate = raw_sql.trim();
    while let Some(stripped) = candidate.strip_suffix(';') {
        candidate = stripped.trim_end();
    }
    candidate
}

fn first_mutating_keyword(normalized_sql: &str) -> Option<String> {
    const MUTATING_KEYWORDS: &[&str] = &[
        "insert", "update", "delete", "create", "alter", "drop", "replace", "truncate", "attach",
        "detach", "pragma", "vacuum", "reindex", "analyze", "begin", "commit", "rollback",
    ];

    normalized_sql
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .find_map(|token| {
            MUTATING_KEYWORDS
                .contains(&token)
                .then_some(token.to_string())
        })
}

fn leading_keyword(normalized_sql: &str) -> String {
    normalized_sql
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .find(|token| !token.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

fn guardrail_violation(
    message: impl Into<String>,
    details: serde_json::Value,
) -> SqlGuardrailViolation {
    SqlGuardrailViolation {
        message: message.into(),
        details: json!({
            "allowed_forms":[
                "SELECT ...",
                "WITH ... SELECT ...",
                "EXPLAIN SELECT ...",
                "EXPLAIN QUERY PLAN SELECT ..."
            ],
            "guardrail":"read_only_sql_single_statement",
            "violation": details
        }),
    }
}

#[derive(Debug, Clone)]
struct QuerySqlProfile {
    statement_kind: &'static str,
    has_where: bool,
    has_group_by: bool,
    has_order_by: bool,
    has_limit: bool,
    uses_explain: bool,
    likely_full_scan: bool,
    sql_length_bytes: usize,
}

impl QuerySqlProfile {
    fn runtime_diagnostics(
        &self,
        duration_ms: u64,
        row_cap: usize,
        row_count: usize,
        truncated: bool,
    ) -> Value {
        json!({
            "statement_kind": self.statement_kind,
            "latency_bucket": classify_latency_bucket(duration_ms),
            "has_where": self.has_where,
            "has_group_by": self.has_group_by,
            "has_order_by": self.has_order_by,
            "has_limit": self.has_limit,
            "uses_explain": self.uses_explain,
            "likely_full_scan": self.likely_full_scan,
            "sql_length_bytes": self.sql_length_bytes,
            "returned_rows": row_count,
            "row_cap": row_cap,
            "truncation_reason": if truncated { "row_cap_reached" } else { "none" }
        })
    }
}

fn analyze_sql_profile(raw_sql: &str) -> QuerySqlProfile {
    let trimmed = strip_trailing_semicolons(raw_sql);
    let normalized = trimmed.to_ascii_lowercase();
    let normalized_whitespace = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let statement_kind = if normalized_whitespace.starts_with("explain query plan select") {
        "explain_query_plan_select"
    } else if normalized_whitespace.starts_with("explain select") {
        "explain_select"
    } else if normalized_whitespace.starts_with("with") {
        "with_select"
    } else if normalized_whitespace.starts_with("select") {
        "select"
    } else {
        "other"
    };
    let has_where = normalized_whitespace.contains(" where ");
    let has_group_by = normalized_whitespace.contains(" group by ");
    let has_order_by = normalized_whitespace.contains(" order by ");
    let has_limit = normalized_whitespace.contains(" limit ");
    let uses_explain =
        statement_kind == "explain_select" || statement_kind == "explain_query_plan_select";
    let likely_full_scan =
        matches!(statement_kind, "select" | "with_select") && !has_where && !has_limit;

    QuerySqlProfile {
        statement_kind,
        has_where,
        has_group_by,
        has_order_by,
        has_limit,
        uses_explain,
        likely_full_scan,
        sql_length_bytes: trimmed.len(),
    }
}

fn classify_latency_bucket(duration_ms: u64) -> &'static str {
    match duration_ms {
        0..=250 => "fast",
        251..=1_500 => "moderate",
        1_501..=10_000 => "slow",
        _ => "very_slow",
    }
}

#[derive(Debug)]
struct QueryExecutionResult {
    column_names: Vec<String>,
    rows: Vec<Value>,
    row_count: usize,
    truncated: bool,
}

fn execute_read_only_query(
    connection: &rusqlite::Connection,
    sql: &str,
    params: &[SqlValue],
    row_cap: usize,
) -> Result<QueryExecutionResult> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| Error::new(error).context("failed to prepare query"))?;
    let column_names = statement
        .column_names()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    let mut rows = statement
        .query(params_from_iter(params.iter()))
        .map_err(|error| Error::new(error).context("failed to execute query"))?;
    let mut result_rows = Vec::new();
    let mut truncated = false;
    while let Some(row) = rows
        .next()
        .map_err(|error| Error::new(error).context("failed to fetch query row"))?
    {
        if result_rows.len() >= row_cap {
            truncated = true;
            break;
        }

        let mut record = serde_json::Map::new();
        for (index, column_name) in column_names.iter().enumerate() {
            let value = row
                .get::<usize, SqlValue>(index)
                .map_err(|error| Error::new(error).context("failed to decode query column"))?;
            record.insert(column_name.clone(), json_value_from_sql(value));
        }
        result_rows.push(Value::Object(record));
    }

    Ok(QueryExecutionResult {
        column_names,
        row_count: result_rows.len(),
        rows: result_rows,
        truncated,
    })
}

fn parse_query_params(params_json: Option<&str>) -> Result<Vec<SqlValue>> {
    let Some(raw) = params_json else {
        return Ok(Vec::new());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let parsed =
        serde_json::from_str::<Value>(trimmed).context("params must be valid JSON if provided")?;
    match parsed {
        Value::Null => Ok(Vec::new()),
        Value::Array(values) => values
            .into_iter()
            .map(sql_value_from_json)
            .collect::<Result<Vec<_>>>(),
        value => Ok(vec![sql_value_from_json(value)?]),
    }
}

fn sql_value_from_json(value: Value) -> Result<SqlValue> {
    match value {
        Value::Null => Ok(SqlValue::Null),
        Value::Bool(flag) => Ok(SqlValue::Integer(i64::from(flag))),
        Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Ok(SqlValue::Integer(integer))
            } else if let Some(unsigned) = number.as_u64() {
                i64::try_from(unsigned)
                    .map(SqlValue::Integer)
                    .map_err(|_| Error::msg("params integer exceeds sqlite INTEGER range"))
            } else if let Some(real) = number.as_f64() {
                Ok(SqlValue::Real(real))
            } else {
                Err(Error::msg("unsupported numeric param value"))
            }
        }
        Value::String(text) => Ok(SqlValue::Text(text)),
        Value::Array(_) | Value::Object(_) => {
            Err(Error::msg("params entries must be scalar JSON values"))
        }
    }
}

fn json_value_from_sql(value: SqlValue) -> Value {
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(value) => json!(value),
        SqlValue::Real(value) => json!(value),
        SqlValue::Text(value) => json!(value),
        SqlValue::Blob(value) => json!(encode_blob_hex(&value)),
    }
}

fn encode_blob_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use rusqlite::types::Value as SqlValue;

    use super::{
        AnswerabilityBenchmarkDomainSummary, AnswerabilityBenchmarkSummary, analyze_sql_profile,
        evaluate_release_gate, parse_query_params, validate_read_only_sql,
    };

    #[test]
    fn allows_select_with_optional_trailing_semicolon() {
        assert!(validate_read_only_sql("SELECT 1").is_ok());
        assert!(validate_read_only_sql("select 1 ; ").is_ok());
    }

    #[test]
    fn allows_with_and_explain_select_forms() {
        assert!(
            validate_read_only_sql("WITH x AS (SELECT 1) SELECT * FROM x").is_ok(),
            "WITH ... SELECT should be allowed"
        );
        assert!(
            validate_read_only_sql("EXPLAIN QUERY PLAN SELECT * FROM agentlog_events").is_ok(),
            "EXPLAIN QUERY PLAN SELECT should be allowed"
        );
    }

    #[test]
    fn rejects_empty_multi_statement_and_mutating_sql() {
        let empty = validate_read_only_sql("   ").expect_err("empty SQL must be rejected");
        assert!(empty.message.contains("empty"));
        assert_eq!(
            empty
                .details
                .pointer("/violation/reason")
                .and_then(|v| v.as_str()),
            Some("empty_statement")
        );

        let multi = validate_read_only_sql("SELECT 1; SELECT 2")
            .expect_err("multi-statement SQL must be rejected");
        assert!(multi.message.contains("Multi-statement"));
        assert_eq!(
            multi
                .details
                .pointer("/violation/reason")
                .and_then(|v| v.as_str()),
            Some("multi_statement")
        );

        let mutating = validate_read_only_sql("INSERT INTO t VALUES (1)")
            .expect_err("mutating SQL must be rejected");
        assert!(mutating.message.contains("Mutating"));
        assert_eq!(
            mutating
                .details
                .pointer("/violation/detected_keyword")
                .and_then(|v| v.as_str()),
            Some("insert")
        );
    }

    #[test]
    fn rejects_explain_non_select_statements() {
        let violation = validate_read_only_sql("EXPLAIN DELETE FROM agentlog_events")
            .expect_err("EXPLAIN DELETE should still be rejected");
        assert_eq!(
            violation
                .details
                .pointer("/violation/detected_keyword")
                .and_then(|v| v.as_str()),
            Some("delete")
        );
    }

    #[test]
    fn params_parser_accepts_scalar_and_array_inputs() {
        let scalar = parse_query_params(Some("42")).expect("scalar params should parse");
        assert_eq!(scalar, vec![SqlValue::Integer(42)]);

        let array =
            parse_query_params(Some("[1, true, null, \"x\"]")).expect("array params should parse");
        assert_eq!(
            array,
            vec![
                SqlValue::Integer(1),
                SqlValue::Integer(1),
                SqlValue::Null,
                SqlValue::Text("x".to_string())
            ]
        );
    }

    #[test]
    fn sql_profile_reports_shape_and_scan_hints() {
        let profile =
            analyze_sql_profile("select * from agentlog_events where run_id = ?1 limit 5");
        assert_eq!(profile.statement_kind, "select");
        assert!(profile.has_where);
        assert!(profile.has_limit);
        assert!(!profile.likely_full_scan);
    }

    #[test]
    fn release_gate_passes_for_high_scoring_summary() {
        let summary = AnswerabilityBenchmarkSummary {
            total_questions: 16,
            passed_questions: 16,
            failed_questions: 0,
            score_pct: 100.0,
            per_domain: vec![
                AnswerabilityBenchmarkDomainSummary {
                    domain: "freshness".to_string(),
                    total_questions: 4,
                    passed_questions: 4,
                    failed_questions: 0,
                    score_pct: 100.0,
                },
                AnswerabilityBenchmarkDomainSummary {
                    domain: "usage".to_string(),
                    total_questions: 4,
                    passed_questions: 4,
                    failed_questions: 0,
                    score_pct: 100.0,
                },
            ],
        };

        let gate = evaluate_release_gate(&summary);
        assert!(gate.passed);
        assert!(gate.failed_checks.is_empty());
        assert!(gate.failing_domains.is_empty());
    }

    #[test]
    fn release_gate_fails_when_domain_and_total_scores_drop() {
        let summary = AnswerabilityBenchmarkSummary {
            total_questions: 10,
            passed_questions: 8,
            failed_questions: 2,
            score_pct: 80.0,
            per_domain: vec![
                AnswerabilityBenchmarkDomainSummary {
                    domain: "freshness".to_string(),
                    total_questions: 4,
                    passed_questions: 4,
                    failed_questions: 0,
                    score_pct: 100.0,
                },
                AnswerabilityBenchmarkDomainSummary {
                    domain: "performance".to_string(),
                    total_questions: 6,
                    passed_questions: 4,
                    failed_questions: 2,
                    score_pct: 66.67,
                },
            ],
        };

        let gate = evaluate_release_gate(&summary);
        assert!(!gate.passed);
        assert_eq!(gate.observed_failed_questions, 2);
        assert_eq!(gate.failing_domains, vec!["performance".to_string()]);
        assert!(
            gate.failed_checks
                .iter()
                .any(|message| message.contains("overall score")),
            "expected overall-score gate failure detail"
        );
        assert!(
            gate.failed_checks
                .iter()
                .any(|message| message.contains("failed question count")),
            "expected failed-question gate failure detail"
        );
    }
}
