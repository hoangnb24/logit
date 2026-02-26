use std::path::PathBuf;

use anyhow::{Error, Result};
use clap::{Args, Subcommand};
use serde_json::json;

use crate::config::RuntimePaths;
use crate::ingest::{
    build_ingest_report_artifact, default_plan_from_paths, ingest_report_artifact_path,
    run_refresh, write_ingest_report_artifact,
};
use crate::models::{QueryEnvelope, QueryEnvelopeCommandFailure};

#[derive(Debug, Clone, Args)]
pub struct IngestArgs {
    #[command(subcommand)]
    pub command: IngestCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum IngestCommand {
    Refresh(IngestRefreshArgs),
}

#[derive(Debug, Clone, Args)]
pub struct IngestRefreshArgs {
    #[arg(long, value_name = "PATH")]
    pub source_root: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub fail_fast: bool,
}

pub fn run(args: &IngestArgs, runtime_paths: &RuntimePaths) -> Result<()> {
    match &args.command {
        IngestCommand::Refresh(refresh_args) => run_refresh_command(refresh_args, runtime_paths),
    }
}

fn run_refresh_command(args: &IngestRefreshArgs, runtime_paths: &RuntimePaths) -> Result<()> {
    let source_root = args
        .source_root
        .as_deref()
        .unwrap_or(runtime_paths.cwd.as_path());
    let plan = default_plan_from_paths(&runtime_paths.out_dir, source_root, args.fail_fast);
    let artifact_path = ingest_report_artifact_path(&runtime_paths.out_dir);

    let report = match run_refresh(&plan) {
        Ok(report) => report,
        Err(error) => {
            let code = classify_ingest_error_code(&error);
            let envelope = QueryEnvelope::error("ingest.refresh", code, "ingest refresh failed")
                .with_meta("fail_fast", json!(args.fail_fast))
                .with_meta(
                    "events_jsonl_path",
                    json!(plan.events_jsonl_path.display().to_string()),
                )
                .with_meta("sqlite_path", json!(plan.sqlite_path.display().to_string()))
                .with_error_details(json!({ "cause": format!("{error:#}") }));
            return Err(Error::new(QueryEnvelopeCommandFailure::new(envelope)));
        }
    };

    if let Err(error) = write_ingest_report_artifact(&artifact_path, &report) {
        let envelope = QueryEnvelope::error(
            "ingest.refresh",
            "ingest_report_artifact_write_failed",
            "failed to write ingest report artifact",
        )
        .with_meta("artifact_path", json!(artifact_path.display().to_string()))
        .with_error_details(json!({ "cause": format!("{error:#}") }));
        return Err(Error::new(QueryEnvelopeCommandFailure::new(envelope)));
    }

    let artifact = build_ingest_report_artifact(&report);
    let data = serde_json::to_value(artifact).map_err(|error| {
        let envelope = QueryEnvelope::error(
            "ingest.refresh",
            "ingest_report_encode_failed",
            "failed to encode ingest report",
        )
        .with_error_details(json!({ "cause": format!("{error:#}") }));
        Error::new(QueryEnvelopeCommandFailure::new(envelope))
    })?;
    let envelope = QueryEnvelope::ok("ingest.refresh", data)
        .with_meta("artifact_path", json!(artifact_path.display().to_string()))
        .with_meta("fail_fast", json!(args.fail_fast));
    let encoded = serde_json::to_string(&envelope).map_err(|error| {
        let fallback = QueryEnvelope::error(
            "ingest.refresh",
            "ingest_response_encode_failed",
            "failed to encode ingest response",
        )
        .with_error_details(json!({ "cause": format!("{error:#}") }));
        let wrapped = serde_json::to_string(&fallback)
            .unwrap_or_else(|_| "{\"ok\":false,\"command\":\"ingest.refresh\"}".to_string());
        Error::new(QueryEnvelopeCommandFailure::new(QueryEnvelope::error(
            "ingest.refresh",
            "ingest_response_encode_failed",
            wrapped,
        )))
    })?;
    println!("{encoded}");

    Ok(())
}

fn classify_ingest_error_code(error: &anyhow::Error) -> &'static str {
    let message = format!("{error:#}");
    if message.contains("failed to read normalized events file") {
        "ingest_events_missing"
    } else if message.contains("invalid events jsonl row") {
        "ingest_events_invalid"
    } else if message.contains("sqlite") {
        "ingest_sqlite_failure"
    } else {
        "ingest_refresh_failed"
    }
}
