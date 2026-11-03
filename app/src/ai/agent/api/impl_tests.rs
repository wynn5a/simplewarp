use std::collections::HashMap;
use std::sync::Arc;

use ai::agent::action_result::{AnyFileContent, FileContext, ReadFilesResult};
use warp_multi_agent_api as api;

use super::{
    add_memories, convert_local_inference_error, convert_local_inference_failure,
    estimate_input_tokens,
};
use crate::ai::agent::task::TaskId;
use crate::ai::agent::{
    AIAgentActionResult, AIAgentActionResultType, AIAgentAttachment, AIAgentInput,
    RenderableAIError, UserQueryMode,
};
use crate::ai::agent_memory::{AGENT_MEMORY_TITLE, ProjectMemories, RequestMemories, RuleText};
use crate::server::server_api::AIApiError;

fn rule(title: &str, content: &str) -> RuleText {
    (title.to_owned(), content.to_owned())
}

#[test]
fn global_rules_reach_the_request_as_a_project_rules_entry_without_a_root() {
    let mut input = api::request::Input::default();
    add_memories(
        &mut input,
        &RequestMemories {
            global: vec![rule("Tabs", "Use tabs."), rule("Rule", "Be brief.")],
            project: None,
        },
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
    add_memories(&mut input, &RequestMemories::default());
    assert!(input.context.is_none());
}

#[test]
fn project_memories_join_the_project_rules_with_the_same_root() {
    let mut input = api::request::Input {
        context: Some(api::InputContext {
            project_rules: vec![api::input_context::ProjectRules {
                root_path: "/work/app".to_owned(),
                active_rule_files: vec![api::FileContent {
                    file_path: "/work/app/AGENTS.md".to_owned(),
                    content: "Run the presubmit.".to_owned(),
                    line_range: None,
                }],
                additional_rule_file_paths: vec![],
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    add_memories(
        &mut input,
        &RequestMemories {
            global: vec![],
            project: Some(ProjectMemories {
                root_path: "/work/app".to_owned(),
                rules: vec![rule(AGENT_MEMORY_TITLE, "Tests run with cargo nextest.")],
            }),
        },
    );

    let rules = &input.context.expect("context").project_rules;
    assert_eq!(rules.len(), 1);
    let files = &rules[0].active_rule_files;
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].file_path, "/work/app/AGENTS.md");
    assert_eq!(files[1].file_path, AGENT_MEMORY_TITLE);
    assert_eq!(files[1].content, "Tests run with cargo nextest.");
}

#[test]
fn project_memories_without_project_rules_get_their_own_entry() {
    let mut input = api::request::Input::default();

    add_memories(
        &mut input,
        &RequestMemories {
            global: vec![rule("Tabs", "Use tabs.")],
            project: Some(ProjectMemories {
                root_path: "/work/app".to_owned(),
                rules: vec![rule(AGENT_MEMORY_TITLE, "Tests run with cargo nextest.")],
            }),
        },
    );

    let rules = &input.context.expect("context").project_rules;
    assert_eq!(rules.len(), 2);
    assert!(rules[0].root_path.is_empty());
    assert_eq!(rules[1].root_path, "/work/app");
    assert_eq!(rules[1].active_rule_files[0].file_path, AGENT_MEMORY_TITLE);
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

fn read_files_input(content: &str) -> AIAgentInput {
    AIAgentInput::ActionResult {
        result: AIAgentActionResult {
            id: "call-1".to_string().into(),
            task_id: TaskId::new("task".to_string()),
            result: AIAgentActionResultType::ReadFiles(ReadFilesResult::Success {
                files: vec![FileContext::new(
                    "big.rs".to_string(),
                    AnyFileContent::StringContent(content.to_string()),
                    None,
                    None,
                )],
                failed_files: Vec::new(),
            }),
        },
        context: Arc::new([]),
    }
}

#[test]
fn a_file_read_is_estimated_by_its_content_not_its_path() {
    let small = estimate_input_tokens(vec![read_files_input(&"x".repeat(400))]);
    let large = estimate_input_tokens(vec![read_files_input(&"x".repeat(8_000))]);

    assert!(small >= 100, "{small}");
    assert!(large >= 2_000, "{large}");
    // Proportional to the content, give or take the file header.
    assert!(
        large - small >= 1_900 && large - small <= 1_910,
        "{small} {large}"
    );
}

#[test]
fn a_question_is_estimated_with_its_attachments() {
    let question = |attachments: HashMap<String, AIAgentAttachment>| AIAgentInput::UserQuery {
        query: "why did it fail?".to_string(),
        context: Arc::new([]),
        referenced_attachments: attachments,
        user_query_mode: UserQueryMode::Normal,
        running_command: None,
        intended_agent: None,
    };
    let bare = estimate_input_tokens(vec![question(HashMap::new())]);
    let attached = estimate_input_tokens(vec![question(HashMap::from([(
        "block".to_string(),
        AIAgentAttachment::PlainText("y".repeat(4_000)),
    )]))]);

    assert!(bare > 0);
    assert!(attached >= bare + 1_000, "{bare} {attached}");
}

#[test]
fn no_input_is_estimated_as_nothing() {
    assert_eq!(estimate_input_tokens(Vec::new()), 0);
}

#[test]
fn a_numeric_429_inside_the_stream_maps_to_a_429() {
    let error = convert_local_inference_error(local_inference::Error::ProviderStream {
        code: "429".to_string(),
        message: "Rate limit exceeded: free-models-per-day".to_string(),
    });
    let AIApiError::ErrorStatus(status, _) = error.as_ref() else {
        panic!("expected an HTTP status error, got {error:?}");
    };
    assert_eq!(*status, http::StatusCode::TOO_MANY_REQUESTS);
}

#[test]
fn local_failures_carry_the_retry_facts() {
    let overloaded = convert_local_inference_failure(
        local_inference::Error::ProviderStatus {
            status: 503,
            body: String::new(),
            retry_after: Some(std::time::Duration::from_secs(2)),
        },
        /*before_reply*/ true,
    );
    let AIApiError::Provider { error, failure } = overloaded.as_ref() else {
        panic!("expected a provider failure, got {overloaded:?}");
    };
    assert!(failure.before_reply && failure.retryable);
    assert_eq!(failure.retry_after, Some(std::time::Duration::from_secs(2)));
    assert!(matches!(
        error.as_ref(),
        AIApiError::ErrorStatus(http::StatusCode::SERVICE_UNAVAILABLE, _)
    ));

    let no_key = convert_local_inference_failure(
        local_inference::Error::NoApiKey {
            model: "m".to_string(),
        },
        /*before_reply*/ true,
    );
    let AIApiError::Provider { failure, .. } = no_key.as_ref() else {
        panic!("expected a provider failure, got {no_key:?}");
    };
    assert!(!failure.retryable);

    let rate_limit = convert_local_inference_failure(
        local_inference::Error::ProviderStatus {
            status: 429,
            body: String::new(),
            retry_after: None,
        },
        /*before_reply*/ true,
    );
    assert!(matches!(
        RenderableAIError::from(&rate_limit),
        RenderableAIError::QuotaLimit { .. }
    ));
}
