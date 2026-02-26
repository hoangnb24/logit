use logit::adapters::AdapterKind;
use logit::discovery::{
    HistoryScore, SourceFormatHint, SourceSelectionFilter, known_path_registry, prioritize_sources,
    prioritized_sources,
};

#[test]
fn ranks_sources_by_history_score_then_precedence() {
    let history = r#"
: 1740467001:0;claude --resume
: 1740467002:0;claude --resume
: 1740467003:0;codex --full-auto
"#;

    let prioritized = prioritized_sources(history, &SourceSelectionFilter::default());
    assert!(!prioritized.is_empty());

    assert_eq!(prioritized[0].adapter, AdapterKind::Claude);
    assert_eq!(prioritized[0].precedence, 10);
    assert_eq!(prioritized[0].path, "~/.claude/projects");
}

#[test]
fn filters_by_adapter_and_source_kind() {
    let filter = SourceSelectionFilter {
        adapters: vec![AdapterKind::Claude],
        format_hints: vec![SourceFormatHint::Json],
        path_substrings: Vec::new(),
    };

    let prioritized = prioritized_sources("", &filter);
    assert_eq!(prioritized.len(), 1);
    assert_eq!(prioritized[0].adapter, AdapterKind::Claude);
    assert_eq!(prioritized[0].path, "~/.claude.json");
    assert_eq!(prioritized[0].format_hint, SourceFormatHint::Json);
}

#[test]
fn filters_by_path_substring_case_insensitively() {
    let filter = SourceSelectionFilter {
        adapters: Vec::new(),
        format_hints: Vec::new(),
        path_substrings: vec!["SESSIONS".to_string()],
    };

    let prioritized = prioritized_sources("", &filter);
    assert!(!prioritized.is_empty());
    assert!(
        prioritized
            .iter()
            .all(|entry| { entry.path.to_ascii_lowercase().contains("sessions") })
    );
}

#[test]
fn prioritization_is_deterministic_for_history_score_order() {
    let rules = known_path_registry();
    let filter = SourceSelectionFilter::default();

    let score_order_a = vec![
        HistoryScore {
            adapter: AdapterKind::Gemini,
            score: 2,
        },
        HistoryScore {
            adapter: AdapterKind::Codex,
            score: 2,
        },
        HistoryScore {
            adapter: AdapterKind::Claude,
            score: 1,
        },
    ];

    let score_order_b = vec![
        HistoryScore {
            adapter: AdapterKind::Claude,
            score: 1,
        },
        HistoryScore {
            adapter: AdapterKind::Codex,
            score: 2,
        },
        HistoryScore {
            adapter: AdapterKind::Gemini,
            score: 2,
        },
    ];

    let prioritized_a = prioritize_sources(&rules, &score_order_a, &filter);
    let prioritized_b = prioritize_sources(&rules, &score_order_b, &filter);

    assert_eq!(prioritized_a, prioritized_b);
}
