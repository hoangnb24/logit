use logit::adapters::AdapterKind;
use logit::discovery::zsh_history_scores;
use logit::utils::history::{parse_zsh_history, score_adapter_command_frequency};

#[test]
fn preserves_command_when_extended_metadata_is_malformed() {
    let parsed = parse_zsh_history(": not-a-ts:not-a-duration;gemini -p \"summarize\"");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].timestamp_unix, None);
    assert_eq!(parsed[0].duration_seconds, None);
    assert_eq!(parsed[0].command, "gemini -p \"summarize\"");
}

#[test]
fn scoring_is_deterministic_across_history_ordering() {
    let history_a = r#"
: 1740467001:0;codex --continue
: 1740467002:1;claude --resume
: 1740467003:0;codex --full-auto
"#;
    let history_b = r#"
: 1740467003:0;codex --full-auto
: 1740467001:0;codex --continue
: 1740467002:1;claude --resume
"#;

    let scores_a = score_adapter_command_frequency(&parse_zsh_history(history_a));
    let scores_b = score_adapter_command_frequency(&parse_zsh_history(history_b));
    assert_eq!(scores_a, scores_b);
}

#[test]
fn discovery_scores_include_every_adapter_with_stable_zeroes() {
    let history = r#"
echo \"hello\"
cat ~/.amp/sessions/thread.json
opencode --project /tmp/demo
"#;

    let scores = zsh_history_scores(history);
    assert_eq!(scores.len(), 5);

    let amp = scores
        .iter()
        .find(|score| score.adapter == AdapterKind::Amp)
        .expect("amp score should exist");
    assert_eq!(amp.score, 1);

    let opencode = scores
        .iter()
        .find(|score| score.adapter == AdapterKind::OpenCode)
        .expect("opencode score should exist");
    assert_eq!(opencode.score, 1);

    let codex = scores
        .iter()
        .find(|score| score.adapter == AdapterKind::Codex)
        .expect("codex score should exist");
    assert_eq!(codex.score, 0);
}
