use logit::adapters::opencode::parse_session_metadata_jsonl;

#[test]
fn parses_opencode_message_metadata_fixture() {
    let raw = include_str!("../../../fixtures/opencode/session_messages.jsonl");
    let parsed = parse_session_metadata_jsonl(raw).expect("fixture should parse");

    assert!(parsed.warnings.is_empty());
    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(parsed.sessions.len(), 1);

    let session = &parsed.sessions[0];
    assert_eq!(session.session_id, "oc-s-001");
    assert_eq!(session.message_count, 2);
    assert_eq!(
        session.first_created_at.as_deref(),
        Some("2026-02-05T07:00:00Z")
    );
    assert_eq!(
        session.last_created_at.as_deref(),
        Some("2026-02-05T07:00:02Z")
    );
    assert_eq!(session.roles_seen, vec!["assistant", "user"]);
    assert_eq!(session.model_hints, vec!["gpt-5"]);
    assert_eq!(session.provider_hints, vec!["openai"]);
}

#[test]
fn handles_malformed_lines_without_crashing() {
    let raw = r#"
{"sessionID":"oc-s-001","messageID":"msg-001","role":"user"}
not-json
{"sessionID":"oc-s-001","messageID":"msg-002"}
{"messageID":"msg-003","role":"assistant"}
"#;
    let parsed =
        parse_session_metadata_jsonl(raw).expect("parser should continue on malformed rows");

    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(parsed.sessions[0].message_count, 2);
    assert!(parsed.warnings.iter().any(|warning| {
        warning.contains("line 3: invalid JSON payload in opencode session metadata JSONL")
    }));
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("missing required `sessionID`"))
    );
}

#[test]
fn parses_session_info_records_with_message_rows() {
    let raw = r#"
{"sessionID":"oc-s-200","title":"Release prep","workspacePath":"/tmp/workspace"}
{"sessionID":"oc-s-200","messageID":"msg-201","createdAt":"2026-02-06T10:00:00Z","role":"user"}
{"session_id":"oc-s-200","message_id":"msg-202","created_at":"2026-02-06T10:00:01Z","role":"assistant"}
"#;

    let parsed =
        parse_session_metadata_jsonl(raw).expect("session info + message rows should parse");

    assert_eq!(parsed.sessions.len(), 1);
    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(parsed.session_info.len(), 1);
    assert_eq!(parsed.session_info[0].session_id, "oc-s-200");
    assert_eq!(
        parsed.session_info[0].title.as_deref(),
        Some("Release prep")
    );
    assert_eq!(
        parsed.session_info[0].workspace_path.as_deref(),
        Some("/tmp/workspace")
    );
}

#[test]
fn maps_null_and_blank_optional_fields_to_stable_defaults() {
    let raw = r#"
{"sessionID":"oc-s-301","messageID":"msg-301","createdAt":"2026-02-07T11:00:00Z","role":null,"model":null,"provider":null}
{"sessionID":"oc-s-301","messageID":"msg-302","createdAt":"2026-02-07T11:00:01Z","role":"   ","model":"gpt-5-mini","provider":"openai"}
{"sessionID":"oc-s-301","title":null,"workspacePath":" /tmp/opencode "}
"#;

    let parsed = parse_session_metadata_jsonl(raw).expect("null/blank rows should parse");

    assert!(parsed.warnings.is_empty());
    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(parsed.messages[0].role, "unknown");
    assert_eq!(parsed.messages[0].model, None);
    assert_eq!(parsed.messages[0].provider, None);
    assert_eq!(parsed.messages[1].role, "unknown");
    assert_eq!(parsed.messages[1].model.as_deref(), Some("gpt-5-mini"));
    assert_eq!(parsed.session_info.len(), 1);
    assert_eq!(
        parsed.session_info[0].workspace_path.as_deref(),
        Some("/tmp/opencode")
    );
}
