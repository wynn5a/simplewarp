//! Recognizes a provider telling us the request did not fit in the model's context window.
//!
//! Providers report it as an ordinary 400, or as an error event inside a stream, with nothing but
//! the error's code and wording to tell it from any other bad request. A wrong match turns a fixable
//! request error into a "context window exceeded" one, so a body is matched only on a code or a
//! phrase that the providers use for this failure alone.

use serde_json::Value;

/// The code OpenAI (Chat Completions and Responses) gives a request that is too long.
const CONTEXT_LENGTH_EXCEEDED: &str = "context_length_exceeded";

/// True when an HTTP error response says the request exceeded the context window.
pub fn is_overflow_response(status: u16, body: &str) -> bool {
    status == 400
        && error_entries(body)
            .iter()
            .any(|(code, message)| is_overflow_error(code, message))
}

/// True when an error reported inside a stream says the request exceeded the context window.
pub fn is_overflow_error(code: &str, message: &str) -> bool {
    code == CONTEXT_LENGTH_EXCEEDED || is_overflow_message(message)
}

fn is_overflow_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    // Anthropic: "prompt is too long: 215000 tokens > 200000 maximum".
    message.starts_with("prompt is too long")
        // Anthropic, older models: "input length and `max_tokens` exceed context limit: 188240 +
        // 21333 > 200000, decrease input length or `max_tokens` and try again".
        || message.contains("exceed context limit")
        // OpenAI Chat Completions and OpenAI-compatible servers such as vLLM: "This model's
        // maximum context length is 128000 tokens. However, your messages resulted in ...".
        || message.contains("maximum context length is")
        // OpenAI Responses: "Your input exceeds the context window of this model."
        || message.contains("exceeds the context window")
        // Gemini: "The input token count (1234567) exceeds the maximum number of tokens allowed
        // (1048576)."
        || (message.contains("input token count") && message.contains("exceeds the maximum"))
}

/// The `(code, message)` of every error object in a body.
///
/// Providers nest the error differently: `{"error": {...}}` (OpenAI, Anthropic, Gemini), a bare
/// `{"code", "message"}` object (vLLM and other compatible servers), or a one-element array of
/// either (Gemini's OpenAI-compatible surface). A code may be a string or a number.
fn error_entries(body: &str) -> Vec<(String, String)> {
    let Ok(value) = serde_json::from_str::<Value>(body.trim()) else {
        return Vec::new();
    };
    let objects = match value {
        Value::Array(items) => items,
        other => vec![other],
    };
    objects
        .iter()
        .flat_map(|object| [&object["error"], object])
        .filter(|error| error.is_object())
        .map(|error| {
            let code = match &error["code"] {
                Value::String(code) => code.clone(),
                _ => String::new(),
            };
            let message = error["message"].as_str().unwrap_or_default().to_string();
            (code, message)
        })
        .collect()
}

#[cfg(test)]
#[path = "overflow_tests.rs"]
mod tests;
