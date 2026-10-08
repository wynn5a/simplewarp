use super::*;

#[test]
fn the_prompt_names_every_tool_rule_the_client_depends_on() {
    // The client asks the user to approve a command that is not marked read-only, so the model
    // must be told what the mark decides.
    assert!(SYSTEM_PROMPT.contains("is_read_only"));
    // The client renders agent output as Markdown.
    assert!(SYSTEM_PROMPT.contains("Markdown"));
    // An exact-match search is the one way a diff can fail silently.
    assert!(SYSTEM_PROMPT.contains("apply_file_diffs"));
}

#[test]
fn the_system_prompt_appends_the_request_environment() {
    use warp_multi_agent_api as api;

    let request = api::Request {
        input: Some(api::request::Input {
            context: Some(api::InputContext {
                directory: Some(api::input_context::Directory {
                    pwd: "/work/app".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let prompt = system_prompt(&request);
    assert!(prompt.starts_with(SYSTEM_PROMPT));
    assert!(prompt.contains("Working directory: /work/app"));

    assert_eq!(system_prompt(&api::Request::default()), SYSTEM_PROMPT);
}
