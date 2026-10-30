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
pub mod openai_responses;

/// One piece of a streamed reply, in a shape that every provider maps onto.
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
    /// Token counts for the reply. A provider may report them across several events.
    Usage(TokenUsage),
    /// The reply is over.
    Stop(StopReason),
    /// The provider reported a failure inside a stream that opened with a success status.
    Error { code: String, message: String },
}

/// Token counts that a provider reported for one reply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    /// All input tokens, cached ones included.
    pub input: u32,
    pub output: u32,
    pub cache_read: u32,
    pub cache_write: u32,
}

impl TokenUsage {
    /// Folds in a later report. Providers repeat or complete earlier counts in later events, and
    /// leave out (or zero) a count they are not reporting there, so a zero keeps the earlier value.
    pub fn merge(&mut self, later: TokenUsage) {
        for (field, later) in [
            (&mut self.input, later.input),
            (&mut self.output, later.output),
            (&mut self.cache_read, later.cache_read),
            (&mut self.cache_write, later.cache_write),
        ] {
            if later != 0 {
                *field = later;
            }
        }
    }
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
        Schema::OpenaiResponses => {
            openai_responses::build_body(target, system_prompt, turns, tools)
        }
    }
}

/// The headers that a provider needs, on top of `content-type`.
pub fn headers(target: &ProviderTarget) -> Vec<(&'static str, String)> {
    match target.schema {
        Schema::AnthropicMessages => anthropic::headers(target),
        Schema::OpenaiChatCompletions => openai::headers(target),
        Schema::OpenaiResponses => openai_responses::headers(target),
    }
}

/// Reads one server-sent event into zero or more deltas.
///
/// `event_name` is the SSE `event:` field, which Anthropic sets and OpenAI leaves empty.
pub fn parse_event(schema: Schema, event_name: &str, data: &str) -> Vec<Delta> {
    match schema {
        Schema::AnthropicMessages => anthropic::parse_event(event_name, data),
        Schema::OpenaiChatCompletions => openai::parse_event(data),
        Schema::OpenaiResponses => openai_responses::parse_event(data),
    }
}
