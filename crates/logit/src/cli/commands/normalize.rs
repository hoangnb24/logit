use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::config::RuntimePaths;

#[derive(Debug, Clone, Args)]
pub struct NormalizeArgs {
    #[arg(long)]
    pub source_root: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub fail_fast: bool,
}

pub fn run(args: &NormalizeArgs, runtime_paths: &RuntimePaths) -> Result<()> {
    let mut plan = crate::normalize::default_plan();
    plan.fail_fast = args.fail_fast;
    let source_root = args
        .source_root
        .as_deref()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "<auto>".to_string());
    println!(
        "normalize: start fail_fast={} source_root={} out_dir={}",
        plan.fail_fast,
        source_root,
        runtime_paths.out_dir.display()
    );

    let artifacts = crate::normalize::build_artifact_layout(&runtime_paths.out_dir);
    let zsh_history = std::fs::read_to_string(runtime_paths.home_dir.join(".zsh_history"))
        .unwrap_or_else(|_| String::new());
    println!("normalize: stage orchestrate");
    let orchestration = crate::normalize::orchestrate_normalization(
        &plan,
        &runtime_paths.home_dir,
        args.source_root.as_deref(),
        &zsh_history,
    )?;
    println!(
        "normalize: checkpoint orchestrate_complete events={} dedupe_input={}",
        orchestration.events.len(),
        orchestration.dedupe_stats.input_records
    );
    for (adapter, report) in &orchestration.adapter_health {
        println!(
            "normalize: adapter_health adapter={} status={} reason={} sources_considered={} files_discovered={} files_parsed={} events_emitted={} warnings={} errors={}",
            adapter,
            report.status.as_str(),
            report.reason.as_deref().unwrap_or("none"),
            report.sources_considered,
            report.files_discovered,
            report.files_parsed,
            report.events_emitted,
            report.warnings.len(),
            report.errors.len()
        );
        for warning in &report.warnings {
            println!(
                "normalize: adapter_health_detail adapter={} level=warning detail={}",
                adapter, warning
            );
        }
        for error in &report.errors {
            println!(
                "normalize: adapter_health_detail adapter={} level=error detail={}",
                adapter, error
            );
        }
    }

    println!("normalize: stage write_normalize_artifacts");
    crate::normalize::write_events_artifact(&artifacts.events_jsonl, &orchestration.events)?;
    println!(
        "normalize: checkpoint events_written {}",
        artifacts.events_jsonl.display()
    );
    crate::normalize::write_schema_artifact(&artifacts.schema_json)?;
    println!(
        "normalize: checkpoint schema_written {}",
        artifacts.schema_json.display()
    );
    let stats =
        crate::normalize::build_normalize_stats(&orchestration.events, orchestration.dedupe_stats);
    crate::normalize::write_stats_artifact(&artifacts.stats_json, &stats)?;
    println!(
        "normalize: checkpoint stats_written {}",
        artifacts.stats_json.display()
    );

    println!("normalize: stage write_discovery_artifacts");
    let discovery_artifacts = crate::discovery::build_artifact_layout(&runtime_paths.out_dir);
    crate::discovery::write_discovery_artifacts(
        &discovery_artifacts,
        &orchestration.prioritized_sources,
        &orchestration.history_scores,
    )?;
    println!(
        "normalize: checkpoint discovery_written sources={} history={}",
        discovery_artifacts.sources_json.display(),
        discovery_artifacts.zsh_history_usage_json.display()
    );
    println!(
        "normalize: complete sources_considered={} events_emitted={} duplicates_removed={} parse_warnings={} event_warnings={} event_errors={}",
        orchestration.prioritized_sources.len(),
        stats.counts.records_emitted,
        stats.counts.duplicates_removed,
        orchestration.warnings.len(),
        stats.counts.warnings,
        stats.counts.errors
    );
    println!(
        "normalize: artifacts events={} schema={} stats={} discovery_sources={} discovery_history={}",
        artifacts.events_jsonl.display(),
        artifacts.schema_json.display(),
        artifacts.stats_json.display(),
        discovery_artifacts.sources_json.display(),
        discovery_artifacts.zsh_history_usage_json.display()
    );
    println!(
        "normalize: next `logit validate {}`",
        artifacts.events_jsonl.display()
    );

    Ok(())
}
