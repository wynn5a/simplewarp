//! The provider layer: build a request body, and read the streamed reply.
//!
//! Each provider module turns the neutral [`Turn`](crate::convert::Turn) list into that
//! provider's own JSON, and turns that provider's server-sent events back into [`Delta`]s. The
//! rest of the crate never sees a provider-specific shape.

use serde_json::Value;

use crate::config::{ProviderTarget, Schema};
use crate::convert::Turn;
use crate::tools::ToolSchema;

pub mod anthropic;
pub mod openai;

/// One piece of a streamed reply, in a shape that both providers map onto.
#[derive(Debug, Clone, PartialEq)]
pub enum Delta {
    /// Text for the user.
    Text(String),
    /// Reasoning tokens, when the model exposes them.
    Reasoning(String),
    /// A tool call has started. `index` orders the calls inside one reply.
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    /// The next piece of a tool call's JSON arguments, for the call at `index`.
    ToolCallArguments { index: usize, fragment: String },
    /// The reply is over.
    Stop(StopReason),
}

/// Why the model stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model finished its answer.
    EndTurn,
    /// The model wants the client to run the tools it just called.
    ToolUse,
    /// The model hit its output limit.
    MaxTokens,
    /// Anything else, including a reason that this crate does not know.
    Other,
}

/// Builds the JSON body for a request.
pub fn build_body(
    target: &ProviderTarget,
    system_prompt: &str,
    turns: &[Turn],
    tools: &[ToolSchema],
) -> Value {
    match target.schema {
        Schema::AnthropicMessages => anthropic::build_body(target, system_prompt, turns, tools),
        Schema::OpenaiChatCompletions => openai::build_body(target, system_prompt, turns, tools),
    }
}

/// The headers that a provider needs, on top of `content-type`.
pub fn headers(target: &ProviderTarget) -> Vec<(&'static str, String)> {
    match target.schema {
        Schema::AnthropicMessages => anthropic::headers(target),
        Schema::OpenaiChatCompletions => openai::headers(target),
    }
}

/// Reads one server-sent event into zero or more deltas.
///
/// `event_name` is the SSE `event:` field, which Anthropic sets and OpenAI leaves empty.
pub fn parse_event(schema: Schema, event_name: &str, data: &str) -> Vec<Delta> {
    match schema {
        Schema::AnthropicMessages => anthropic::parse_event(event_name, data),
        Schema::OpenaiChatCompletions => openai::parse_event(data),
    }
}
