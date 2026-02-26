use logit::adapters::{amp, claude, codex, gemini, opencode};
use logit::models::{ActorRole, AgentLogEvent, AgentSource, EventType, RecordFormat};

fn assert_core_event_contract(event: &AgentLogEvent, source: AgentSource, run_id: &str) {
    assert_eq!(event.run_id, run_id);
    assert_eq!(event.source_kind, source);
    assert_eq!(event.adapter_name, source);
    assert!(!event.event_id.trim().is_empty());
    assert!(!event.source_record_locator.trim().is_empty());
    assert!(!event.timestamp_utc.trim().is_empty());
    assert!(!event.raw_hash.trim().is_empty());
    assert!(!event.canonical_hash.trim().is_empty());
}

#[derive(Debug, Clone, Copy)]
struct CanonicalFixtureExpectation {
    fixture_path: &'static str,
    source: AgentSource,
    expected_event_count: usize,
    expected_first_record_format: RecordFormat,
    expected_first_event_type: EventType,
    expected_first_role: ActorRole,
    expected_first_session_id: Option<&'static str>,
    expected_first_conversation_id: Option<&'static str>,
}

const CANONICAL_EXPECTATIONS: &[CanonicalFixtureExpectation] = &[
    CanonicalFixtureExpectation {
        fixture_path: "fixtures/codex/rollout_primary.jsonl",
        source: AgentSource::Codex,
        expected_event_count: 3,
        expected_first_record_format: RecordFormat::Message,
        expected_first_event_type: EventType::Prompt,
        expected_first_role: ActorRole::User,
        expected_first_session_id: Some("codex-s-001"),
        expected_first_conversation_id: None,
    },
    CanonicalFixtureExpectation {
        fixture_path: "fixtures/claude/project_session.jsonl",
        source: AgentSource::Claude,
        expected_event_count: 3,
        expected_first_record_format: RecordFormat::Message,
        expected_first_event_type: EventType::Prompt,
        expected_first_role: ActorRole::User,
        expected_first_session_id: Some("claude-s-001"),
        expected_first_conversation_id: Some("claude-p-001"),
    },
    CanonicalFixtureExpectation {
        fixture_path: "fixtures/gemini/chat_messages.json",
        source: AgentSource::Gemini,
        expected_event_count: 2,
        expected_first_record_format: RecordFormat::Message,
        expected_first_event_type: EventType::Prompt,
        expected_first_role: ActorRole::User,
        expected_first_session_id: None,
        expected_first_conversation_id: Some("gemini-c-001"),
    },
    CanonicalFixtureExpectation {
        fixture_path: "fixtures/opencode/runtime_prompt_history.log",
        source: AgentSource::OpenCode,
        expected_event_count: 2,
        expected_first_record_format: RecordFormat::Message,
        expected_first_event_type: EventType::Prompt,
        expected_first_role: ActorRole::User,
        expected_first_session_id: None,
        expected_first_conversation_id: None,
    },
];

#[test]
fn adapter_mapping_matrix_matches_fixture_expectation_table() {
    let run_id = "matrix-run";
    for expectation in CANONICAL_EXPECTATIONS {
        let events = parse_fixture_events(expectation, run_id);
        assert_eq!(
            events.len(),
            expectation.expected_event_count,
            "{} should produce deterministic event count",
            expectation.fixture_path
        );

        let first = events
            .first()
            .expect("fixture expectation table requires at least one event");
        assert_core_event_contract(first, expectation.source, run_id);
        assert_eq!(
            first.record_format, expectation.expected_first_record_format,
            "{} first-event record_format mismatch",
            expectation.fixture_path
        );
        assert_eq!(
            first.event_type, expectation.expected_first_event_type,
            "{} first-event event_type mismatch",
            expectation.fixture_path
        );
        assert_eq!(
            first.role, expectation.expected_first_role,
            "{} first-event role mismatch",
            expectation.fixture_path
        );
        assert_eq!(
            first.session_id.as_deref(),
            expectation.expected_first_session_id,
            "{} first-event session_id mismatch",
            expectation.fixture_path
        );
        assert_eq!(
            first.conversation_id.as_deref(),
            expectation.expected_first_conversation_id,
            "{} first-event conversation_id mismatch",
            expectation.fixture_path
        );
    }
}

#[test]
fn amp_fixture_expectation_table_matches_thread_envelope_contract() {
    let parsed =
        amp::parse_thread_envelope(include_str!("../../../fixtures/amp/thread_payloads.json"))
            .expect("amp thread fixture should parse");
    assert_eq!(parsed.thread.thread_id, "amp-t-001");
    assert_eq!(parsed.thread.message_count, parsed.messages.len());
    assert!(!parsed.messages.is_empty());
}

fn parse_fixture_events(
    expectation: &CanonicalFixtureExpectation,
    run_id: &str,
) -> Vec<AgentLogEvent> {
    assert!(
        expectation.source != AgentSource::Amp,
        "amp fixture uses envelope expectation test, not canonical event rows"
    );
    match expectation.source {
        AgentSource::Codex => {
            codex::parse_rollout_jsonl(
                include_str!("../../../fixtures/codex/rollout_primary.jsonl"),
                run_id,
                expectation.fixture_path,
            )
            .events
        }
        AgentSource::Claude => {
            claude::parse_project_session_jsonl(
                include_str!("../../../fixtures/claude/project_session.jsonl"),
                run_id,
                expectation.fixture_path,
            )
            .events
        }
        AgentSource::Gemini => {
            gemini::parse_chat_session_json(
                include_str!("../../../fixtures/gemini/chat_messages.json"),
                run_id,
                expectation.fixture_path,
            )
            .expect("gemini fixture should parse")
            .events
        }
        AgentSource::OpenCode => {
            opencode::parse_auxiliary_log_text(
                include_str!("../../../fixtures/opencode/runtime_prompt_history.log"),
                run_id,
                expectation.fixture_path,
            )
            .events
        }
        AgentSource::Amp => Vec::new(),
    }
}
