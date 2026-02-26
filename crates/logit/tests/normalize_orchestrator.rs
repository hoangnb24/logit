use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use logit::adapters::AdapterKind;
use logit::cli::commands::normalize::{NormalizeArgs, run as run_normalize};
use logit::config::RuntimePaths;
use logit::models::AgentSource;
use logit::normalize::{default_plan, orchestrate_normalization};
use serde_json::Value;

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

fn write_file(path: &std::path::Path, content: &str) {
    let parent = path.parent().expect("test file path should have parent");
    std::fs::create_dir_all(parent).expect("test parent directory should be creatable");
    std::fs::write(path, content).expect("test fixture file should be written");
}

#[cfg(unix)]
fn set_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions).expect("permissions should be set");
}

fn seed_codex_and_claude_sources(root: &std::path::Path) {
    write_file(
        &root.join(".codex/sessions/rollout_primary.jsonl"),
        include_str!("../../../fixtures/codex/rollout_primary.jsonl"),
    );
    write_file(
        &root.join(".codex/history.jsonl"),
        include_str!("../../../fixtures/codex/history_auxiliary.jsonl"),
    );
    write_file(
        &root.join(".claude/projects/project_session.jsonl"),
        include_str!("../../../fixtures/claude/project_session.jsonl"),
    );
}

#[test]
fn orchestrator_includes_codex_history_auxiliary_events() {
    let source_root = unique_temp_dir("logit-orchestrator-codex-history");
    seed_codex_and_claude_sources(&source_root);

    let mut plan = default_plan();
    plan.adapters = vec![AdapterKind::Codex];

    let result = orchestrate_normalization(
        &plan,
        std::path::Path::new("/tmp/home"),
        Some(&source_root),
        "",
    )
    .expect("orchestrator should succeed");

    assert!(
        result
            .events
            .iter()
            .any(|event| event.tags.iter().any(|tag| tag == "history_auxiliary"))
    );
    assert!(
        result
            .events
            .iter()
            .any(|event| event.source_path.ends_with("/.codex/history.jsonl"))
    );
}

#[test]
fn orchestrator_dedupes_exact_history_rollout_message_duplicates() {
    let source_root = unique_temp_dir("logit-orchestrator-codex-history-dedupe");
    write_file(
        &source_root.join(".codex/sessions/rollout_primary.jsonl"),
        r#"{"session_id":"codex-s-dup","event_id":"evt-001","event_type":"user_prompt","created_at":"2026-02-01T12:00:00Z","text":"Please summarize the last run."}"#,
    );
    write_file(
        &source_root.join(".codex/history.jsonl"),
        r#"{"source":"codex_history","session_id":"codex-s-dup","prompt_id":"p-001","created_at":"2026-02-01T12:00:00Z","role":"user","content":"Please summarize the last run."}"#,
    );

    let mut plan = default_plan();
    plan.adapters = vec![AdapterKind::Codex];

    let result = orchestrate_normalization(
        &plan,
        std::path::Path::new("/tmp/home"),
        Some(&source_root),
        "",
    )
    .expect("orchestrator should succeed");

    assert_eq!(result.dedupe_stats.input_records, 2);
    assert_eq!(result.dedupe_stats.unique_records, 1);
    assert_eq!(result.dedupe_stats.duplicate_records, 1);
    assert_eq!(result.events.len(), 1);
}

#[test]
fn orchestrator_fans_in_codex_and_claude_sources() {
    let source_root = unique_temp_dir("logit-orchestrator-sources");
    seed_codex_and_claude_sources(&source_root);

    let mut plan = default_plan();
    plan.adapters = vec![AdapterKind::Codex, AdapterKind::Claude];

    let result = orchestrate_normalization(
        &plan,
        std::path::Path::new("/tmp/home"),
        Some(&source_root),
        "",
    )
    .expect("orchestrator should succeed");

    assert!(!result.events.is_empty());
    assert_eq!(result.dedupe_stats.unique_records, result.events.len());
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
}

