use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use logit::adapters::AdapterKind;
use logit::normalize::{default_plan, orchestrate_normalization};

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

#[cfg(unix)]
fn seed_sources_with_unreadable_claude_projects(root: &std::path::Path) -> PathBuf {
    write_file(
        &root.join(".codex/sessions/rollout_primary.jsonl"),
        include_str!("../../../fixtures/codex/rollout_primary.jsonl"),
    );
    let unreadable = root.join(".claude/projects");
    write_file(
        &unreadable.join("project_session.jsonl"),
        include_str!("../../../fixtures/claude/project_session.jsonl"),
    );
    set_mode(&unreadable, 0o000);
    unreadable
}

#[cfg(unix)]
#[test]
fn default_mode_warns_and_continues_when_source_directory_is_unreadable() {
    let source_root = unique_temp_dir("logit-unreadable-default");
    let unreadable = seed_sources_with_unreadable_claude_projects(&source_root);

    let mut plan = default_plan();
    plan.adapters = vec![AdapterKind::Codex, AdapterKind::Claude];
    plan.fail_fast = false;

    let result = orchestrate_normalization(
        &plan,
        std::path::Path::new("/tmp/home"),
        Some(&source_root),
        "",
    )
    .expect("default mode should not fail for unreadable source paths");

    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("source path unreadable"))
    );
    assert!(
        result
            .events
            .iter()
            .any(|event| event.adapter_name == logit::models::AgentSource::Codex)
    );

    set_mode(&unreadable, 0o755);
}

#[cfg(unix)]
#[test]
fn fail_fast_mode_errors_when_source_directory_is_unreadable() {
    let source_root = unique_temp_dir("logit-unreadable-failfast");
    let unreadable = seed_sources_with_unreadable_claude_projects(&source_root);

    let mut plan = default_plan();
    plan.adapters = vec![AdapterKind::Codex, AdapterKind::Claude];
    plan.fail_fast = true;

    let error = orchestrate_normalization(
        &plan,
        std::path::Path::new("/tmp/home"),
        Some(&source_root),
        "",
    )
    .expect_err("fail_fast should fail for unreadable source paths");
    let message = format!("{error:#}");
    assert!(message.contains("failed to collect parseable files"));
    assert!(message.contains(".claude/projects"));

    set_mode(&unreadable, 0o755);
}

#[cfg(not(unix))]
#[test]
fn unreadable_path_policy_tests_are_unix_only() {
    // Permission-bit based unreadable path simulation is Unix-specific.
}
