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
use crate::prompt::Prompt;
use crate::provider::{Delta, StopReason, TokenUsage};
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

/// Builds the request body, with prompt-cache breakpoints.
///
/// Anthropic caches a request prefix only up to a block marked with `cache_control`, at most four
/// per request, and reads it back when a later request repeats that prefix exactly. The cache
/// covers tools, then the system prompt, then the messages, so three marks are set:
///
/// - the system prompt, which caches the tools with it;
/// - the last block of the newest user-role message, before the current state: what the next
///   request will repeat;
/// - the last block of the user-role message before that, which is where the previous request put
///   its mark. A mark also finds a cached prefix up to 20 blocks back, but a turn with many
///   parallel tool calls can be longer than that.
///
/// A prefix shorter than the model's minimum is not cached, and that is not an error. A custom
/// endpoint gets no marks and a plain-string system prompt.
pub fn build_body(
    target: &ProviderTarget,
    prompt: &Prompt,
    turns: &[Turn],
    tools: &[ToolSchema],
) -> Value {
    // A custom endpoint that speaks this schema is often a proxy or another vendor's server,
    // which may reject `cache_control` or a system prompt given as blocks, so it gets neither.
    let caches = !target.is_custom;
    let mut messages = turns.iter().map(message_for).collect::<Vec<_>>();
    if caches {
        mark_cache_breakpoints(&mut messages);
    }
    push_current_state(&mut messages, &prompt.current_state);

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
        "messages": messages,
    });
    if !caches {
        body["system"] = Value::String(prompt.system.clone());
    } else if !prompt.system.is_empty() {
        // An empty text block is rejected, so an empty system prompt is left out.
        body["system"] = json!([{
            "type": "text",
            "text": prompt.system,
            "cache_control": cache_control(),
        }]);
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    body
}

fn cache_control() -> Value {
    json!({ "type": "ephemeral" })
}

/// Marks the last block of the two newest user-role messages. See [`build_body`].
fn mark_cache_breakpoints(messages: &mut [Value]) {
    for message in messages
        .iter_mut()
        .rev()
        .filter(|message| message["role"] == "user")
        .take(2)
    {
        if let Some(block) = message["content"]
            .as_array_mut()
            .and_then(|content| content.last_mut())
        {
            block["cache_control"] = cache_control();
        }
    }
}

/// Adds the current state as its own text block at the end of the last user-role message, or as
/// a new user message when the conversation ends with the agent. Its own block keeps every block
/// before it, and so the cached prefix, the same as in the next request.
fn push_current_state(messages: &mut Vec<Value>, current_state: &str) {
    if current_state.is_empty() {
        return;
    }
    let block = json!({ "type": "text", "text": current_state });
    match messages.last_mut() {
        Some(last) if last["role"] == "user" => {
            if let Some(content) = last["content"].as_array_mut() {
                content.push(block);
            }
        }
        _ => messages.push(json!({ "role": "user", "content": [block] })),
    }
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
        "message_start" => usage(&value["message"]["usage"])
            .map(Delta::Usage)
            .into_iter()
            .collect(),
        "message_delta" => {
            // `message_delta` carries the final output count; the input counts came with
            // `message_start`.
            let mut deltas: Vec<Delta> = usage(&value["usage"])
                .map(Delta::Usage)
                .into_iter()
                .collect();
            if let Some(reason) = value["delta"]["stop_reason"].as_str() {
                deltas.push(Delta::Stop(stop_reason(reason)));
            }
            deltas
        }
        // `message_stop` closes the stream, but `message_delta` already carried the reason.
        // Emitting a second stop here would end the reply twice.
        _ => Vec::new(),
    }
}

/// Anthropic counts cached input apart from `input_tokens`, so the total adds them back.
fn usage(usage: &Value) -> Option<TokenUsage> {
    if !usage.is_object() {
        return None;
    }
    let count = |key: &str| usage[key].as_u64().unwrap_or(0) as u32;
    let cache_read = count("cache_read_input_tokens");
    let cache_write = count("cache_creation_input_tokens");
    // A report that carries only the output count sums to zero here, which `TokenUsage::merge`
    // reads as no input count rather than as zero input.
    let input = count("input_tokens") + cache_read + cache_write;
    Some(TokenUsage {
        input,
        output: count("output_tokens"),
        cache_read,
        cache_write,
    })
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
