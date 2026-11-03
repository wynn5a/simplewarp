//! Asks a provider which models the user's key can reach.
//!
//! A hardcoded model list would be wrong within months: provider slugs change often, and a
//! wrong slug fails at request time with a 404 the user cannot act on. Every provider here
//! answers `GET /models` with the key the user already gave us, so the catalog is whatever that
//! key can actually reach today.
//!
//! All four providers return the same envelope — `{"data": [{"id": "..."}]}` — so one parser
//! serves them; only the auth header and a Google-specific id prefix differ.

use serde_json::Value;

use crate::Error;
use crate::config::{ANTHROPIC_BASE_URL, GOOGLE_OPENAI_BASE_URL, OPENAI_BASE_URL, Schema};

/// A first-party provider the user can hold a key for.
///
/// Aggregators such as OpenRouter are deliberately absent. They are reached as a custom
/// endpoint, where the user gives the base URL and the exact model slug, because an aggregator
/// lists hundreds of models from other vendors and none of them is "this provider's official
/// API" in the sense the rest of this enum means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAI,
    Google,
}

impl Provider {
    /// The label shown next to the model in the picker.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::OpenAI => "OpenAI",
            Self::Google => "Google",
        }
    }

    /// The protocol this provider's inference endpoint speaks.
    pub fn schema(self) -> Schema {
        match self {
            Self::Anthropic => Schema::AnthropicMessages,
            // Google is reached through its OpenAI-compatible surface.
            Self::OpenAI | Self::Google => Schema::OpenaiChatCompletions,
        }
    }

    fn base_url(self) -> &'static str {
        match self {
            Self::Anthropic => ANTHROPIC_BASE_URL,
            Self::OpenAI => OPENAI_BASE_URL,
            Self::Google => GOOGLE_OPENAI_BASE_URL,
        }
    }
}

/// One model a provider offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModel {
    pub provider: Provider,
    /// The slug to send as the model, for example `claude-sonnet-4-5-20250929`.
    pub id: String,
    /// A friendlier name when the provider gives one, else the slug.
    pub display_name: String,
    /// Input tokens the model accepts: the smaller of what the provider reported and
    /// [`context_window`], or whichever of the two is known.
    pub context_window: Option<u32>,
}

/// Lists the models that `api_key` can reach at `provider`.
///
/// The only host contacted is the provider's own.
pub async fn list_models(provider: Provider, api_key: &str) -> Result<Vec<ProviderModel>, Error> {
    if api_key.is_empty() {
        return Ok(Vec::new());
    }

    let url = format!("{}/models", provider.base_url());
    let mut request = reqwest::Client::new().get(url);
    request = match provider {
        Provider::Anthropic => request.header("x-api-key", api_key).header(
            "anthropic-version",
            crate::provider::anthropic::ANTHROPIC_VERSION,
        ),
        _ => request.header("authorization", format!("Bearer {api_key}")),
    };

    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(Error::ProviderStatus {
            status: status.as_u16(),
            body,
        });
    }

    parse_models(provider, &body)
}

/// Reads the `{"data": [{"id": ...}]}` envelope that all four providers return.
pub fn parse_models(provider: Provider, body: &str) -> Result<Vec<ProviderModel>, Error> {
    let value: Value = serde_json::from_str(body)?;
    let Some(entries) = value["data"].as_array() else {
        return Ok(Vec::new());
    };

    Ok(entries
        .iter()
        .filter_map(|entry| {
            let raw_id = entry["id"].as_str()?;
            let id = normalize_id(provider, raw_id);
            if id.is_empty() {
                return None;
            }
            // Anthropic uses `display_name`; OpenAI and Google give neither.
            let display_name = entry["display_name"]
                .as_str()
                .or(entry["name"].as_str())
                .unwrap_or(&id)
                .to_string();
            // Anthropic reports `max_input_tokens`; OpenAI and Google report no window. The listing
            // can give a window that only a beta header unlocks, which this crate never sends, so
            // the table caps it.
            let listed = entry["max_input_tokens"]
                .as_u64()
                .and_then(|tokens| u32::try_from(tokens).ok())
                .filter(|tokens| *tokens > 0);
            let window = match (listed, context_window(&id)) {
                (Some(listed), Some(known)) => Some(listed.min(known)),
                (listed, known) => listed.or(known),
            };
            Some(ProviderModel {
                provider,
                id,
                display_name,
                context_window: window,
            })
        })
        .collect())
}

/// Google's OpenAI-compatible surface returns ids as `models/gemini-...`, but the inference
/// endpoint wants the bare slug.
fn normalize_id(provider: Provider, raw_id: &str) -> String {
    match provider {
        Provider::Google => raw_id.strip_prefix("models/").unwrap_or(raw_id).to_string(),
        _ => raw_id.to_string(),
    }
}

/// The input-token window of a first-party model, by slug. `None` for a slug this table does not
/// know.
///
/// Families are matched by prefix, first match wins, so a dated snapshot
/// (`claude-sonnet-4-5-20250929`) takes its family's value. Where a family's window depends on a
/// beta header or a newer point release this table cannot see, the value is the one that holds
/// without it. OpenAI's GPT-5 family accepts 400k tokens in total, of which 128k are reserved for
/// output, so its input window is 272k; its ChatGPT snapshots (`gpt-5-chat-latest`,
/// `gpt-5.1-chat-latest`) take 128k.
pub fn context_window(model_id: &str) -> Option<u32> {
    const K: u32 = 1_000;
    const ONE_MILLION_INPUT: u32 = 1_000_000;
    // Gemini counts its "1M" window in binary.
    const GOOGLE_MILLION: u32 = 1_048_576;
    const GPT_4_1: u32 = 1_047_576;
    const WINDOWS: &[(&str, u32)] = &[
        // Anthropic. The 4.6 generation onward has 1M by default; Haiku 4.5 and everything before
        // 4.6 has 200k (1M on Sonnet 4/4.5 needs a beta header this crate does not send).
        ("claude-haiku-4-5", 200 * K),
        ("claude-opus-4-6", ONE_MILLION_INPUT),
        ("claude-opus-4-7", ONE_MILLION_INPUT),
        ("claude-opus-4-8", ONE_MILLION_INPUT),
        ("claude-sonnet-4-6", ONE_MILLION_INPUT),
        ("claude-opus-5", ONE_MILLION_INPUT),
        ("claude-sonnet-5", ONE_MILLION_INPUT),
        ("claude-haiku-5", ONE_MILLION_INPUT),
        ("claude-fable-5", ONE_MILLION_INPUT),
        ("claude-mythos-5", ONE_MILLION_INPUT),
        ("claude-", 200 * K),
        // OpenAI.
        ("gpt-5", 272 * K),
        ("gpt-4.1", GPT_4_1),
        ("gpt-4o", 128 * K),
        ("gpt-4-turbo", 128 * K),
        ("o1-mini", 128 * K),
        ("o1-preview", 128 * K),
        ("o1", 200 * K),
        ("o3", 200 * K),
        ("o4-mini", 200 * K),
        // Google.
        ("gemini-1.5-flash", GOOGLE_MILLION),
        ("gemini-1.5-pro", GOOGLE_MILLION),
        ("gemini-2", GOOGLE_MILLION),
        ("gemini-3", GOOGLE_MILLION),
    ];
    if model_id.starts_with("gpt-5") && model_id.contains("-chat") {
        return Some(128 * K);
    }
    WINDOWS
        .iter()
        .find(|(prefix, _)| model_id.starts_with(prefix))
        .map(|(_, window)| *window)
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
