use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use logit::cli::commands::normalize::{NormalizeArgs, run as run_normalize};
use logit::config::RuntimePaths;
use logit::discovery::build_artifact_layout as build_discovery_artifact_layout;
use logit::models::AgentSource;
use logit::normalize::{
    build_artifact_layout as build_normalize_artifact_layout, default_plan,
    orchestrate_normalization,
};
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

fn seed_local_layout(source_root: &Path, home_dir: &Path) -> String {
    write_file(
        &source_root.join(".codex/sessions/rollout_primary.jsonl"),
        include_str!("../../../fixtures/codex/rollout_primary.jsonl"),
    );
    write_file(
        &source_root.join(".codex/history.jsonl"),
        include_str!("../../../fixtures/codex/history_auxiliary.jsonl"),
    );
    write_file(
        &source_root.join(".claude/projects/project_session.jsonl"),
        include_str!("../../../fixtures/claude/project_session.jsonl"),
    );
    write_file(
        &source_root.join(".gemini/history/chat_session.jsonl"),
        r#"{"kind":"message","role":"user","content":"gemini smoke row"}"#,
    );
    write_file(
        &source_root.join(".amp/history/thread.jsonl"),
        r#"{"type":"message","content":"amp smoke row"}"#,
    );
    write_file(
        &source_root.join(".opencode/sessions/session.jsonl"),
        r#"{"id":"op-msg-1","type":"message","content":"opencode smoke row"}"#,
    );

    let zsh_history = r#": 1740467001:0;codex --full-auto
: 1740467002:0;claude --resume
: 1740467003:0;gemini -p "hello"
: 1740467004:0;amp run
: 1740467005:0;opencode run
"#;
    write_file(&home_dir.join(".zsh_history"), zsh_history);
    zsh_history.to_string()
}

#[test]
fn orchestrator_smoke_handles_local_layout_and_adapter_coverage() {
    let source_root = unique_temp_dir("logit-local-smoke-orchestrator-source");
    let home_dir = unique_temp_dir("logit-local-smoke-orchestrator-home");
    let zsh_history = seed_local_layout(&source_root, &home_dir);

    let plan = default_plan();
    let result = orchestrate_normalization(&plan, &home_dir, Some(&source_root), &zsh_history)
        .expect("orchestrator should succeed in local-layout smoke run");

    assert!(
        result
            .events
            .iter()
            .any(|event| event.adapter_name == AgentSource::Codex)
    );
    assert!(
        result
            .events
            .iter()
            .any(|event| event.adapter_name == AgentSource::Claude)
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("adapter `gemini` not yet supported"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("adapter `amp` not yet supported"))
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("adapter `opencode` not yet supported"))
    );

    let discovered_adapters = result
        .prioritized_sources
        .iter()
        .map(|source| source.adapter.as_str().to_string())
        .collect::<BTreeSet<_>>();
    assert!(discovered_adapters.contains("codex"));
    assert!(discovered_adapters.contains("claude"));
    assert!(discovered_adapters.contains("gemini"));
    assert!(discovered_adapters.contains("amp"));
    assert!(discovered_adapters.contains("opencode"));
}

#[test]
fn normalize_command_smoke_writes_artifacts_for_local_layout() {
    let source_root = unique_temp_dir("logit-local-smoke-normalize-source");
    let home_dir = unique_temp_dir("logit-local-smoke-normalize-home");
    let out_dir = unique_temp_dir("logit-local-smoke-normalize-out");
    seed_local_layout(&source_root, &home_dir);

    let runtime_paths = RuntimePaths {
        home_dir: home_dir.clone(),
        cwd: PathBuf::from("/tmp/logit-cwd"),
        out_dir: out_dir.clone(),
    };
    let args = NormalizeArgs {
        source_root: Some(source_root),
        fail_fast: false,
    };

    run_normalize(&args, &runtime_paths)
        .expect("normalize should succeed in local-layout smoke run");

    let normalize_layout = build_normalize_artifact_layout(&out_dir);
    let discovery_layout = build_discovery_artifact_layout(&out_dir);
    assert!(normalize_layout.events_jsonl.exists());
    assert!(normalize_layout.stats_json.exists());
    assert!(normalize_layout.schema_json.exists());
    assert!(discovery_layout.sources_json.exists());
    assert!(discovery_layout.zsh_history_usage_json.exists());

    let event_rows = std::fs::read_to_string(&normalize_layout.events_jsonl)
        .expect("events artifact should be readable")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("event row should parse"))
        .collect::<Vec<_>>();
    assert!(!event_rows.is_empty());
    assert!(
        event_rows
            .iter()
            .any(|row| row.get("adapter_name").and_then(Value::as_str) == Some("codex"))
    );
    assert!(
        event_rows
            .iter()
            .any(|row| row.get("adapter_name").and_then(Value::as_str) == Some("claude"))
    );

    let stats: Value = serde_json::from_str(
        &std::fs::read_to_string(&normalize_layout.stats_json)
            .expect("stats artifact should be readable"),
    )
    .expect("stats artifact should parse");
    assert_eq!(
        stats
            .pointer("/counts/records_emitted")
            .and_then(Value::as_u64),
        Some(event_rows.len() as u64)
    );

    let discovery_sources: Value = serde_json::from_str(
        &std::fs::read_to_string(&discovery_layout.sources_json)
            .expect("discovery sources artifact should be readable"),
    )
    .expect("discovery sources artifact should parse");
    let adapter_counts = discovery_sources
        .get("adapter_counts")
        .and_then(Value::as_object)
        .expect("adapter_counts should be present");
    assert!(adapter_counts.contains_key("codex"));
    assert!(adapter_counts.contains_key("claude"));
    assert!(adapter_counts.contains_key("gemini"));
    assert!(adapter_counts.contains_key("amp"));
    assert!(adapter_counts.contains_key("opencode"));

    let history_usage: Value = serde_json::from_str(
        &std::fs::read_to_string(&discovery_layout.zsh_history_usage_json)
            .expect("history usage artifact should be readable"),
    )
    .expect("history usage artifact should parse");
    assert_eq!(
        history_usage
            .get("total_command_hits")
            .and_then(Value::as_u64),
        Some(5)
    );
}
