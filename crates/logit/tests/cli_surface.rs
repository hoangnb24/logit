use std::path::Path;

use clap::Parser;
use logit::cli::app::{Cli, Command};

#[test]
fn parses_global_runtime_flags_for_snapshot() {
    let cli = Cli::parse_from([
        "logit",
        "--home-dir",
        "/home/tester",
        "--cwd",
        "/work/repo",
        "--out-dir",
        "/tmp/logit-out",
        "snapshot",
        "--sample-size",
        "5",
    ]);

    assert_eq!(
        cli.runtime.home_dir.as_deref(),
        Some(Path::new("/home/tester"))
    );
    assert_eq!(cli.runtime.cwd.as_deref(), Some(Path::new("/work/repo")));
    assert_eq!(
        cli.runtime.out_dir.as_deref(),
        Some(Path::new("/tmp/logit-out"))
    );

    match cli.command {
        Command::Snapshot(args) => {
            assert_eq!(args.sample_size, 5);
            assert!(args.source_root.is_none());
        }
        other => panic!("expected snapshot command, got {other:?}"),
    }
}

#[test]
fn parses_normalize_fail_fast_flag() {
    let cli = Cli::parse_from(["logit", "normalize", "--fail-fast"]);

    match cli.command {
        Command::Normalize(args) => {
            assert!(args.fail_fast);
            assert!(args.source_root.is_none());
        }
        other => panic!("expected normalize command, got {other:?}"),
    }
}

#[test]
fn parses_inspect_json_flag() {
    let cli = Cli::parse_from(["logit", "inspect", "events.jsonl", "--json"]);

    match cli.command {
        Command::Inspect(args) => {
            assert!(args.json);
            assert_eq!(args.target, Path::new("events.jsonl"));
        }
        other => panic!("expected inspect command, got {other:?}"),
    }
}

#[test]
fn parses_validate_strict_flag() {
    let cli = Cli::parse_from(["logit", "validate", "out/events.jsonl", "--strict"]);

    match cli.command {
        Command::Validate(args) => {
            assert!(args.strict);
            assert_eq!(args.input, Path::new("out/events.jsonl"));
        }
        other => panic!("expected validate command, got {other:?}"),
    }
}
