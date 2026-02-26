use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use logit::models::{AgentSource, SCHEMA_VERSION};
use logit::snapshot::samples::RepresentativeSample;
use logit::snapshot::{
    SerializableKeyStats, SnapshotArtifactPointers, SnapshotCollection, SnapshotDiscoveredSource,
    SnapshotIndex, SnapshotIndexCounts, SnapshotSchemaProfile, SnapshotSchemaProfileEntry,
    build_artifact_layout, verify_snapshot_artifacts_parseable,
    verify_snapshot_collection_integrity, write_snapshot_artifacts,
};
use serde_json::json;

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

fn fixture_collection() -> SnapshotCollection {
    SnapshotCollection {
        index: SnapshotIndex {
            schema_version: SCHEMA_VERSION.to_string(),
            sample_size: 2,
            redaction_enabled: true,
            artifacts: SnapshotArtifactPointers {
                index_json: "snapshot/index.json".to_string(),
                samples_jsonl: "snapshot/samples.jsonl".to_string(),
                schema_profile_json: "snapshot/schema_profile.json".to_string(),
            },
            counts: SnapshotIndexCounts {
                discovered_sources: 1,
                existing_sources: 1,
                files_profiled: 1,
                records_profiled: 2,
                samples_emitted: 2,
                warnings: 0,
            },
            sources: vec![SnapshotDiscoveredSource {
                adapter: "codex".to_string(),
                source_kind: "codex".to_string(),
                path: "~/.codex/sessions".to_string(),
                resolved_path: "/tmp/sessions".to_string(),
                format_hint: "directory".to_string(),
                recursive: true,
                exists: true,
                files_profiled: 1,
                records_profiled: 2,
            }],
            warnings: Vec::new(),
        },
        samples: vec![
            RepresentativeSample {
                source_kind: AgentSource::Codex,
                source_path: "/tmp/sessions/rollout.jsonl".to_string(),
                source_record_locator: "line:1".to_string(),
                sample_rank: 0,
                event_kind: Some("prompt".to_string()),
                record: json!({"event_type":"prompt","text":"hello"}),
            },
            RepresentativeSample {
                source_kind: AgentSource::Codex,
                source_path: "/tmp/sessions/rollout.jsonl".to_string(),
                source_record_locator: "line:2".to_string(),
                sample_rank: 1,
                event_kind: Some("response".to_string()),
                record: json!({"event_type":"response","text":"world"}),
            },
        ],
        schema_profile: SnapshotSchemaProfile {
            schema_version: SCHEMA_VERSION.to_string(),
            profiles: vec![SnapshotSchemaProfileEntry {
                adapter: "codex".to_string(),
                source_kind: "codex".to_string(),
                source_path: "~/.codex/sessions".to_string(),
                resolved_path: "/tmp/sessions".to_string(),
                format_hint: "directory".to_string(),
                files_profiled: 1,
                records_profiled: 2,
                event_kind_frequency: BTreeMap::from([
                    ("prompt".to_string(), 1),
                    ("response".to_string(), 1),
                ]),
                key_stats: BTreeMap::from([(
                    "event_type".to_string(),
                    SerializableKeyStats {
                        occurrences: 2,
                        value_types: BTreeMap::from([("string".to_string(), 2)]),
                    },
                )]),
                warnings: Vec::new(),
            }],
        },
    }
}

#[test]
fn collection_integrity_passes_for_consistent_data() {
    let collection = fixture_collection();
    verify_snapshot_collection_integrity(&collection)
        .expect("consistent collection should pass integrity checks");
}

#[test]
fn collection_integrity_fails_for_count_mismatch() {
    let mut collection = fixture_collection();
    collection.index.counts.samples_emitted = 3;

    let error = verify_snapshot_collection_integrity(&collection)
        .expect_err("mismatched sample count should fail integrity checks");
    assert!(error.to_string().contains("samples_emitted"));
}

#[test]
fn collection_integrity_fails_for_non_deterministic_sample_order() {
    let mut collection = fixture_collection();
    collection.samples.swap(0, 1);

    let error = verify_snapshot_collection_integrity(&collection)
        .expect_err("out-of-order samples should fail integrity checks");
    assert!(error.to_string().contains("deterministic order"));
}

#[test]
fn artifact_parseability_check_detects_invalid_samples_jsonl() {
    let out_dir = unique_temp_dir("logit-snapshot-integrity");
    let layout = build_artifact_layout(&out_dir);
    let collection = fixture_collection();
    write_snapshot_artifacts(&layout, &collection).expect("artifact write should succeed");

    std::fs::write(&layout.samples_jsonl, "{invalid-json}\n")
        .expect("tamper should rewrite samples artifact");
    let error = verify_snapshot_artifacts_parseable(&layout)
        .expect_err("invalid samples jsonl should fail parseability check");
    assert!(error.to_string().contains("invalid JSON"));
}
