use warp_multi_agent_api as api;

use super::{add_global_rules, api_keys_with_warp_credit_fallback_setting};

#[test]
fn api_keys_with_warp_credit_fallback_setting_returns_none_without_keys_or_fallback() {
    let api_keys = api_keys_with_warp_credit_fallback_setting(None, false);

    assert!(api_keys.is_none());
}

#[test]
fn api_keys_with_warp_credit_fallback_setting_creates_fallback_only_api_keys() {
    let api_keys = api_keys_with_warp_credit_fallback_setting(None, true)
        .expect("fallback setting should create ApiKeys");

    assert!(api_keys.allow_use_of_warp_credits);
    assert!(api_keys.anthropic.is_empty());
    assert!(api_keys.openai.is_empty());
    assert!(api_keys.google.is_empty());
    assert!(api_keys.open_router.is_empty());
    assert!(api_keys.aws_credentials.is_none());
}

#[test]
fn api_keys_with_warp_credit_fallback_setting_preserves_existing_keys() {
    let api_keys = api_keys_with_warp_credit_fallback_setting(
        Some(api::request::settings::ApiKeys {
            anthropic: "anthropic-key".to_string(),
            openai: String::new(),
            google: String::new(),
            open_router: String::new(),
            grok_oauth_access_token: String::new(),
            allow_use_of_warp_credits: false,
            aws_credentials: None,
            google_cloud_credentials: None,
        }),
        true,
    )
    .expect("existing ApiKeys should be preserved");

    assert_eq!(api_keys.anthropic, "anthropic-key");
    assert!(api_keys.allow_use_of_warp_credits);
}

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
