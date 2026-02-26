use std::path::Path;

use clap::Parser;
use logit::cli::app::{Cli, Command};
use logit::cli::commands::ingest::IngestCommand;
use logit::cli::commands::query::QueryCommand;

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

#[test]
fn parses_ingest_refresh_with_global_runtime_flags() {
    let cli = Cli::parse_from([
        "logit",
        "--home-dir",
        "/home/tester",
        "--cwd",
        "/work/repo",
        "--out-dir",
        "/tmp/logit-out",
        "ingest",
        "refresh",
        "--source-root",
        "/work/repo",
        "--fail-fast",
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
        matches!(&cli.command, Command::Ingest(_)),
        "expected ingest command"
    );
    let args = match cli.command {
        Command::Ingest(args) => args,
        _ => return,
    };
    match args.command {
        IngestCommand::Refresh(refresh) => {
            assert_eq!(
                refresh.source_root.as_deref(),
                Some(Path::new("/work/repo"))
            );
            assert!(refresh.fail_fast);
        }
    }
}

#[test]
fn parses_query_namespace_subcommands() {
    let cli = Cli::parse_from([
        "logit",
        "query",
        "sql",
        "select 1",
        "--params",
        "{\"a\":1}",
        "--row-cap",
        "25",
    ]);

    assert!(
        matches!(&cli.command, Command::Query(_)),
        "expected query command"
    );
    let args = match cli.command {
        Command::Query(args) => args,
        _ => return,
    };
    match args.command {
        QueryCommand::Sql(sql) => {
            assert_eq!(sql.sql, "select 1");
            assert_eq!(sql.params.as_deref(), Some("{\"a\":1}"));
            assert_eq!(sql.row_cap, 25);
        }
        other => {
            assert!(
                matches!(other, QueryCommand::Sql(_)),
                "expected query sql command, got {other:?}"
            );
            return;
        }
    }

    let cli = Cli::parse_from(["logit", "query", "schema", "--include-internal"]);
    assert!(
        matches!(&cli.command, Command::Query(_)),
        "expected query command"
    );
    let args = match cli.command {
        Command::Query(args) => args,
        _ => return,
    };
    match args.command {
        QueryCommand::Schema(schema) => assert!(schema.include_internal),
        other => {
            assert!(
                matches!(other, QueryCommand::Schema(_)),
                "expected query schema command, got {other:?}"
            );
            return;
        }
    }

    let cli = Cli::parse_from([
        "logit",
        "query",
        "benchmark",
        "--corpus",
        "/work/repo/fixtures/benchmarks/answerability_question_corpus_v1.json",
        "--row-cap",
        "250",
    ]);
    assert!(
        matches!(&cli.command, Command::Query(_)),
        "expected query command"
    );
    let args = match cli.command {
        Command::Query(args) => args,
        _ => return,
    };
    match args.command {
        QueryCommand::Benchmark(benchmark) => {
            assert_eq!(
                benchmark.corpus.as_deref(),
                Some(Path::new(
                    "/work/repo/fixtures/benchmarks/answerability_question_corpus_v1.json"
                ))
            );
            assert_eq!(benchmark.row_cap, 250);
        }
        other => {
            assert!(
                matches!(other, QueryCommand::Benchmark(_)),
                "expected query benchmark command, got {other:?}"
            );
        }
    }
}
