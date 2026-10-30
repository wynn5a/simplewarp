//! The OpenAI Responses API.
//!
//! Every request is stateless (`store: false`) and carries the whole conversation, like the other
//! two schemas, so the reply never depends on a response that OpenAI kept.
//!
//! Reference shape of a streaming reply:
//!
//! ```text
//! event: response.output_text.delta
//! data: {"type":"response.output_text.delta","output_index":0,"delta":"Hello"}
//! event: response.output_item.added
//! data: {"type":"response.output_item.added","output_index":1,
//!        "item":{"type":"function_call","call_id":"call_1","name":"grep","arguments":""}}
//! event: response.function_call_arguments.delta
//! data: {"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"q"}
//! event: response.completed
//! data: {"type":"response.completed","response":{"status":"completed",
//!        "output":[...],"usage":{"input_tokens":812,"output_tokens":40}}}
//! ```

use serde_json::{Value, json};

use crate::config::ProviderTarget;
use crate::convert::Turn;
use crate::provider::{Delta, StopReason, TokenUsage};
use crate::tools::ToolSchema;

pub fn headers(target: &ProviderTarget) -> Vec<(&'static str, String)> {
    vec![("authorization", format!("Bearer {}", target.api_key))]
}

/// The system prompt goes in `instructions`: the ChatGPT-plan endpoint rejects a system-role
/// input item. `temperature` and `max_output_tokens` are left out for the same reason.
pub fn build_body(
    target: &ProviderTarget,
    system_prompt: &str,
    turns: &[Turn],
    tools: &[ToolSchema],
) -> Value {
    let mut input = Vec::new();
    for turn in turns {
        push_items(&mut input, turn);
    }

    let tools = tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.input_schema,
            })
        })
        .collect::<Vec<_>>();

    let mut body = json!({
        "model": target.model,
        "stream": true,
        "store": false,
        "instructions": system_prompt,
        "input": input,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    body
}

fn push_items(input: &mut Vec<Value>, turn: &Turn) {
    match turn {
        Turn::User(text) => input.push(message_item("user", text)),
        // `reasoning` is left out. Replaying it would take the encrypted reasoning item, which
        // the conversation does not keep.
        Turn::Assistant {
            text, tool_calls, ..
        } => {
            if !text.is_empty() {
                input.push(message_item("assistant", text));
            }
            for call in tool_calls {
                input.push(json!({
                    "type": "function_call",
                    "call_id": call.id,
                    "name": call.name,
                    // The API takes the arguments as a JSON string, not an object.
                    "arguments": call.arguments.to_string(),
                }));
            }
        }
        Turn::ToolResults(results) => {
            for result in results {
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": result.id,
                    "output": result.content,
                }));
            }
        }
    }
}

/// `type` is optional on OpenAI's own endpoint, but some Responses-compatible servers require it.
fn message_item(role: &str, text: &str) -> Value {
    json!({ "type": "message", "role": role, "content": text })
}

pub fn parse_event(data: &str) -> Vec<Delta> {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };

    match value["type"].as_str().unwrap_or_default() {
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            text_delta(&value)
                .map(Delta::Reasoning)
                .into_iter()
                .collect()
        }
        "response.output_text.delta" | "response.refusal.delta" => {
            text_delta(&value).map(Delta::Text).into_iter().collect()
        }
        // Summary parts arrive back to back, each opening with its own bold title.
        "response.reasoning_summary_part.added"
            if value["summary_index"]
                .as_u64()
                .is_some_and(|index| index > 0) =>
        {
            vec![Delta::Reasoning("\n\n".to_string())]
        }
        // The finished item carries the whole arguments, which some Responses-compatible servers
        // never stream as deltas, so it restarts the call and replaces any fragments that arrived.
        "response.output_item.added" | "response.output_item.done" => function_call(&value),
        "response.function_call_arguments.delta" => {
            match (output_index(&value), text_delta(&value)) {
                (Some(index), Some(fragment)) => vec![Delta::ToolCallArguments { index, fragment }],
                _ => Vec::new(),
            }
        }
        "response.completed" | "response.incomplete" => {
            let response = &value["response"];
            let mut deltas: Vec<Delta> = usage(&response["usage"])
                .map(Delta::Usage)
                .into_iter()
                .collect();
            deltas.push(Delta::Stop(stop_reason(response)));
            deltas
        }
        "response.failed" => vec![error(&value["response"]["error"])],
        // OpenAI puts the fields on the event; the ChatGPT backend nests them under `error`.
        "error" if value["error"].is_object() => vec![error(&value["error"])],
        "error" => vec![error(&value)],
        _ => Vec::new(),
    }
}

fn function_call(value: &Value) -> Vec<Delta> {
    let item = &value["item"];
    if item["type"] != "function_call" {
        return Vec::new();
    }
    let (Some(index), Some(id), Some(name)) = (
        output_index(value),
        item["call_id"].as_str(),
        item["name"].as_str(),
    ) else {
        return Vec::new();
    };
    let mut deltas = vec![Delta::ToolCallStart {
        index,
        // The `call_id`, not the item `id`: a `function_call_output` answers the call by its
        // `call_id`.
        id: id.to_string(),
        name: name.to_string(),
    }];
    if let Some(arguments) = item["arguments"].as_str()
        && !arguments.is_empty()
    {
        deltas.push(Delta::ToolCallArguments {
            index,
            fragment: arguments.to_string(),
        });
    }
    deltas
}

fn error(error: &Value) -> Delta {
    let code = error["code"]
        .as_str()
        .or(error["type"].as_str())
        .unwrap_or_default();
    let message = error["message"]
        .as_str()
        .filter(|message| !message.is_empty())
        .unwrap_or("the response failed without an error message");
    Delta::Error {
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn text_delta(value: &Value) -> Option<String> {
    value["delta"]
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn output_index(value: &Value) -> Option<usize> {
    value["output_index"].as_u64().map(|index| index as usize)
}

fn usage(usage: &Value) -> Option<TokenUsage> {
    if !usage.is_object() {
        return None;
    }
    let count = |value: &Value| value.as_u64().unwrap_or(0) as u32;
    Some(TokenUsage {
        input: count(&usage["input_tokens"]),
        output: count(&usage["output_tokens"]),
        cache_read: count(&usage["input_tokens_details"]["cached_tokens"]),
        cache_write: 0,
    })
}

/// Responses has no `finish_reason`, so the reason comes from the status and the output items.
fn stop_reason(response: &Value) -> StopReason {
    match response["status"].as_str() {
        Some("completed") => {
            let called_a_tool = response["output"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["type"] == "function_call"));
            if called_a_tool {
                StopReason::ToolUse
            } else {
                StopReason::EndTurn
            }
        }
        Some("incomplete") if response["incomplete_details"]["reason"] == "max_output_tokens" => {
            StopReason::MaxTokens
        }
        _ => StopReason::Other,
    }
}

#[cfg(test)]
#[path = "openai_responses_tests.rs"]
mod tests;
