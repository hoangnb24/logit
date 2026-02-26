use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::config::RuntimePaths;

#[derive(Debug, Clone, Args)]
pub struct SnapshotArgs {
    #[arg(long)]
    pub source_root: Option<PathBuf>,

    #[arg(long, default_value_t = 3)]
    pub sample_size: usize,
}

pub fn run(args: &SnapshotArgs, runtime_paths: &RuntimePaths) -> Result<()> {
    let config = crate::snapshot::SnapshotConfig {
        sample_size: args.sample_size,
        redact_sensitive_values: true,
    };
    let source_root = args
        .source_root
        .as_deref()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "<auto>".to_string());
    println!(
        "snapshot: start sample_size={} source_root={} out_dir={}",
        config.sample_size,
        source_root,
        runtime_paths.out_dir.display()
    );

    let zsh_history = std::fs::read_to_string(runtime_paths.home_dir.join(".zsh_history"))
        .unwrap_or_else(|_| String::new());
    let collection = crate::snapshot::collect_snapshot_data(
        &config,
        &runtime_paths.home_dir,
        args.source_root.as_deref(),
        &zsh_history,
    )?;

    let artifacts = crate::snapshot::build_artifact_layout(&runtime_paths.out_dir);
    crate::snapshot::write_snapshot_artifacts(&artifacts, &collection)?;
    println!(
        "snapshot: complete discovered_sources={} existing_sources={} files_profiled={} records_profiled={} samples_emitted={} warnings={}",
        collection.index.counts.discovered_sources,
        collection.index.counts.existing_sources,
        collection.index.counts.files_profiled,
        collection.index.counts.records_profiled,
        collection.index.counts.samples_emitted,
        collection.index.counts.warnings
    );
    println!(
        "snapshot: artifacts index={} samples={} schema_profile={}",
        artifacts.index_json.display(),
        artifacts.samples_jsonl.display(),
        artifacts.schema_profile_json.display()
    );
    println!(
        "snapshot: next `logit inspect {} --json`",
        artifacts.index_json.display()
    );

    Ok(())
}
