use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use logit::cli::commands::snapshot::{SnapshotArgs, run as run_snapshot};
use logit::config::RuntimePaths;
use logit::snapshot::build_artifact_layout;
use logit::utils::redaction::REDACTION_TOKEN;
use serde_json::Value;

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

fn write_fixture_source_tree(root: &Path) {
    let codex_sessions = root.join(".codex").join("sessions");
    std::fs::create_dir_all(&codex_sessions).expect("codex sessions dir should be creatable");
    std::fs::write(
        codex_sessions.join("rollout_primary.jsonl"),
        r#"{"event_type":"user_prompt","text":"email=alice@example.com token=sk-secret1234"}
{"event_type":"assistant_response","text":"Bearer abcdefghijklmnop1234"}
"#,
    )
    .expect("codex fixture should be writable");

    let gemini_tmp = root.join(".gemini").join("tmp");
    std::fs::create_dir_all(&gemini_tmp).expect("gemini tmp dir should be creatable");
    std::fs::write(
        gemini_tmp.join("chat_messages.json"),
        r#"{
  "conversation_id": "gemini-c-1",
  "messages": [
    { "role": "user", "type": "message", "content": [{"text":"show diff"}] },
    { "role": "model", "type": "message", "content": [{"text":"2 files changed"}] }
  ]
}
"#,
    )
    .expect("gemini fixture should be writable");
    std::fs::write(
        gemini_tmp.join("conversations.pbmeta.json"),
        include_str!("../../../fixtures/gemini/conversations.pbmeta.json"),
    )
    .expect("gemini protobuf metadata fixture should be writable");
    std::fs::write(
        gemini_tmp.join("conversation_2026-02-03.pb"),
        [0_u8, 159, 146, 150, 0, 42],
    )
    .expect("gemini protobuf binary fixture should be writable");
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions).expect("permissions should be set");
}

#[cfg(unix)]
fn seed_unreadable_claude_projects(root: &Path) -> PathBuf {
    let unreadable = root.join(".claude/projects");
    std::fs::create_dir_all(&unreadable).expect("claude projects dir should be creatable");
    std::fs::write(
        unreadable.join("project_session.jsonl"),
        include_str!("../../../fixtures/claude/project_session.jsonl"),
    )
    .expect("claude fixture should be writable");
    set_mode(&unreadable, 0o000);
    unreadable
}

#[test]
fn snapshot_artifact_layout_uses_contract_filenames() {
    let layout = build_artifact_layout(Path::new("/tmp/logit-out"));
    assert_eq!(
        layout.index_json,
        Path::new("/tmp/logit-out/snapshot/index.json")
    );
    assert_eq!(
        layout.samples_jsonl,
        Path::new("/tmp/logit-out/snapshot/samples.jsonl")
    );
    assert_eq!(
        layout.schema_profile_json,
        Path::new("/tmp/logit-out/snapshot/schema_profile.json")
    );
}

