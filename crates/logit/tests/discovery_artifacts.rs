use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use logit::cli::commands::normalize::{NormalizeArgs, run as run_normalize};
use logit::config::RuntimePaths;
use logit::discovery::build_artifact_layout;
use logit::normalize::build_artifact_layout as build_normalize_artifact_layout;
use serde_json::Value;

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

fn write_file(path: &Path, content: &str) {
    let parent = path.parent().expect("test path should have parent");
    std::fs::create_dir_all(parent).expect("test parent directory should be creatable");
    std::fs::write(path, content).expect("test file should be writable");
}

fn seed_codex_and_claude_sources(root: &Path) {
    write_file(
        &root.join(".codex/sessions/rollout_primary.jsonl"),
        include_str!("../../../fixtures/codex/rollout_primary.jsonl"),
    );
    write_file(
        &root.join(".claude/projects/project_session.jsonl"),
        include_str!("../../../fixtures/claude/project_session.jsonl"),
    );
}

fn seed_realistic_local_environment(home_dir: &Path) {
    seed_codex_and_claude_sources(home_dir);
    write_file(
        &home_dir.join(".claude/statsig/cache.log"),
        "statsig cache checkpoint\n",
    );
    write_file(&home_dir.join(".claude.json"), r#"{"recentProjects":[]}"#);

    write_file(
        &home_dir.join(".gemini/tmp/run-001/logs.json"),
        r#"{"status":"ok"}"#,
    );
    write_file(
        &home_dir.join(".gemini/history/chat.jsonl"),
        r#"{"kind":"message","text":"hello from gemini history"}"#,
    );
    write_file(
        &home_dir.join(".gemini/debug/runtime.log"),
        "gemini debug diagnostics\n",
    );

    write_file(
        &home_dir.join(".amp/sessions/thread.json"),
        r#"{"session":"amp-001"}"#,
    );
    write_file(
        &home_dir.join(".amp/history/history.jsonl"),
        r#"{"kind":"history","text":"amp history row"}"#,
    );
    write_file(
        &home_dir.join(".amp/logs/runtime.log"),
        "amp runtime diagnostics\n",
    );

    write_file(
        &home_dir.join(".opencode/project/messages.jsonl"),
        r#"{"id":"msg-1","text":"project message"}"#,
    );
    write_file(
        &home_dir.join(".opencode/sessions/parts.jsonl"),
        r#"{"id":"part-1","kind":"text"}"#,
    );
    write_file(
        &home_dir.join(".opencode/logs/runtime.log"),
        "opencode runtime diagnostics\n",
    );

    write_file(
        &home_dir.join(".zsh_history"),
        r#": 1740467001:0;codex --full-auto
: 1740467002:0;codex --continue
: 1740467003:0;claude --resume
: 1740467004:0;gemini --prompt "summarize"
: 1740467005:0;cat ~/.amp/sessions/thread.json
: 1740467006:0;opencode --project /tmp/demo
"#,
    );
}

#[test]
fn discovery_artifact_layout_uses_contract_filenames() {
    let layout = build_artifact_layout(Path::new("/tmp/logit-out"));
    assert_eq!(
        layout.sources_json,
        Path::new("/tmp/logit-out/discovery/sources.json")
    );
    assert_eq!(
        layout.zsh_history_usage_json,
        Path::new("/tmp/logit-out/discovery/zsh_history_usage.json")
    );
}

#[test]
fn normalize_command_emits_discovery_evidence_artifacts() {
    let source_root = unique_temp_dir("logit-discovery-sources");
    let home_dir = unique_temp_dir("logit-discovery-home");
    let out_dir = unique_temp_dir("logit-discovery-out");
    seed_codex_and_claude_sources(&source_root);
    write_file(
        &home_dir.join(".zsh_history"),
        r#": 1740467001:0;claude --resume
: 1740467002:0;claude --resume
: 1740467003:0;codex --full-auto
"#,
    );

    let runtime_paths = RuntimePaths {
        home_dir: home_dir.clone(),
        cwd: PathBuf::from("/tmp/logit-cwd"),
        out_dir: out_dir.clone(),
    };
    let args = NormalizeArgs {
        source_root: Some(source_root),
        fail_fast: false,
    };

    run_normalize(&args, &runtime_paths).expect("normalize command should succeed");

    let layout = build_artifact_layout(&out_dir);
    assert!(layout.sources_json.exists(), "sources.json should exist");
    assert!(
        layout.zsh_history_usage_json.exists(),
        "zsh_history_usage.json should exist"
    );

    let sources: Value = serde_json::from_str(
        &std::fs::read_to_string(&layout.sources_json)
            .expect("sources artifact should be readable"),
    )
    .expect("sources artifact should be valid json");
    assert!(
        sources
            .get("total_sources")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
    );
    let source_rows = sources
        .get("sources")
        .and_then(Value::as_array)
        .expect("sources array should exist");
    assert!(source_rows.iter().any(|row| {
        row.get("adapter").and_then(Value::as_str) == Some("claude")
            && row.get("path").and_then(Value::as_str) == Some("~/.claude/projects")
    }));
    assert!(source_rows.iter().any(|row| {
        row.get("adapter").and_then(Value::as_str) == Some("codex")
            && row.get("path").and_then(Value::as_str) == Some("~/.codex/sessions")
    }));

    let history_usage: Value = serde_json::from_str(
        &std::fs::read_to_string(&layout.zsh_history_usage_json)
            .expect("history usage artifact should be readable"),
    )
    .expect("history usage artifact should be valid json");
    assert_eq!(
        history_usage
            .get("total_command_hits")
            .and_then(Value::as_u64),
        Some(3)
    );
    let usage_rows = history_usage
        .get("adapter_usage")
        .and_then(Value::as_array)
        .expect("adapter usage array should exist");
    assert!(usage_rows.iter().any(|row| {
        row.get("adapter").and_then(Value::as_str) == Some("claude")
            && row.get("score").and_then(Value::as_u64) == Some(2)
    }));
    assert!(usage_rows.iter().any(|row| {
        row.get("adapter").and_then(Value::as_str) == Some("codex")
            && row.get("score").and_then(Value::as_u64) == Some(1)
    }));
}

#[test]
fn normalize_smoke_handles_realistic_local_home_and_adapter_coverage() {
    let home_dir = unique_temp_dir("logit-discovery-smoke-home");
    let out_dir = unique_temp_dir("logit-discovery-smoke-out");
    seed_realistic_local_environment(&home_dir);

    let runtime_paths = RuntimePaths {
        home_dir: home_dir.clone(),
        cwd: PathBuf::from("/tmp/logit-cwd"),
        out_dir: out_dir.clone(),
    };
    let args = NormalizeArgs {
        source_root: None,
        fail_fast: false,
    };

    run_normalize(&args, &runtime_paths).expect("normalize smoke run should succeed");

    let discovery_layout = build_artifact_layout(&out_dir);
    let sources: Value = serde_json::from_str(
        &std::fs::read_to_string(&discovery_layout.sources_json)
            .expect("sources artifact should be readable"),
    )
    .expect("sources artifact should parse");
    assert_eq!(
        sources.get("total_sources").and_then(Value::as_u64),
        Some(15)
    );
    let adapter_counts = sources
        .get("adapter_counts")
        .and_then(Value::as_object)
        .expect("adapter_counts object should exist");
    for adapter in ["codex", "claude", "gemini", "amp", "opencode"] {
        assert_eq!(adapter_counts.get(adapter).and_then(Value::as_u64), Some(3));
    }

    let history_usage: Value = serde_json::from_str(
        &std::fs::read_to_string(&discovery_layout.zsh_history_usage_json)
            .expect("history usage artifact should be readable"),
    )
    .expect("history usage artifact should parse");
    assert_eq!(
        history_usage
            .get("total_command_hits")
            .and_then(Value::as_u64),
        Some(6)
    );
    let usage_rows = history_usage
        .get("adapter_usage")
        .and_then(Value::as_array)
        .expect("adapter usage rows should exist");
    assert!(usage_rows.iter().any(|row| {
        row.get("adapter").and_then(Value::as_str) == Some("codex")
            && row.get("score").and_then(Value::as_u64) == Some(2)
    }));
    assert!(usage_rows.iter().any(|row| {
        row.get("adapter").and_then(Value::as_str) == Some("claude")
            && row.get("score").and_then(Value::as_u64) == Some(1)
    }));
    assert!(usage_rows.iter().any(|row| {
        row.get("adapter").and_then(Value::as_str) == Some("gemini")
            && row.get("score").and_then(Value::as_u64) == Some(1)
    }));
    assert!(usage_rows.iter().any(|row| {
        row.get("adapter").and_then(Value::as_str) == Some("amp")
            && row.get("score").and_then(Value::as_u64) == Some(1)
    }));
    assert!(usage_rows.iter().any(|row| {
        row.get("adapter").and_then(Value::as_str) == Some("opencode")
            && row.get("score").and_then(Value::as_u64) == Some(1)
    }));

    let normalize_layout = build_normalize_artifact_layout(&out_dir);
    let rows = std::fs::read_to_string(&normalize_layout.events_jsonl)
        .expect("events artifact should be readable")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("event row should parse"))
        .collect::<Vec<_>>();
    assert!(
        !rows.is_empty(),
        "expected normalized events for supported adapters"
    );
    assert!(
        rows.iter()
            .any(|row| row.get("adapter_name").and_then(Value::as_str) == Some("codex"))
    );
    assert!(
        rows.iter()
            .any(|row| row.get("adapter_name").and_then(Value::as_str) == Some("claude"))
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.get("adapter_name").and_then(Value::as_str) == Some("gemini"))
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.get("adapter_name").and_then(Value::as_str) == Some("amp"))
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.get("adapter_name").and_then(Value::as_str) == Some("opencode"))
    );
}
