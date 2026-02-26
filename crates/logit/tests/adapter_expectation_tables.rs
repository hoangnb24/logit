use logit::adapters::{amp, claude, codex, gemini, opencode};
use logit::models::{ActorRole, AgentLogEvent, EventType, RecordFormat};

struct CanonicalFixtureExpectation {
    name: &'static str,
    events: usize,
    warnings: usize,
    first_record_format: RecordFormat,
    first_event_type: EventType,
    first_role: ActorRole,
}

fn parse_canonical_fixture(name: &str, run_id: &str) -> Vec<AgentLogEvent> {
    match name {
        "codex_rollout_primary" => {
            codex::parse_rollout_jsonl(
                include_str!("../../../fixtures/codex/rollout_primary.jsonl"),
                run_id,
                "fixtures/codex/rollout_primary.jsonl",
            )
            .events
        }
        "claude_project_session" => {
            claude::parse_project_session_jsonl(
                include_str!("../../../fixtures/claude/project_session.jsonl"),
                run_id,
                "fixtures/claude/project_session.jsonl",
            )
            .events
        }
        "gemini_chat_messages" => {
            gemini::parse_chat_session_json(
                include_str!("../../../fixtures/gemini/chat_messages.json"),
                run_id,
                "fixtures/gemini/chat_messages.json",
            )
            .expect("gemini fixture should parse")
            .events
        }
        "opencode_runtime_prompt_history" => {
            opencode::parse_auxiliary_log_text(
                include_str!("../../../fixtures/opencode/runtime_prompt_history.log"),
                run_id,
                "fixtures/opencode/runtime_prompt_history.log",
            )
            .events
        }
        _ => panic!("unknown canonical fixture: {name}"),
    }
}

fn parse_canonical_fixture_warnings(name: &str, run_id: &str) -> Vec<String> {
    match name {
        "codex_rollout_primary" => {
            codex::parse_rollout_jsonl(
                include_str!("../../../fixtures/codex/rollout_primary.jsonl"),
                run_id,
                "fixtures/codex/rollout_primary.jsonl",
            )
            .warnings
        }
        "claude_project_session" => {
            claude::parse_project_session_jsonl(
                include_str!("../../../fixtures/claude/project_session.jsonl"),
                run_id,
                "fixtures/claude/project_session.jsonl",
            )
            .warnings
        }
        "gemini_chat_messages" => {
            gemini::parse_chat_session_json(
                include_str!("../../../fixtures/gemini/chat_messages.json"),
                run_id,
                "fixtures/gemini/chat_messages.json",
            )
            .expect("gemini fixture should parse")
            .warnings
        }
        "opencode_runtime_prompt_history" => {
            opencode::parse_auxiliary_log_text(
                include_str!("../../../fixtures/opencode/runtime_prompt_history.log"),
                run_id,
                "fixtures/opencode/runtime_prompt_history.log",
            )
            .warnings
        }
        _ => panic!("unknown canonical fixture: {name}"),
    }
}

#[test]
fn fixture_corpus_canonical_expectation_table_is_stable() {
    let run_id = "expectation-table-run";
    let table = [
        CanonicalFixtureExpectation {
            name: "codex_rollout_primary",
            events: 3,
            warnings: 0,
            first_record_format: RecordFormat::Message,
            first_event_type: EventType::Prompt,
            first_role: ActorRole::User,
        },
        CanonicalFixtureExpectation {
            name: "claude_project_session",
            events: 3,
            warnings: 0,
            first_record_format: RecordFormat::Message,
            first_event_type: EventType::Prompt,
            first_role: ActorRole::User,
        },
        CanonicalFixtureExpectation {
            name: "gemini_chat_messages",
            events: 2,
            warnings: 0,
            first_record_format: RecordFormat::Message,
            first_event_type: EventType::Prompt,
            first_role: ActorRole::User,
        },
        CanonicalFixtureExpectation {
            name: "opencode_runtime_prompt_history",
            events: 2,
            warnings: 0,
            first_record_format: RecordFormat::Message,
            first_event_type: EventType::Prompt,
            first_role: ActorRole::User,
        },
    ];

    for expectation in table {
        let events = parse_canonical_fixture(expectation.name, run_id);
        let warnings = parse_canonical_fixture_warnings(expectation.name, run_id);

        assert_eq!(
            events.len(),
            expectation.events,
            "unexpected event count for {}",
            expectation.name
        );
        assert_eq!(
            warnings.len(),
            expectation.warnings,
            "unexpected warning count for {}",
            expectation.name
        );

        let first = events
            .first()
            .unwrap_or_else(|| panic!("missing first event for {}", expectation.name));
        assert_eq!(first.record_format, expectation.first_record_format);
        assert_eq!(first.event_type, expectation.first_event_type);
        assert_eq!(first.role, expectation.first_role);
    }
}

#[test]
fn fixture_corpus_metadata_expectation_table_is_stable() {
    let amp_thread =
        amp::parse_thread_envelope(include_str!("../../../fixtures/amp/thread_payloads.json"))
            .expect("amp fixture should parse");
    assert_eq!(amp_thread.thread.thread_id, "amp-t-001");
    assert_eq!(amp_thread.messages.len(), 2);
    assert_eq!(amp_thread.warnings.len(), 0);
    assert_eq!(
        amp_thread.messages[0].content_text.as_deref(),
        Some("Summarize this file.")
    );

    let opencode_session = opencode::parse_session_metadata_jsonl(include_str!(
        "../../../fixtures/opencode/session_messages.jsonl"
    ))
    .expect("opencode session fixture should parse");
    assert_eq!(opencode_session.messages.len(), 2);
    assert_eq!(opencode_session.sessions.len(), 1);
    assert_eq!(opencode_session.warnings.len(), 0);
    assert_eq!(opencode_session.sessions[0].session_id, "oc-s-001");
}
