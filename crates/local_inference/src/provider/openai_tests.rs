use serde_json::json;

use super::*;
use crate::config::Schema;
use crate::convert::{ToolResult, ToolUse};

fn target() -> ProviderTarget {
    ProviderTarget {
        schema: Schema::OpenaiChatCompletions,
        base_url: "https://api.openai.com/v1".to_string(),
        api_key: "sk-openai-test".to_string(),
        model: "gpt-5".to_string(),
        is_custom: false,
    }
}

fn system(text: &str) -> Prompt {
    Prompt {
        system: text.to_string(),
        ..Default::default()
    }
}

#[test]
fn the_api_key_goes_in_a_bearer_header_not_the_body() {
    let headers = headers(&target());
    assert_eq!(
        headers,
        vec![("authorization", "Bearer sk-openai-test".to_string())]
    );

    let body = build_body(&target(), &system("be helpful"), &[], &[]);
    assert!(!body.to_string().contains("sk-openai-test"));
}

#[test]
fn the_system_prompt_is_the_first_message() {
    let body = build_body(
        &target(),
        &system("be helpful"),
        &[Turn::User("hi".to_string())],
        &[],
    );
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], "be helpful");
    assert_eq!(body["messages"][1]["role"], "user");
}

#[test]
fn tool_arguments_are_sent_as_a_json_string() {
    let turns = vec![Turn::Assistant {
        text: String::new(),
        reasoning: String::new(),
        tool_calls: vec![ToolUse {
            id: "call_1".to_string(),
            name: "grep".to_string(),
            arguments: json!({ "queries": ["fn main"] }),
        }],
    }];

    let body = build_body(&target(), &system(""), &turns, &[]);
    let message = &body["messages"][1];
    assert!(message["content"].is_null());
    let arguments = &message["tool_calls"][0]["function"]["arguments"];
    assert!(arguments.is_string(), "arguments must be a string");
    assert!(arguments.as_str().expect("a string").contains("fn main"));
}

#[test]
fn each_tool_result_is_its_own_message() {
    let turns = vec![Turn::ToolResults(vec![
        ToolResult {
            id: "call_1".to_string(),
            content: "first".to_string(),
            is_error: false,
        },
        ToolResult {
            id: "call_2".to_string(),
            content: "second".to_string(),
            is_error: false,
        },
    ])];

    let body = build_body(&target(), &system(""), &turns, &[]);
    let messages = body["messages"].as_array().expect("an array");
    // One system message, then one message per result.
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1]["role"], "tool");
    assert_eq!(messages[1]["tool_call_id"], "call_1");
    assert_eq!(messages[2]["tool_call_id"], "call_2");
}

#[test]
fn a_tool_schema_is_wrapped_in_a_function_object() {
    let tools = crate::tools::schemas_for(&[]);
    let body = build_body(&target(), &system(""), &[], &tools);
    assert_eq!(body["tools"][0]["type"], "function");
    assert!(body["tools"][0]["function"]["name"].is_string());
    assert!(body["tools"][0]["function"]["parameters"].is_object());
}

#[test]
fn a_content_delta_becomes_text() {
    assert_eq!(
        parse_event(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#),
        vec![Delta::Text("Hello".to_string())]
    );
}

#[test]
fn a_tool_call_opens_with_an_id_then_streams_arguments() {
    let opening = parse_event(
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
           "function":{"name":"grep","arguments":""}}]}}]}"#,
    );
    assert_eq!(
        opening,
        vec![Delta::ToolCallStart {
            index: 0,
            id: "call_1".to_string(),
            name: "grep".to_string(),
        }]
    );

    let continuing = parse_event(
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,
           "function":{"arguments":"{\"queries\""}}]}}]}"#,
    );
    assert_eq!(
        continuing,
        vec![Delta::ToolCallArguments {
            index: 0,
            fragment: "{\"queries\"".to_string(),
        }]
    );
}

