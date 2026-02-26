#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::Parser;
use clap::error::ErrorKind;
use logit::cli::app::{Cli, Command, RuntimeArgs};
use logit::cli::commands;
use logit::config::RuntimePaths;

const EXIT_SUCCESS: i32 = 0;
const EXIT_RUNTIME_FAILURE: i32 = 1;
const EXIT_VALIDATION_FAILURE: i32 = 2;
const EXIT_USAGE_ERROR: i32 = 64;

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => return exit_code_for_parse_error(error),
    };
    let command_name = command_name(&cli.command);
    println!("logit: starting `{command_name}`");

    match execute(cli) {
        Ok(()) => {
            println!("logit: completed `{command_name}` (exit_code={EXIT_SUCCESS})");
            EXIT_SUCCESS
        }
        Err(error) => {
            let exit_code = classify_runtime_error(&error);
            eprintln!("logit: failed `{command_name}` (exit_code={exit_code})");
            eprintln!("{error:#}");
            exit_code
        }
    }
}

fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Snapshot(args) => {
            let runtime_paths = resolve_runtime_paths(&cli.runtime)?;
            commands::snapshot::run(&args, &runtime_paths)
        }
        Command::Normalize(args) => {
            let runtime_paths = resolve_runtime_paths(&cli.runtime)?;
            commands::normalize::run(&args, &runtime_paths)
        }
        Command::Inspect(args) => commands::inspect::run(&args),
        Command::Validate(args) => {
            let runtime_paths = resolve_runtime_paths(&cli.runtime)?;
            commands::validate::run(&args, &runtime_paths)
        }
    }
}

fn classify_runtime_error(error: &anyhow::Error) -> i32 {
    if error
        .downcast_ref::<commands::validate::ValidationCommandFailure>()
        .is_some()
    {
        EXIT_VALIDATION_FAILURE
    } else {
        EXIT_RUNTIME_FAILURE
    }
}

fn exit_code_for_parse_error(error: clap::Error) -> i32 {
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            let _ = error.print();
            EXIT_SUCCESS
        }
        _ => {
            let _ = error.print();
            EXIT_USAGE_ERROR
        }
    }
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Snapshot(_) => "snapshot",
        Command::Normalize(_) => "normalize",
        Command::Inspect(_) => "inspect",
        Command::Validate(_) => "validate",
    }
}

fn resolve_runtime_paths(args: &RuntimeArgs) -> Result<RuntimePaths> {
    let home_dir = match &args.home_dir {
        Some(path) => path.clone(),
        None => std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("HOME is not set; pass --home-dir"))?,
    };

    let cwd = match &args.cwd {
        Some(path) => path.clone(),
        None => std::env::current_dir()?,
    };

    logit::config::resolve_runtime_paths(&home_dir, &cwd, args.out_dir.as_deref())
}
