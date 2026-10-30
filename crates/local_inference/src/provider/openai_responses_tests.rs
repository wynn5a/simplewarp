use serde_json::json;

use super::*;
use crate::config::Schema;
use crate::convert::{ToolResult, ToolUse};

fn target() -> ProviderTarget {
    ProviderTarget {
        schema: Schema::OpenaiResponses,
        base_url: "https://api.openai.com/v1".to_string(),
        api_key: "sk-openai-test".to_string(),
        model: "gpt-5".to_string(),
        is_custom: true,
    }
}

fn grep_call(id: &str) -> ToolUse {
    ToolUse {
        id: id.to_string(),
        name: "grep".to_string(),
        arguments: json!({ "queries": ["fn main"] }),
    }
}

#[test]
fn the_api_key_goes_in_a_bearer_header_not_the_body() {
    assert_eq!(
        headers(&target()),
        vec![("authorization", "Bearer sk-openai-test".to_string())]
    );

    let body = build_body(&target(), "be helpful", &[], &[]);
    assert!(!body.to_string().contains("sk-openai-test"));
}

#[test]
fn the_endpoint_is_responses() {
    assert_eq!(
        target().endpoint_url(),
        "https://api.openai.com/v1/responses"
    );
}

#[test]
fn the_request_is_streamed_and_not_stored() {
    let body = build_body(&target(), "", &[], &[]);
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(body["model"], "gpt-5");
    assert!(body.get("temperature").is_none());
    assert!(body.get("max_output_tokens").is_none());
}

#[test]
fn the_system_prompt_is_the_instructions_not_an_input_item() {
    let body = build_body(
        &target(),
        "be helpful",
        &[Turn::User("hi".to_string())],
        &[],
    );
    assert_eq!(body["instructions"], "be helpful");
    let input = body["input"].as_array().expect("an array");
    assert_eq!(input.len(), 1);
    assert_eq!(
        input[0],
        json!({ "type": "message", "role": "user", "content": "hi" })
    );
}

#[test]
fn a_tool_call_and_its_result_are_paired_by_call_id() {
    let turns = vec![
        Turn::User("find main".to_string()),
        Turn::Assistant {
            text: "Searching.".to_string(),
            reasoning: "I should grep.".to_string(),
            tool_calls: vec![grep_call("call_1")],
        },
        Turn::ToolResults(vec![ToolResult {
            id: "call_1".to_string(),
            content: "src/main.rs".to_string(),
            is_error: false,
        }]),
    ];

    let body = build_body(&target(), "", &turns, &[]);
    let input = body["input"].as_array().expect("an array");
    assert_eq!(input.len(), 4);
    assert_eq!(
        input[1],
        json!({ "type": "message", "role": "assistant", "content": "Searching." })
    );
    assert_eq!(input[2]["type"], "function_call");
    assert_eq!(input[2]["call_id"], "call_1");
    assert_eq!(input[2]["name"], "grep");
    let arguments = &input[2]["arguments"];
    assert!(arguments.is_string(), "arguments must be a string");
    assert!(arguments.as_str().expect("a string").contains("fn main"));
    assert_eq!(
        input[3],
        json!({ "type": "function_call_output", "call_id": "call_1", "output": "src/main.rs" })
    );
    assert!(
        !body.to_string().contains("I should grep."),
        "reasoning is not replayed"
    );
}

#[test]
fn an_assistant_turn_with_only_tool_calls_sends_no_empty_message() {
    let turns = vec![Turn::Assistant {
        text: String::new(),
        reasoning: String::new(),
        tool_calls: vec![grep_call("call_1"), grep_call("call_2")],
    }];

    let body = build_body(&target(), "", &turns, &[]);
    let input = body["input"].as_array().expect("an array");
    assert_eq!(input.len(), 2);
    assert_eq!(input[0]["call_id"], "call_1");
    assert_eq!(input[1]["call_id"], "call_2");
}

#[test]
fn a_tool_schema_is_a_flat_function() {
    let tools = crate::tools::schemas_for(&[]);
    let body = build_body(&target(), "", &[], &tools);
    assert_eq!(body["tools"][0]["type"], "function");
    assert!(body["tools"][0]["name"].is_string());
    assert!(body["tools"][0]["parameters"].is_object());
    assert!(body["tools"][0].get("function").is_none());
}

#[test]
fn no_tools_sends_no_tools_field() {
    let body = build_body(&target(), "", &[], &[]);
    assert!(body.get("tools").is_none());
}

#[test]
fn a_text_delta_becomes_text() {
    assert_eq!(
        parse_event(r#"{"type":"response.output_text.delta","output_index":0,"delta":"Hello"}"#),
        vec![Delta::Text("Hello".to_string())]
    );
}

#[test]
fn a_reasoning_summary_delta_becomes_reasoning() {
    assert_eq!(
        parse_event(
            r#"{"type":"response.reasoning_summary_text.delta","output_index":0,"delta":"Hmm"}"#
        ),
        vec![Delta::Reasoning("Hmm".to_string())]
    );
}

#[test]
fn a_tool_call_opens_with_its_call_id_then_streams_arguments() {
    let opening = parse_event(
        r#"{"type":"response.output_item.added","output_index":1,
            "item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"grep",
                    "arguments":""}}"#,
    );
    assert_eq!(
        opening,
        vec![Delta::ToolCallStart {
            index: 1,
            id: "call_1".to_string(),
            name: "grep".to_string(),
        }]
    );

    let continuing = parse_event(
        r#"{"type":"response.function_call_arguments.delta","output_index":1,
            "item_id":"fc_1","delta":"{\"queries\""}"#,
    );
    assert_eq!(
        continuing,
        vec![Delta::ToolCallArguments {
            index: 1,
            fragment: "{\"queries\"".to_string(),
        }]
    );
}

