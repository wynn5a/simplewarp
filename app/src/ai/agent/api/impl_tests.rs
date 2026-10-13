use warp_multi_agent_api as api;

use super::add_global_rules;

#[test]
fn global_rules_reach_the_request_as_a_project_rules_entry_without_a_root() {
    let mut input = api::request::Input::default();
    add_global_rules(
        &mut input,
        &[
            ("Tabs".to_owned(), "Use tabs.".to_owned()),
            (String::new(), "Be brief.".to_owned()),
        ],
    );

    let rules = &input.context.expect("context").project_rules;
    assert_eq!(rules.len(), 1);
    assert!(rules[0].root_path.is_empty());
    let files = &rules[0].active_rule_files;
    assert_eq!(files[0].file_path, "Tabs");
    assert_eq!(files[0].content, "Use tabs.");
    assert_eq!(files[1].file_path, "Rule");
}

#[test]
fn no_global_rules_leaves_the_context_alone() {
    let mut input = api::request::Input::default();
    add_global_rules(&mut input, &[]);
    assert!(input.context.is_none());
}
