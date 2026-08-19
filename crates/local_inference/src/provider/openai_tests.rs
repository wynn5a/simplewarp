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

#[test]
fn the_api_key_goes_in_a_bearer_header_not_the_body() {
    let headers = headers(&target());
    assert_eq!(
        headers,
        vec![("authorization", "Bearer sk-openai-test".to_string())]
    );

    let body = build_body(&target(), "be helpful", &[], &[]);
    assert!(!body.to_string().contains("sk-openai-test"));
}

#[test]
fn the_system_prompt_is_the_first_message() {
    let body = build_body(
        &target(),
        "be helpful",
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

    let body = build_body(&target(), "", &turns, &[]);
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

    let body = build_body(&target(), "", &turns, &[]);
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
    let body = build_body(&target(), "", &[], &tools);
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

    let body = build_body(&target, "", &turns, &[]);
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

    let body = build_body(&target, "", &turns, &[]);
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

    let body = build_body(&target(), "", &turns, &[]);
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

    let body = build_body(&target, "", &turns, &[]);
    assert!(body["messages"][1]["reasoning_content"].is_null());
}