#[test]
fn orchestrator_surfaces_non_fatal_warnings_for_unsupported_adapters() {
    let source_root = unique_temp_dir("logit-orchestrator-unsupported");
    write_file(
        &source_root.join(".opencode/sessions/chat.jsonl"),
        r#"{"kind":"message","text":"hello"}"#,
    );

    let mut plan = default_plan();
    plan.adapters = vec![AdapterKind::OpenCode];

    let result = orchestrate_normalization(
        &plan,
        std::path::Path::new("/tmp/home"),
        Some(&source_root),
        "",
    )
    .expect("unsupported adapters should be non-fatal in default mode");
    assert!(result.events.is_empty());
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("not yet supported"))
    );
    let opencode_health = result
        .adapter_health
        .get("opencode")
        .expect("health report should include opencode");
    assert_eq!(opencode_health.status.as_str(), "skipped");
    assert_eq!(
        opencode_health.reason.as_deref(),
        Some("adapter_not_supported_by_normalize_v1")
    );
    assert!(
        opencode_health
            .warnings
            .iter()
            .any(|warning| warning.contains("not yet supported"))
    );
}

#[cfg(unix)]
#[test]
fn orchestrator_marks_partial_failure_when_adapter_emits_events_and_errors() {
    let source_root = unique_temp_dir("logit-orchestrator-partial-failure");
    write_file(
        &source_root.join(".codex/sessions/rollout_primary.jsonl"),
        include_str!("../../../fixtures/codex/rollout_primary.jsonl"),
    );
    std::fs::create_dir_all(source_root.join(".codex/history.jsonl"))
        .expect("history path directory should be creatable");
    set_mode(&source_root.join(".codex/history.jsonl"), 0o000);

    let mut plan = default_plan();
    plan.adapters = vec![AdapterKind::Codex];
    plan.fail_fast = false;

    let result = orchestrate_normalization(
        &plan,
        std::path::Path::new("/tmp/home"),
        Some(&source_root),
        "",
    )
    .expect("default mode should continue when one adapter source path fails");

    assert!(
        result
            .events
            .iter()
            .any(|event| event.adapter_name == AgentSource::Codex)
    );
    let codex_health = result
        .adapter_health
        .get("codex")
        .expect("health report should include codex");
    assert_eq!(codex_health.status.as_str(), "partial_failure");
    assert_eq!(
        codex_health.reason.as_deref(),
        Some("adapter_emitted_partial_results")
    );
    assert!(codex_health.events_emitted > 0);
    assert!(!codex_health.errors.is_empty());
    assert!(
        codex_health
            .errors
            .iter()
            .any(|error| error.contains("source path unreadable"))
    );

    set_mode(&source_root.join(".codex/history.jsonl"), 0o755);
}

#[test]
fn normalize_command_emits_artifacts_from_orchestrated_sources() {
    let source_root = unique_temp_dir("logit-normalize-run-sources");
    seed_codex_and_claude_sources(&source_root);
    let out_dir = unique_temp_dir("logit-normalize-run-artifacts");

    let runtime_paths = RuntimePaths {
        home_dir: PathBuf::from("/tmp/logit-home"),
        cwd: PathBuf::from("/tmp/logit-cwd"),
        out_dir: out_dir.clone(),
    };
    let args = NormalizeArgs {
        source_root: Some(source_root.clone()),
        fail_fast: false,
    };

    run_normalize(&args, &runtime_paths).expect("normalize run should succeed");

    let events_path = out_dir.join("events.jsonl");
    let events_content =
        std::fs::read_to_string(&events_path).expect("events artifact should exist");
    let rows = events_content
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("each event row should parse"))
        .collect::<Vec<_>>();
    assert!(!rows.is_empty());
    assert!(
        rows.iter()
            .any(|row| row.get("adapter_name").and_then(Value::as_str) == Some("codex"))
    );
    assert!(
        rows.iter()
            .any(|row| row.get("adapter_name").and_then(Value::as_str) == Some("claude"))
    );

    let stats_path = out_dir.join("stats.json");
    let stats: Value = serde_json::from_str(
        &std::fs::read_to_string(&stats_path).expect("stats artifact should exist"),
    )
    .expect("stats artifact should parse");
    assert!(
        stats
            .pointer("/counts/records_emitted")
            .and_then(Value::as_u64)
            .expect("records_emitted should exist")
            > 0
    );
}
