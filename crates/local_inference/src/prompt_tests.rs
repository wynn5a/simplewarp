use warp_multi_agent_api as api;

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

fn request_at(pwd: &str, seconds: i64, branch: &str) -> api::Request {
    api::Request {
        input: Some(api::request::Input {
            context: Some(api::InputContext {
                directory: Some(api::input_context::Directory {
                    pwd: pwd.to_string(),
                    home: "/home/me".to_string(),
                    ..Default::default()
                }),
                shell: Some(api::input_context::Shell {
                    name: "zsh".to_string(),
                    ..Default::default()
                }),
                current_time: Some(prost_types::Timestamp { seconds, nanos: 0 }),
                git: Some(api::input_context::Git {
                    branch: branch.to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn the_system_prompt_appends_the_stable_environment() {
    let prompt = Prompt::for_request(&request_at("/work/app", 0, "main"));

    assert!(prompt.system.starts_with(SYSTEM_PROMPT));
    assert!(prompt.system.contains("Shell: zsh"));
    assert!(prompt.system.contains("Home directory: /home/me"));
    assert!(!prompt.system.contains("/work/app"));
    assert!(!prompt.system.contains("Current time"));
    assert!(!prompt.system.contains("main"));
}

/// A provider caches a request by its prefix, so the system prompt must not change when only the
/// time, the directory or the branch does.
#[test]
fn the_system_prompt_is_stable_across_requests() {
    let first = Prompt::for_request(&request_at("/work/app", 0, "main"));
    let second = Prompt::for_request(&request_at("/work/lib", 3_600, "feature"));

    assert_eq!(first.system, second.system);
    assert_ne!(first.current_state, second.current_state);
    assert!(second.current_state.starts_with(CURRENT_STATE_INTRO));
    assert!(
        second
            .current_state
            .contains("- Working directory: /work/lib")
    );
    assert!(
        second
            .current_state
            .contains("- Current time: 1970-01-01 01:00 UTC")
    );
    assert!(second.current_state.contains("- Git branch: feature"));
}

#[test]
fn no_context_means_no_current_state() {
    let prompt = Prompt::for_request(&api::Request::default());

    assert_eq!(prompt.system, SYSTEM_PROMPT);
    assert_eq!(prompt.current_state, "");
}