#[test]
fn snapshot_command_emits_index_samples_and_schema_profile() {
    let source_root = unique_temp_dir("logit-snapshot-source");
    let out_dir = unique_temp_dir("logit-snapshot-out");
    write_fixture_source_tree(&source_root);

    let runtime_paths = RuntimePaths {
        home_dir: PathBuf::from("/tmp/logit-home"),
        cwd: PathBuf::from("/tmp/logit-cwd"),
        out_dir: out_dir.clone(),
    };
    let args = SnapshotArgs {
        source_root: Some(source_root.clone()),
        sample_size: 1,
    };

    run_snapshot(&args, &runtime_paths).expect("snapshot command should succeed");

    let layout = build_artifact_layout(&out_dir);
    assert!(layout.index_json.exists(), "snapshot index should exist");
    assert!(
        layout.samples_jsonl.exists(),
        "snapshot samples should exist"
    );
    assert!(
        layout.schema_profile_json.exists(),
        "snapshot schema profile should exist"
    );

    let index: Value = serde_json::from_str(
        &std::fs::read_to_string(&layout.index_json).expect("index should be readable"),
    )
    .expect("index should be valid json");
    assert_eq!(
        index
            .pointer("/artifacts/index_json")
            .and_then(Value::as_str),
        Some("snapshot/index.json")
    );
    assert_eq!(
        index
            .pointer("/artifacts/samples_jsonl")
            .and_then(Value::as_str),
        Some("snapshot/samples.jsonl")
    );
    assert!(
        index
            .pointer("/counts/files_profiled")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 4,
        "expected JSON + protobuf fixture files to be profiled"
    );
    let index_warnings = index
        .get("warnings")
        .and_then(Value::as_array)
        .expect("index warnings array should exist");
    assert!(index_warnings.iter().any(|warning| {
        warning
            .as_str()
            .is_some_and(|text| text.contains("snapshot-only metadata"))
    }));

    let samples_text =
        std::fs::read_to_string(&layout.samples_jsonl).expect("samples should be readable");
    let sample_rows = samples_text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("sample jsonl row should parse"))
        .collect::<Vec<_>>();
    assert!(
        sample_rows.len() >= 2,
        "expected one sample per source file with sample_size=1"
    );
    assert!(sample_rows.iter().any(|row| {
        row.pointer("/record/artifact_kind").and_then(Value::as_str) == Some("protobuf-binary")
    }));

    let rendered_samples = serde_json::to_string(&sample_rows).expect("samples should serialize");
    assert!(rendered_samples.contains(REDACTION_TOKEN));
    assert!(!rendered_samples.contains("alice@example.com"));
    assert!(!rendered_samples.contains("sk-secret1234"));

    let schema_profile: Value = serde_json::from_str(
        &std::fs::read_to_string(&layout.schema_profile_json)
            .expect("schema profile should be readable"),
    )
    .expect("schema profile should be valid json");
    let codex_profile = schema_profile
        .pointer("/profiles")
        .and_then(Value::as_array)
        .and_then(|profiles| {
            profiles.iter().find(|profile| {
                profile.get("source_path").and_then(Value::as_str) == Some("~/.codex/sessions")
            })
        })
        .expect("codex profile entry should exist");

    assert_eq!(
        codex_profile.get("files_profiled").and_then(Value::as_u64),
        Some(1)
    );
    assert!(
        codex_profile.pointer("/key_stats/event_type").is_some(),
        "event_type key stats should be present for codex fixture"
    );

    let gemini_profile = schema_profile
        .pointer("/profiles")
        .and_then(Value::as_array)
        .and_then(|profiles| {
            profiles.iter().find(|profile| {
                profile.get("source_path").and_then(Value::as_str) == Some("~/.gemini/tmp")
            })
        })
        .expect("gemini tmp profile entry should exist");
    assert!(
        gemini_profile
            .get("files_profiled")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 3
    );
    assert!(
        gemini_profile.pointer("/key_stats/artifact_kind").is_some(),
        "protobuf metadata should be represented in gemini profile key stats"
    );
}

#[cfg(unix)]
#[test]
fn snapshot_warns_and_continues_when_source_directory_is_unreadable() {
    let source_root = unique_temp_dir("logit-snapshot-unreadable-source");
    let out_dir = unique_temp_dir("logit-snapshot-unreadable-out");
    write_fixture_source_tree(&source_root);
    let unreadable = seed_unreadable_claude_projects(&source_root);

    let runtime_paths = RuntimePaths {
        home_dir: PathBuf::from("/tmp/logit-home"),
        cwd: PathBuf::from("/tmp/logit-cwd"),
        out_dir: out_dir.clone(),
    };
    let args = SnapshotArgs {
        source_root: Some(source_root),
        sample_size: 1,
    };

    run_snapshot(&args, &runtime_paths)
        .expect("snapshot should continue when a source path is unreadable");

    let layout = build_artifact_layout(&out_dir);
    let index: Value = serde_json::from_str(
        &std::fs::read_to_string(&layout.index_json).expect("index should be readable"),
    )
    .expect("index should be valid json");
    let warnings = index
        .get("warnings")
        .and_then(Value::as_array)
        .expect("index warnings array should exist");
    assert!(warnings.iter().any(|warning| {
        warning.as_str().is_some_and(|text| {
            text.contains("source path unreadable") && text.contains(".claude/projects")
        })
    }));
    assert!(
        index
            .pointer("/counts/files_profiled")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 4,
        "other readable sources should still be profiled"
    );

    set_mode(&unreadable, 0o755);
}

#[cfg(not(unix))]
#[test]
fn snapshot_unreadable_path_policy_tests_are_unix_only() {
    // Permission-bit based unreadable path simulation is Unix-specific.
}
