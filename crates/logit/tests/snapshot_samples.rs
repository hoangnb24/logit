use logit::models::AgentSource;
use logit::snapshot::samples::{SampleCandidate, extract_representative_samples};
use serde_json::json;

fn candidate(
    source_kind: AgentSource,
    source_path: &str,
    locator: &str,
    record: serde_json::Value,
) -> SampleCandidate {
    SampleCandidate {
        source_kind,
        source_path: source_path.to_string(),
        source_record_locator: locator.to_string(),
        record,
    }
}

#[test]
fn extracts_three_samples_with_kind_coverage_first() {
    let candidates = vec![
        candidate(
            AgentSource::Codex,
            "/tmp/codex.jsonl",
            "line:1",
            json!({"event_type":"response","value":1}),
        ),
        candidate(
            AgentSource::Codex,
            "/tmp/codex.jsonl",
            "line:2",
            json!({"event_type":"prompt","value":2}),
        ),
        candidate(
            AgentSource::Codex,
            "/tmp/codex.jsonl",
            "line:3",
            json!({"event_type":"tool_output","value":3}),
        ),
        candidate(
            AgentSource::Codex,
            "/tmp/codex.jsonl",
            "line:4",
            json!({"event_type":"prompt","value":4}),
        ),
    ];

    let samples = extract_representative_samples(&candidates, 3);
    assert_eq!(samples.len(), 3);
    assert_eq!(samples[0].source_record_locator, "line:1");
    assert_eq!(samples[1].source_record_locator, "line:2");
    assert_eq!(samples[2].source_record_locator, "line:3");
}

#[test]
fn extracts_timeline_anchors_when_kind_diversity_is_missing() {
    let candidates = (1..=7)
        .map(|line| {
            candidate(
                AgentSource::Claude,
                "/tmp/claude.jsonl",
                &format!("line:{line}"),
                json!({"payload":{"line":line}}),
            )
        })
        .collect::<Vec<_>>();

    let samples = extract_representative_samples(&candidates, 3);
    assert_eq!(samples.len(), 3);
    assert_eq!(samples[0].source_record_locator, "line:1");
    assert_eq!(samples[1].source_record_locator, "line:4");
    assert_eq!(samples[2].source_record_locator, "line:7");
}

#[test]
fn output_is_deterministic_across_input_ordering() {
    let canonical = vec![
        candidate(
            AgentSource::Amp,
            "/tmp/a.jsonl",
            "line:1",
            json!({"kind":"step"}),
        ),
        candidate(
            AgentSource::Amp,
            "/tmp/a.jsonl",
            "line:2",
            json!({"kind":"step_end"}),
        ),
        candidate(
            AgentSource::Gemini,
            "/tmp/b.jsonl",
            "line:2",
            json!({"type":"message"}),
        ),
        candidate(
            AgentSource::Gemini,
            "/tmp/b.jsonl",
            "line:1",
            json!({"type":"message"}),
        ),
    ];

    let shuffled = vec![
        canonical[2].clone(),
        canonical[0].clone(),
        canonical[3].clone(),
        canonical[1].clone(),
    ];

    let samples_a = extract_representative_samples(&canonical, 2);
    let samples_b = extract_representative_samples(&shuffled, 2);

    assert_eq!(samples_a, samples_b);
    assert_eq!(samples_a.len(), 4);
    assert_eq!(samples_a[0].source_path, "/tmp/a.jsonl");
    assert_eq!(samples_a[2].source_path, "/tmp/b.jsonl");
}

#[test]
fn returns_empty_for_zero_limit_or_empty_input() {
    let one = vec![candidate(
        AgentSource::OpenCode,
        "/tmp/opencode.jsonl",
        "line:1",
        json!({"kind":"progress"}),
    )];

    assert!(extract_representative_samples(&[], 3).is_empty());
    assert!(extract_representative_samples(&one, 0).is_empty());
}
