use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::config::RuntimePaths;

#[derive(Debug, Clone, Args)]
pub struct ValidateArgs {
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,

    #[arg(long, default_value_t = false)]
    pub strict: bool,
}

#[derive(Debug)]
pub struct ValidationCommandFailure {
    pub errors: usize,
    pub first_issue: Option<String>,
}

impl std::fmt::Display for ValidationCommandFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "validation failed with {} error(s).", self.errors)?;
        if let Some(issue) = &self.first_issue {
            write!(f, " {issue}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationCommandFailure {}

pub fn run(args: &ValidateArgs, runtime_paths: &RuntimePaths) -> Result<()> {
    let mode = if args.strict {
        crate::validate::ValidationMode::Strict
    } else {
        crate::validate::ValidationMode::Baseline
    };
    println!(
        "validate: start mode={} input={} out_dir={}",
        validation_mode_key(mode),
        args.input.display(),
        runtime_paths.out_dir.display()
    );

    let report = crate::validate::validate_jsonl_file(args.input.as_path(), mode)?;
    let artifacts = crate::validate::build_artifact_layout(&runtime_paths.out_dir);
    crate::validate::write_report_artifact(&artifacts.report_json, &report)?;
    println!(
        "validate: report status={} records={} validated={} errors={} warnings={} report={}",
        validation_status_key(report.status),
        report.total_records,
        report.records_validated,
        report.errors,
        report.warnings,
        artifacts.report_json.display()
    );

    if report.errors > 0 {
        eprintln!(
            "validate: failed errors={} warnings={} next=inspect_report",
            report.errors, report.warnings
        );
        let first_issue = report
            .issues
            .first()
            .map(|issue| format!("line {}: {}", issue.line, issue.detail))
            .filter(|text| !text.is_empty());
        return Err(ValidationCommandFailure {
            errors: report.errors,
            first_issue,
        }
        .into());
    }

    println!(
        "validate: complete exit_code={} next=review {}",
        report.exit_code(),
        artifacts.report_json.display()
    );

    Ok(())
}

fn validation_mode_key(mode: crate::validate::ValidationMode) -> &'static str {
    match mode {
        crate::validate::ValidationMode::Baseline => "baseline",
        crate::validate::ValidationMode::Strict => "strict",
    }
}

fn validation_status_key(status: crate::validate::ValidationStatus) -> &'static str {
    match status {
        crate::validate::ValidationStatus::Pass => "pass",
        crate::validate::ValidationStatus::Warn => "warn",
        crate::validate::ValidationStatus::Fail => "fail",
    }
}