#[test]
fn the_finish_reason_ends_the_reply() {
    assert_eq!(
        parse_event(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#),
        vec![Delta::Stop(StopReason::ToolUse)]
    );
    assert_eq!(
        parse_event(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#),
        vec![Delta::Stop(StopReason::EndTurn)]
    );
}

#[test]
fn done_does_not_end_the_reply_a_second_time() {
    assert!(parse_event("[DONE]").is_empty());
}

#[test]
fn the_body_asks_for_usage_in_the_stream() {
    let body = build_body(&target(), &system("be helpful"), &[], &[]);
    assert_eq!(body["stream_options"]["include_usage"], true);
}

#[test]
fn the_usage_chunk_is_read_even_without_choices() {
    assert_eq!(
        parse_event(
            r#"{"choices":[],"usage":{"prompt_tokens":812,"completion_tokens":40,
                "prompt_tokens_details":{"cached_tokens":512}}}"#
        ),
        vec![Delta::Usage(TokenUsage {
            input: 812,
            output: 40,
            cache_read: 512,
            cache_write: 0,
            reasoning: 0,
        })]
    );
}

#[test]
fn a_null_usage_is_ignored() {
    assert_eq!(
        parse_event(r#"{"choices":[{"delta":{"content":"hi"}}],"usage":null}"#),
        vec![Delta::Text("hi".to_string())]
    );
}

#[test]
fn reasoning_from_a_compatible_server_is_read() {
    assert_eq!(
        parse_event(r#"{"choices":[{"delta":{"reasoning":"thinking"}}]}"#),
        vec![Delta::Reasoning("thinking".to_string())]
    );
    assert_eq!(
        parse_event(r#"{"choices":[{"delta":{"reasoning_content":"thinking"}}]}"#),
        vec![Delta::Reasoning("thinking".to_string())]
    );
}

#[test]
fn a_broken_event_is_ignored() {
    assert!(parse_event("not json").is_empty());
    assert!(parse_event(r#"{"choices":[]}"#).is_empty());
}

/// A custom endpoint may front a reasoning model that refuses a tool-call message with no
/// `reasoning_content`. See the note in `push_messages`.
#[test]
fn a_custom_endpoint_gets_the_reasoning_back_with_a_tool_call() {
    let mut target = target();
    target.is_custom = true;

    let turns = vec![Turn::Assistant {
        text: String::new(),
        reasoning: "I should list the files.".to_string(),
        tool_calls: vec![ToolUse {
            id: "call_1".to_string(),
            name: "grep".to_string(),
            arguments: json!({ "queries": ["fn main"] }),
        }],
    }];

    let body = build_body(&target, &system(""), &turns, &[]);
    assert_eq!(
        body["messages"][1]["reasoning_content"],
        "I should list the files."
    );
}

/// The field must be present even when the reply streamed no thinking, because the provider
/// checks that it is there, not that it says anything.
#[test]
fn an_empty_reasoning_is_still_sent_to_a_custom_endpoint() {
    let mut target = target();
    target.is_custom = true;

    let turns = vec![Turn::Assistant {
        text: String::new(),
        reasoning: String::new(),
        tool_calls: vec![ToolUse {
            id: "call_1".to_string(),
            name: "grep".to_string(),
            arguments: json!({ "queries": ["fn main"] }),
        }],
    }];

    let body = build_body(&target, &system(""), &turns, &[]);
    assert_eq!(body["messages"][1]["reasoning_content"], "");
}

/// A first-party provider gets the official schema only, so an unknown field can never make it
/// reject the request.
#[test]
fn a_first_party_provider_gets_no_reasoning_field() {
    let turns = vec![Turn::Assistant {
        text: String::new(),
        reasoning: "thinking".to_string(),
        tool_calls: vec![ToolUse {
            id: "call_1".to_string(),
            name: "grep".to_string(),
            arguments: json!({ "queries": ["fn main"] }),
        }],
    }];

    let body = build_body(&target(), &system(""), &turns, &[]);
    assert!(body["messages"][1]["reasoning_content"].is_null());
}

/// A plain reply carries no tool call, and the probe showed such a message is accepted without
/// the field, so it stays off the ordinary chat path.
#[test]
fn a_reply_without_tool_calls_gets_no_reasoning_field() {
    let mut target = target();
    target.is_custom = true;

    let turns = vec![Turn::Assistant {
        text: "It prints the working directory.".to_string(),
        reasoning: "thinking".to_string(),
        tool_calls: Vec::new(),
    }];

    let body = build_body(&target, &system(""), &turns, &[]);
    assert!(body["messages"][1]["reasoning_content"].is_null());
}

fn with_state(current_state: &str) -> Prompt {
    Prompt {
        system: "be helpful".to_string(),
        current_state: current_state.to_string(),
    }
}

#[test]
fn current_state_is_appended_to_the_last_user_message() {
    let body = build_body(
        &target(),
        &with_state("Current environment:\n- Working directory: /work\n"),
        &[Turn::User("hi".to_string())],
        &[],
    );
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(
        messages[1]["content"],
        "hi\n\nCurrent environment:\n- Working directory: /work\n"
    );
}

#[test]
fn current_state_after_tool_results_is_a_new_user_message() {
    let turns = vec![
        Turn::User("find main".to_string()),
        Turn::Assistant {
            text: String::new(),
            reasoning: String::new(),
            tool_calls: vec![ToolUse {
                id: "call_1".to_string(),
                name: "grep".to_string(),
                arguments: json!({ "queries": ["fn main"] }),
            }],
        },
        Turn::ToolResults(vec![ToolResult {
            id: "call_1".to_string(),
            content: "found".to_string(),
            is_error: false,
        }]),
    ];
    let body = build_body(
        &target(),
        &with_state("Current environment:\n"),
        &turns,
        &[],
    );
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages[3]["role"], "tool");
    assert_eq!(messages[3]["content"], "found");
    assert_eq!(messages[4]["role"], "user");
    assert_eq!(messages[4]["content"], "Current environment:\n");
}

#[test]
fn empty_current_state_changes_nothing() {
    let turns = [Turn::User("hi".to_string())];
    assert_eq!(
        build_body(&target(), &with_state(""), &turns, &[]),
        build_body(&target(), &system("be helpful"), &turns, &[])
    );
    assert_eq!(
        build_body(&target(), &with_state(""), &turns, &[])["messages"][1]["content"],
        "hi"
    );
}

#[test]
fn an_error_chunk_is_an_error() {
    assert_eq!(
        parse_event(
            r#"{"error":{"message":"This model's maximum context length is 65536 tokens. However, you requested 70000 tokens.","type":"invalid_request_error","code":"context_length_exceeded"}}"#
        ),
        vec![Delta::Error {
            code: "context_length_exceeded".to_string(),
            message: "This model's maximum context length is 65536 tokens. However, you requested 70000 tokens.".to_string(),
        }]
    );
}

#[test]
fn an_error_chunk_with_a_numeric_code_keeps_it() {
    assert_eq!(
        parse_event(r#"{"error":{"code":502,"message":"Upstream error"}}"#),
        vec![Delta::Error {
            code: "502".to_string(),
            message: "Upstream error".to_string(),
        }]
    );
}

#[test]
fn reasoning_tokens_are_read_from_the_completion_details() {
    let deltas = parse_event(
        r#"{"choices":[],"usage":{"prompt_tokens":812,"completion_tokens":400,
            "completion_tokens_details":{"reasoning_tokens":320}}}"#,
    );
    let [Delta::Usage(usage)] = deltas.as_slice() else {
        panic!("expected usage, got {deltas:?}");
    };
    assert_eq!(usage.output, 400);
    assert_eq!(usage.reasoning, 320);
}
