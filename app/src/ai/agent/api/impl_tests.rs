use warp_multi_agent_api as api;

use super::{add_global_rules, convert_local_inference_error};
use crate::server::server_api::AIApiError;

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

#[test]
fn a_rate_limit_inside_the_stream_maps_to_a_429() {
    let error = convert_local_inference_error(local_inference::Error::ProviderStream {
        code: "subscription_sharing_usage_limit_exceeded".to_string(),
        message: "You have reached your plan limit.".to_string(),
    });
    let AIApiError::ErrorStatus(status, body) = error.as_ref() else {
        panic!("expected an HTTP status error, got {error:?}");
    };
    assert_eq!(*status, http::StatusCode::TOO_MANY_REQUESTS);
    assert!(body.contains("You have reached your plan limit."));
}

#[test]
fn any_other_error_inside_the_stream_is_not_a_quota_limit() {
    let error = convert_local_inference_error(local_inference::Error::ProviderStream {
        code: "server_error".to_string(),
        message: "boom".to_string(),
    });
    assert!(matches!(error.as_ref(), AIApiError::Other(_)));
}