#[test]
fn parallel_tool_calls_keep_their_own_index() {
    let second = parse_event(
        r#"{"type":"response.output_item.added","output_index":2,
            "item":{"type":"function_call","call_id":"call_2","name":"read_files"}}"#,
    );
    assert_eq!(
        second,
        vec![Delta::ToolCallStart {
            index: 2,
            id: "call_2".to_string(),
            name: "read_files".to_string(),
        }]
    );
}

#[test]
fn a_message_or_reasoning_item_is_not_a_tool_call() {
    assert!(
        parse_event(
            r#"{"type":"response.output_item.added","output_index":0,
                "item":{"type":"message","role":"assistant"}}"#
        )
        .is_empty()
    );
    assert!(
        parse_event(
            r#"{"type":"response.output_item.added","output_index":0,
                "item":{"type":"reasoning"}}"#
        )
        .is_empty()
    );
}

#[test]
fn completed_reports_usage_then_the_stop_reason() {
    let deltas = parse_event(
        r#"{"type":"response.completed","response":{"status":"completed",
            "output":[{"type":"message"}],
            "usage":{"input_tokens":812,"output_tokens":40,
                     "input_tokens_details":{"cached_tokens":500}}}}"#,
    );
    assert_eq!(
        deltas,
        vec![
            Delta::Usage(TokenUsage {
                input: 812,
                output: 40,
                cache_read: 500,
                cache_write: 0,
            }),
            Delta::Stop(StopReason::EndTurn),
        ]
    );
}

#[test]
fn completed_with_a_function_call_is_a_tool_use_stop() {
    assert_eq!(
        parse_event(
            r#"{"type":"response.completed","response":{"status":"completed",
                "output":[{"type":"reasoning"},{"type":"function_call","call_id":"call_1"}]}}"#
        ),
        vec![Delta::Stop(StopReason::ToolUse)]
    );
}

#[test]
fn incomplete_at_the_output_limit_is_a_max_tokens_stop() {
    assert_eq!(
        parse_event(
            r#"{"type":"response.incomplete","response":{"status":"incomplete",
                "incomplete_details":{"reason":"max_output_tokens"}}}"#
        ),
        vec![Delta::Stop(StopReason::MaxTokens)]
    );
    assert_eq!(
        parse_event(
            r#"{"type":"response.incomplete","response":{"status":"incomplete",
                "incomplete_details":{"reason":"content_filter"}}}"#
        ),
        vec![Delta::Stop(StopReason::Other)]
    );
}

#[test]
fn a_failed_response_is_an_error() {
    assert_eq!(
        parse_event(
            r#"{"type":"response.failed","response":{"status":"failed",
                "error":{"code":"server_error","message":"boom"}}}"#
        ),
        vec![Delta::Error {
            code: "server_error".to_string(),
            message: "boom".to_string(),
        }]
    );
}

#[test]
fn an_error_event_is_an_error() {
    assert_eq!(
        parse_event(
            r#"{"type":"error","code":"rate_limit_exceeded","message":"slow down",
                "sequence_number":3}"#
        ),
        vec![Delta::Error {
            code: "rate_limit_exceeded".to_string(),
            message: "slow down".to_string(),
        }]
    );
}

#[test]
fn bookkeeping_and_broken_events_are_ignored() {
    for data in [
        r#"{"type":"response.created","response":{"status":"in_progress"}}"#,
        r#"{"type":"response.output_text.done","text":"Hello"}"#,
        r#"{"type":"response.output_text.delta","delta":""}"#,
        "[DONE]",
        "not json",
    ] {
        assert!(parse_event(data).is_empty(), "expected nothing for {data}");
    }
}

#[test]
fn a_finished_function_call_item_carries_the_whole_arguments() {
    assert_eq!(
        parse_event(
            r#"{"type":"response.output_item.done","output_index":1,
                "item":{"type":"function_call","call_id":"call_1","name":"grep",
                        "arguments":"{\"queries\":[\"fn main\"]}"}}"#
        ),
        vec![
            Delta::ToolCallStart {
                index: 1,
                id: "call_1".to_string(),
                name: "grep".to_string(),
            },
            Delta::ToolCallArguments {
                index: 1,
                fragment: r#"{"queries":["fn main"]}"#.to_string(),
            },
        ]
    );
}

#[test]
fn a_refusal_is_shown_as_text() {
    assert_eq!(
        parse_event(r#"{"type":"response.refusal.delta","output_index":0,"delta":"I can't."}"#),
        vec![Delta::Text("I can't.".to_string())]
    );
}

#[test]
fn a_later_reasoning_summary_part_is_set_apart() {
    assert!(
        parse_event(r#"{"type":"response.reasoning_summary_part.added","summary_index":0}"#)
            .is_empty()
    );
    assert_eq!(
        parse_event(r#"{"type":"response.reasoning_summary_part.added","summary_index":1}"#),
        vec![Delta::Reasoning("\n\n".to_string())]
    );
}

#[test]
fn a_nested_error_event_is_an_error() {
    assert_eq!(
        parse_event(
            r#"{"type":"error","error":{"type":"usage_limit_reached",
                "code":"subscription_sharing_usage_limit_exceeded","message":"limit"}}"#
        ),
        vec![Delta::Error {
            code: "subscription_sharing_usage_limit_exceeded".to_string(),
            message: "limit".to_string(),
        }]
    );
}

#[test]
fn a_failed_response_without_an_error_still_says_it_failed() {
    let deltas = parse_event(r#"{"type":"response.failed","response":{"status":"failed"}}"#);
    let [Delta::Error { message, .. }] = deltas.as_slice() else {
        panic!("expected one error, got {deltas:?}");
    };
    assert!(!message.is_empty());
}
