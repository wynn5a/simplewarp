use super::*;

#[test]
fn a_user_rule_for_all_work_has_no_origin_label() {
    assert_eq!(origin_label(AIMemorySource::User, None), None);
}

#[test]
fn agent_memories_and_project_rules_are_labelled() {
    assert_eq!(
        origin_label(AIMemorySource::Agent, None),
        Some("Added by agent".to_owned())
    );
    assert_eq!(
        origin_label(AIMemorySource::Agent, Some("/work/app")),
        Some("Added by agent · Project: /work/app".to_owned())
    );
    assert_eq!(
        origin_label(AIMemorySource::User, Some("/work/app")),
        Some("Project: /work/app".to_owned())
    );
}
