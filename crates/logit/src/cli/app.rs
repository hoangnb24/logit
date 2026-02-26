use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use super::commands::{
    inspect::InspectArgs, normalize::NormalizeArgs, snapshot::SnapshotArgs, validate::ValidateArgs,
};

#[derive(Debug, Parser)]
#[command(name = "logit", version, about = "Multi-agent local log intelligence")]
pub struct Cli {
    #[command(flatten)]
    pub runtime: RuntimeArgs,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Args)]
pub struct RuntimeArgs {
    #[arg(long, global = true, value_name = "PATH")]
    pub home_dir: Option<PathBuf>,

    #[arg(long, global = true, value_name = "PATH")]
    pub cwd: Option<PathBuf>,

    #[arg(long, global = true, value_name = "PATH")]
    pub out_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Snapshot(SnapshotArgs),
    Normalize(NormalizeArgs),
    Inspect(InspectArgs),
    Validate(ValidateArgs),
}
