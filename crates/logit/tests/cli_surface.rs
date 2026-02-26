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

    assert!(
        matches!(&cli.command, Command::Snapshot(_)),
        "expected snapshot command, got {:?}",
        cli.command
    );
    if let Command::Snapshot(args) = cli.command {
        assert_eq!(args.sample_size, 5);
        assert!(args.source_root.is_none());
    }
}

#[test]
fn parses_normalize_fail_fast_flag() {
    let cli = Cli::parse_from(["logit", "normalize", "--fail-fast"]);

    assert!(
        matches!(&cli.command, Command::Normalize(_)),
        "expected normalize command, got {:?}",
        cli.command
    );
    if let Command::Normalize(args) = cli.command {
        assert!(args.fail_fast);
        assert!(args.source_root.is_none());
    }
}

#[test]
fn parses_inspect_json_flag() {
    let cli = Cli::parse_from(["logit", "inspect", "events.jsonl", "--json"]);

    assert!(
        matches!(&cli.command, Command::Inspect(_)),
        "expected inspect command, got {:?}",
        cli.command
    );
    if let Command::Inspect(args) = cli.command {
        assert!(args.json);
        assert_eq!(args.target, Path::new("events.jsonl"));
    }
}

#[test]
fn parses_validate_strict_flag() {
    let cli = Cli::parse_from(["logit", "validate", "out/events.jsonl", "--strict"]);

    assert!(
        matches!(&cli.command, Command::Validate(_)),
        "expected validate command, got {:?}",
        cli.command
    );
    if let Command::Validate(args) = cli.command {
        assert!(args.strict);
        assert_eq!(args.input, Path::new("out/events.jsonl"));
    }
}
