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

fn system(text: &str) -> Prompt {
    Prompt {
        system: text.to_string(),
        ..Default::default()
    }
}

#[test]
fn the_api_key_goes_in_a_header_not_the_body() {
    let headers = headers(&target());
    assert!(headers.contains(&("x-api-key", "sk-ant-test".to_string())));

    let body = build_body(&target(), &system("be helpful"), &[], &[]);
    assert!(!body.to_string().contains("sk-ant-test"));
}

#[test]
fn the_system_prompt_is_cached() {
    let body = build_body(
        &target(),
        &system("be helpful"),
        &[Turn::User("hi".to_string())],
        &[],
    );
    assert_eq!(
        body["system"],
        json!([{
            "type": "text",
            "text": "be helpful",
            "cache_control": { "type": "ephemeral" },
        }])
    );
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["stream"], true);

    let body = build_body(&target(), &system(""), &[Turn::User("hi".to_string())], &[]);
    assert!(body.get("system").is_none(), "{body}");
}

fn assistant_calling(id: &str) -> Turn {
    Turn::Assistant {
        text: "looking".to_string(),
        reasoning: String::new(),
        tool_calls: vec![ToolUse {
            id: id.to_string(),
            name: "grep".to_string(),
            arguments: json!({ "queries": ["fn main"] }),
        }],
    }
}

fn results_for(id: &str) -> Turn {
    Turn::ToolResults(vec![ToolResult {
        id: id.to_string(),
        content: "src/main.rs: lines 1".to_string(),
        is_error: false,
    }])
}

/// Where each `cache_control` sits, as `(message index, block index)`.
fn breakpoints(body: &Value) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for (message_index, message) in body["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .enumerate()
    {
        for (block_index, block) in message["content"]
            .as_array()
            .expect("content")
            .iter()
            .enumerate()
        {
            if block.get("cache_control").is_some() {
                found.push((message_index, block_index));
            }
        }
    }
    found
}

#[test]
fn the_two_newest_user_messages_are_cache_breakpoints() {
    let turns = vec![
        Turn::User("find main".to_string()),
        assistant_calling("toolu_1"),
        results_for("toolu_1"),
        assistant_calling("toolu_2"),
        results_for("toolu_2"),
    ];
    let body = build_body(&target(), &system("be helpful"), &turns, &[]);

    assert_eq!(breakpoints(&body), vec![(2, 0), (4, 0)]);
    // With the system prompt that makes three, under Anthropic's limit of four.
    assert!(body["system"][0].get("cache_control").is_some());
}

#[test]
fn a_single_user_message_is_the_only_message_breakpoint() {
    let body = build_body(&target(), &system(""), &[Turn::User("hi".to_string())], &[]);
    assert_eq!(breakpoints(&body), vec![(0, 0)]);
}

#[test]
fn current_state_follows_the_newest_user_message_uncached() {
    let prompt = Prompt {
        system: "be helpful".to_string(),
        current_state: "Current environment:\n- Working directory: /work\n".to_string(),
        ..Default::default()
    };

    let body = build_body(&target(), &prompt, &[Turn::User("hi".to_string())], &[]);
    let content = body["messages"][0]["content"].as_array().expect("content");
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["text"], "hi");
    assert!(content[0].get("cache_control").is_some());
    assert_eq!(content[1]["text"], prompt.current_state.as_str());
    assert!(content[1].get("cache_control").is_none());

    // After tool results it joins the same user message, behind the `tool_result` blocks.
    let turns = vec![
        Turn::User("find main".to_string()),
        assistant_calling("toolu_1"),
        results_for("toolu_1"),
    ];
    let body = build_body(&target(), &prompt, &turns, &[]);
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 3);
    let content = messages[2]["content"].as_array().expect("content");
    assert_eq!(content[0]["type"], "tool_result");
    assert!(content[0].get("cache_control").is_some());
    assert_eq!(content[1]["text"], prompt.current_state.as_str());
    assert!(content[1].get("cache_control").is_none());
}

