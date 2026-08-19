//! The OpenAI Chat Completions API.
//!
//! This schema also covers OpenRouter, Google's OpenAI-compatible endpoint, and local servers
//! such as Ollama, LM Studio, and vLLM, so it is the fallback for any custom endpoint.
//!
//! Reference shape of a streaming reply:
//!
//! ```text
//! data: {"choices":[{"delta":{"content":"Hello"}}]}
//! data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
//!        "function":{"name":"grep","arguments":"{\"queries\""}}]}}]}
//! data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}
//! data: [DONE]
//! ```

use serde_json::{Value, json};

use crate::config::ProviderTarget;
use crate::convert::Turn;
use crate::provider::{Delta, StopReason};
use crate::tools::ToolSchema;

pub fn headers(target: &ProviderTarget) -> Vec<(&'static str, String)> {
    vec![("authorization", format!("Bearer {}", target.api_key))]
}

pub fn build_body(
    target: &ProviderTarget,
    system_prompt: &str,
    turns: &[Turn],
    tools: &[ToolSchema],
) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": system_prompt })];
    for turn in turns {
        push_messages(&mut messages, turn);
    }

    let tools = tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                },
            })
        })
        .collect::<Vec<_>>();

    let mut body = json!({
        "model": target.model,
        "stream": true,
        "messages": messages,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    body
}

fn push_messages(messages: &mut Vec<Value>, turn: &Turn) {
    match turn {
        Turn::User(text) => messages.push(json!({ "role": "user", "content": text })),
        Turn::Assistant { text, tool_calls } => {
            let mut message = json!({ "role": "assistant" });
            // The API rejects an assistant message with neither content nor tool calls, so an
            // empty text still goes out as an explicit null when tool calls are present.
            message["content"] = if text.is_empty() {
                Value::Null
            } else {
                Value::String(text.clone())
            };
            if !tool_calls.is_empty() {
                message["tool_calls"] = Value::Array(
                    tool_calls
                        .iter()
                        .map(|call| {
                            json!({
                                "id": call.id,
                                "type": "function",
                                "function": {
                                    "name": call.name,
                                    // The API takes the arguments as a JSON string, not an object.
                                    "arguments": call.arguments.to_string(),
                                },
                            })
                        })
                        .collect(),
                );
            }
            messages.push(message);
        }
        // Each tool result is its own message, unlike Anthropic, which groups them.
        Turn::ToolResults(results) => {
            for result in results {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": result.id,
                    "content": result.content,
                }));
            }
        }
    }
}

pub fn parse_event(data: &str) -> Vec<Delta> {
    if data.trim() == "[DONE]" {
        // The reply already ended with a `finish_reason`. A second stop here would end it twice.
        return Vec::new();
    }
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };
    let Some(choice) = value["choices"]
        .as_array()
        .and_then(|choices| choices.first())
    else {
        return Vec::new();
    };

    let mut deltas = Vec::new();
    let delta = &choice["delta"];

    if let Some(text) = delta["content"].as_str()
        && !text.is_empty()
    {
        deltas.push(Delta::Text(text.to_string()));
    }

    // Not part of the OpenAI schema, but OpenRouter and several local servers put reasoning here.
    if let Some(text) = delta["reasoning"]
        .as_str()
        .or(delta["reasoning_content"].as_str())
        && !text.is_empty()
    {
        deltas.push(Delta::Reasoning(text.to_string()));
    }

    if let Some(calls) = delta["tool_calls"].as_array() {
        for call in calls {
            let index = call["index"].as_u64().unwrap_or(0) as usize;
            let function = &call["function"];
            // A call opens with an id and a name, then streams its arguments in later events
            // that carry the index alone.
            if let (Some(id), Some(name)) = (call["id"].as_str(), function["name"].as_str()) {
                deltas.push(Delta::ToolCallStart {
                    index,
                    id: id.to_string(),
                    name: name.to_string(),
                });
            }
            if let Some(fragment) = function["arguments"].as_str()
                && !fragment.is_empty()
            {
                deltas.push(Delta::ToolCallArguments {
                    index,
                    fragment: fragment.to_string(),
                });
            }
        }
    }

    if let Some(reason) = choice["finish_reason"].as_str() {
        deltas.push(Delta::Stop(stop_reason(reason)));
    }

    deltas
}

fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        _ => StopReason::Other,
    }
}

#[cfg(test)]
#[path = "openai_tests.rs"]
mod tests;
