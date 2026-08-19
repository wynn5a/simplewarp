//! The Anthropic Messages API.
//!
//! Reference shape of a streaming reply:
//!
//! ```text
//! event: content_block_start
//! data: {"index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"grep"}}
//! event: content_block_delta
//! data: {"index":0,"delta":{"type":"input_json_delta","partial_json":"{\"queries\""}}
//! event: message_delta
//! data: {"delta":{"stop_reason":"tool_use"}}
//! ```

use serde_json::{Value, json};

use crate::config::ProviderTarget;
use crate::convert::Turn;
use crate::provider::{Delta, StopReason};
use crate::tools::ToolSchema;

/// The API version that this crate is written against.
pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The output-token ceiling for one reply. Anthropic makes this field required.
const MAX_OUTPUT_TOKENS: u32 = 32_000;

pub fn headers(target: &ProviderTarget) -> Vec<(&'static str, String)> {
    vec![
        ("x-api-key", target.api_key.clone()),
        ("anthropic-version", ANTHROPIC_VERSION.to_string()),
    ]
}

pub fn build_body(
    target: &ProviderTarget,
    system_prompt: &str,
    turns: &[Turn],
    tools: &[ToolSchema],
) -> Value {
    let messages = turns.iter().map(message_for).collect::<Vec<_>>();
    let tools = tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema,
            })
        })
        .collect::<Vec<_>>();

    let mut body = json!({
        "model": target.model,
        "max_tokens": MAX_OUTPUT_TOKENS,
        "stream": true,
        "system": system_prompt,
        "messages": messages,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    body
}

fn message_for(turn: &Turn) -> Value {
    match turn {
        Turn::User(text) => json!({
            "role": "user",
            "content": [{ "type": "text", "text": text }],
        }),
        // `reasoning` is deliberately left out. Anthropic carries thinking in a `thinking` block
        // that is signed, and it validates that signature when the block is replayed. This crate
        // keeps only the text, so it has nothing valid to send back, and Anthropic does not ask
        // for one.
        Turn::Assistant {
            text, tool_calls, ..
        } => {
            let mut content = Vec::new();
            if !text.is_empty() {
                content.push(json!({ "type": "text", "text": text }));
            }
            for call in tool_calls {
                content.push(json!({
                    "type": "tool_use",
                    "id": call.id,
                    "name": call.name,
                    "input": call.arguments,
                }));
            }
            json!({ "role": "assistant", "content": content })
        }
        // Anthropic carries tool results in a user turn.
        Turn::ToolResults(results) => {
            let content = results
                .iter()
                .map(|result| {
                    json!({
                        "type": "tool_result",
                        "tool_use_id": result.id,
                        "content": result.content,
                        "is_error": result.is_error,
                    })
                })
                .collect::<Vec<_>>();
            json!({ "role": "user", "content": content })
        }
    }
}

pub fn parse_event(event_name: &str, data: &str) -> Vec<Delta> {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };

    match event_name {
        "content_block_start" => {
            let Some(index) = index_of(&value) else {
                return Vec::new();
            };
            let block = &value["content_block"];
            if block["type"] != "tool_use" {
                return Vec::new();
            }
            let (Some(id), Some(name)) = (block["id"].as_str(), block["name"].as_str()) else {
                return Vec::new();
            };
            vec![Delta::ToolCallStart {
                index,
                id: id.to_string(),
                name: name.to_string(),
            }]
        }
        "content_block_delta" => {
            let Some(index) = index_of(&value) else {
                return Vec::new();
            };
            let delta = &value["delta"];
            match delta["type"].as_str() {
                Some("text_delta") => text_of(delta, "text")
                    .map(Delta::Text)
                    .into_iter()
                    .collect(),
                Some("thinking_delta") => text_of(delta, "thinking")
                    .map(Delta::Reasoning)
                    .into_iter()
                    .collect(),
                Some("input_json_delta") => text_of(delta, "partial_json")
                    .map(|fragment| Delta::ToolCallArguments { index, fragment })
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
            }
        }
        "message_delta" => match value["delta"]["stop_reason"].as_str() {
            Some(reason) => vec![Delta::Stop(stop_reason(reason))],
            None => Vec::new(),
        },
        // `message_stop` closes the stream, but `message_delta` already carried the reason.
        // Emitting a second stop here would end the reply twice.
        _ => Vec::new(),
    }
}

fn index_of(value: &Value) -> Option<usize> {
    value["index"].as_u64().map(|index| index as usize)
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_string)
}

fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "end_turn" | "stop_sequence" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        _ => StopReason::Other,
    }
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
