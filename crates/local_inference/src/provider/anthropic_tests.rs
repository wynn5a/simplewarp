use serde_json::json;

use super::*;
use crate::config::Schema;
use crate::convert::{ToolResult, ToolUse};

fn target() -> ProviderTarget {
    ProviderTarget {
        schema: Schema::AnthropicMessages,
        base_url: "https://api.anthropic.com/v1".to_string(),
        api_key: "sk-ant-test".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        is_custom: false,
    }
}

#[test]
fn the_api_key_goes_in_a_header_not_the_body() {
    let headers = headers(&target());
    assert!(headers.contains(&("x-api-key", "sk-ant-test".to_string())));

    let body = build_body(&target(), "be helpful", &[], &[]);
    assert!(!body.to_string().contains("sk-ant-test"));
}

#[test]
fn the_system_prompt_is_a_top_level_field() {
    let body = build_body(
        &target(),
        "be helpful",
        &[Turn::User("hi".to_string())],
        &[],
    );
    assert_eq!(body["system"], "be helpful");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["stream"], true);
}

#[test]
fn tool_results_go_into_a_user_turn() {
    let turns = vec![Turn::ToolResults(vec![ToolResult {
        id: "toolu_1".to_string(),
        content: "exit code: 0".to_string(),
        is_error: false,
    }])];

    let body = build_body(&target(), "", &turns, &[]);
    let message = &body["messages"][0];
    assert_eq!(message["role"], "user");
    assert_eq!(message["content"][0]["type"], "tool_result");
    assert_eq!(message["content"][0]["tool_use_id"], "toolu_1");
    assert_eq!(message["content"][0]["is_error"], false);
}

#[test]
fn tool_arguments_stay_an_object() {
    let turns = vec![Turn::Assistant {
        text: "looking".to_string(),
        reasoning: String::new(),
        tool_calls: vec![ToolUse {
            id: "toolu_1".to_string(),
            name: "grep".to_string(),
            arguments: json!({ "queries": ["fn main"] }),
        }],
    }];

    let body = build_body(&target(), "", &turns, &[]);
    let content = &body["messages"][0]["content"];
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "tool_use");
    assert!(
        content[1]["input"].is_object(),
        "input must not be a string"
    );
}

#[test]
fn a_tool_use_block_starts_a_call() {
    let deltas = parse_event(
        "content_block_start",
        r#"{"index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"grep"}}"#,
    );
    assert_eq!(
        deltas,
        vec![Delta::ToolCallStart {
            index: 1,
            id: "toolu_1".to_string(),
            name: "grep".to_string(),
        }]
    );
}

#[test]
fn a_text_block_start_produces_nothing() {
    let deltas = parse_event(
        "content_block_start",
        r#"{"index":0,"content_block":{"type":"text","text":""}}"#,
    );
    assert!(deltas.is_empty());
}

#[test]
fn text_and_argument_deltas_are_read() {
    assert_eq!(
        parse_event(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"text_delta","text":"Hello"}}"#
        ),
        vec![Delta::Text("Hello".to_string())]
    );
    assert_eq!(
        parse_event(
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"a\":"}}"#
        ),
        vec![Delta::ToolCallArguments {
            index: 1,
            fragment: "{\"a\":".to_string(),
        }]
    );
    assert_eq!(
        parse_event(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"thinking_delta","thinking":"let me see"}}"#
        ),
        vec![Delta::Reasoning("let me see".to_string())]
    );
}

#[test]
fn the_stop_reason_comes_from_message_delta() {
    assert_eq!(
        parse_event("message_delta", r#"{"delta":{"stop_reason":"tool_use"}}"#),
        vec![Delta::Stop(StopReason::ToolUse)]
    );
    assert_eq!(
        parse_event("message_delta", r#"{"delta":{"stop_reason":"end_turn"}}"#),
        vec![Delta::Stop(StopReason::EndTurn)]
    );
    assert_eq!(
        parse_event("message_delta", r#"{"delta":{"stop_reason":"max_tokens"}}"#),
        vec![Delta::Stop(StopReason::MaxTokens)]
    );
}

#[test]
fn message_stop_does_not_end_the_reply_a_second_time() {
    assert!(parse_event("message_stop", "{}").is_empty());
}

#[test]
fn a_broken_event_is_ignored() {
    assert!(parse_event("content_block_delta", "not json").is_empty());
    assert!(parse_event("content_block_start", r#"{"index":0}"#).is_empty());
}