#[test]
fn current_state_after_the_agent_is_a_new_user_message() {
    let prompt = Prompt {
        system: String::new(),
        current_state: "Current environment:\n".to_string(),
        ..Default::default()
    };
    let turns = vec![
        Turn::User("hi".to_string()),
        Turn::Assistant {
            text: "hello".to_string(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
        },
    ];
    let body = build_body(&target(), &prompt, &turns, &[]);
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["text"], "Current environment:\n");
}

#[test]
fn tool_results_go_into_a_user_turn() {
    let turns = vec![Turn::ToolResults(vec![ToolResult {
        id: "toolu_1".to_string(),
        content: "exit code: 0".to_string(),
        is_error: false,
    }])];

    let body = build_body(&target(), &system(""), &turns, &[]);
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

    let body = build_body(&target(), &system(""), &turns, &[]);
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
fn running_out_of_window_mid_reply_is_a_context_window_stop() {
    assert_eq!(
        parse_event(
            "message_delta",
            r#"{"delta":{"stop_reason":"model_context_window_exceeded"}}"#
        ),
        vec![Delta::Stop(StopReason::ContextWindowExceeded)]
    );
}

#[test]
fn usage_comes_from_message_start_and_message_delta() {
    let start = parse_event(
        "message_start",
        r#"{"message":{"usage":{"input_tokens":20,"cache_read_input_tokens":800,
            "cache_creation_input_tokens":100,"output_tokens":1}}}"#,
    );
    assert_eq!(
        start,
        vec![Delta::Usage(TokenUsage {
            input: 920,
            output: 1,
            cache_read: 800,
            cache_write: 100,
            reasoning: 0,
        })]
    );

    let delta = parse_event(
        "message_delta",
        r#"{"delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":57}}"#,
    );
    assert_eq!(
        delta,
        vec![
            Delta::Usage(TokenUsage {
                output: 57,
                ..Default::default()
            }),
            Delta::Stop(StopReason::EndTurn),
        ]
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

#[test]
fn empty_current_state_changes_nothing() {
    let turns = [Turn::User("hi".to_string())];
    let prompt = Prompt {
        system: "be helpful".to_string(),
        current_state: String::new(),
        ..Default::default()
    };
    let body = build_body(&target(), &prompt, &turns, &[]);
    assert_eq!(
        body["messages"][0]["content"].as_array().map(Vec::len),
        Some(1)
    );
}

#[test]
fn a_custom_endpoint_gets_no_cache_fields() {
    let custom = ProviderTarget {
        is_custom: true,
        ..target()
    };
    let turns = vec![
        Turn::User("find main".to_string()),
        assistant_calling("toolu_1"),
        results_for("toolu_1"),
    ];
    let body = build_body(&custom, &system("be helpful"), &turns, &[]);

    assert_eq!(body["system"], "be helpful");
    assert!(breakpoints(&body).is_empty(), "{body}");
    assert!(!body.to_string().contains("cache_control"));
}

#[test]
fn the_built_in_endpoint_gets_cache_fields() {
    let body = build_body(
        &target(),
        &system("be helpful"),
        &[Turn::User("hi".to_string())],
        &[],
    );

    assert!(body["system"].is_array());
    assert_eq!(breakpoints(&body), vec![(0, 0)]);
}

/// A request whose whole prompt came from the cache reports no uncached input at all.
#[test]
fn a_fully_cached_input_still_counts() {
    let start = parse_event(
        "message_start",
        r#"{"message":{"usage":{"input_tokens":0,"cache_read_input_tokens":900,
            "output_tokens":1}}}"#,
    );
    assert_eq!(
        start,
        vec![Delta::Usage(TokenUsage {
            input: 900,
            output: 1,
            cache_read: 900,
            cache_write: 0,
            reasoning: 0,
        })]
    );
}

#[test]
fn a_prompt_output_cap_lowers_max_tokens_but_never_raises_it() {
    let capped = Prompt {
        max_output_tokens: Some(8_000),
        ..Default::default()
    };
    assert_eq!(
        build_body(&target(), &capped, &[], &[])["max_tokens"],
        8_000
    );
    let above = Prompt {
        max_output_tokens: Some(100_000),
        ..Default::default()
    };
    assert_eq!(
        build_body(&target(), &above, &[], &[])["max_tokens"],
        32_000
    );
}
