use logit::{adapters, discovery};

#[test]
fn adapter_registry_has_five_supported_agents() {
    let kinds = adapters::all_adapter_kinds();
    assert_eq!(kinds.len(), 5);
}

#[test]
fn discovery_registry_has_paths_for_each_adapter() {
    let rules = discovery::known_path_registry();
    assert_eq!(rules.len(), 5);
    assert!(rules.iter().all(|rule| !rule.candidate_paths.is_empty()));
}
