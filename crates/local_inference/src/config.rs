//! Works out which provider endpoint to call, from the settings in the request.
//!
//! Nothing here contacts a Warp server. Every field comes from the request that the client
//! already built, so the keys stay on the machine.

use warp_multi_agent_api as api;

use crate::Error;

/// The request/response protocol that an endpoint speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    /// Anthropic Messages (`POST /v1/messages`).
    AnthropicMessages,
    /// OpenAI Chat Completions (`POST /chat/completions`).
    OpenaiChatCompletions,
}

/// A fully resolved endpoint: where to send the request, and how to speak to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTarget {
    pub schema: Schema,
    /// Base URL with no trailing slash, for example `https://api.anthropic.com/v1`.
    pub base_url: String,
    pub api_key: String,
    /// The model slug that the provider expects, for example `claude-sonnet-4-20250514`.
    pub model: String,
    /// True when the user named this endpoint themselves, rather than it being a first-party
    /// provider that this crate knows.
    ///
    /// A custom endpoint can be any OpenAI-compatible server, so the provider modules may send it
    /// fields that are outside the official schema. A first-party endpoint gets the official
    /// schema only.
    pub is_custom: bool,
}

impl ProviderTarget {
    /// The full URL of the chat/messages endpoint.
    pub fn endpoint_url(&self) -> String {
        match self.schema {
            Schema::AnthropicMessages => format!("{}/messages", self.base_url),
            Schema::OpenaiChatCompletions => format!("{}/chat/completions", self.base_url),
        }
    }
}

pub(crate) const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";
pub(crate) const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub(crate) const GOOGLE_OPENAI_BASE_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/openai";

/// Resolves the endpoint for the base model of a request.
///
/// The order is:
/// 1. A custom endpoint that declares this model. This covers local servers such as Ollama,
///    LM Studio, and vLLM, and any OpenAI-compatible or Anthropic-compatible router.
/// 2. A first-party provider, found from the shape of the model slug.
pub fn resolve_target(request: &api::Request) -> Result<ProviderTarget, Error> {
    let settings = request.settings.as_ref().ok_or(Error::NoSettings)?;
    let model = settings
        .model_config
        .as_ref()
        .map(|config| config.base.as_str())
        .filter(|base| !base.is_empty())
        .ok_or(Error::NoModel)?;

    if let Some(target) = custom_target(settings, model) {
        return Ok(target);
    }
    // A router id is not a model: it was Warp's server that chose the model behind it. Reaching
    // this point means nothing local is configured, so say that rather than report a missing key
    // for a model name the user never picked.
    if is_warp_router(model) {
        return Err(Error::NoModelConfigured);
    }
    builtin_target(settings, model)
}

/// The model ids that stood for "let Warp's server choose". They name no real model, so this
/// crate can never route one.
fn is_warp_router(model: &str) -> bool {
    matches!(model, "auto" | "cli-agent-auto" | "computer-use-agent-auto")
}

/// Looks for a custom endpoint that declares `model`, either by its `config_key` or by its
/// `slug`. The client keys custom models by `config_key`, so that comes first.
fn custom_target(settings: &api::request::Settings, model: &str) -> Option<ProviderTarget> {
    let providers = settings.custom_model_providers.as_ref()?;
    for provider in &providers.providers {
        let Some(matched) = provider
            .models
            .iter()
            .find(|candidate| candidate.config_key == model || candidate.slug == model)
        else {
            continue;
        };

        let schema = match provider.schema() {
            api::request::settings::custom_model_providers::CustomEndpointSchema::AnthropicMessages => {
                Schema::AnthropicMessages
            }
            // OpenAI Responses is not supported yet, so fall back to Chat Completions, which
            // every OpenAI-compatible server speaks.
            _ => Schema::OpenaiChatCompletions,
        };

        return Some(ProviderTarget {
            schema,
            base_url: trim_base_url(&provider.base_url),
            api_key: provider.api_key.clone(),
            model: matched.slug.clone(),
            is_custom: true,
        });
    }
    None
}

/// Picks a first-party provider from the shape of the model slug.
fn builtin_target(settings: &api::request::Settings, model: &str) -> Result<ProviderTarget, Error> {
    let keys = settings.api_keys.as_ref();
    let key_for = |key: Option<&String>| key.map(String::as_str).unwrap_or_default().to_string();

    // A slug with a slash names a vendor inside an aggregator, such as
    // `anthropic/claude-sonnet-4` on OpenRouter. This crate routes to first-party providers
    // only, so there is no base URL to send it to. Guessing one would send the user's key to a
    // host they never named.
    if model.contains('/') {
        return Err(Error::AggregatorModel {
            model: model.to_string(),
        });
    }

    let (schema, base_url, api_key) = if model.starts_with("claude") {
        (
            Schema::AnthropicMessages,
            ANTHROPIC_BASE_URL,
            key_for(keys.map(|keys| &keys.anthropic)),
        )
    } else if model.starts_with("gemini") {
        (
            Schema::OpenaiChatCompletions,
            GOOGLE_OPENAI_BASE_URL,
            key_for(keys.map(|keys| &keys.google)),
        )
    } else {
        // `gpt-*`, `o1`, `o3`, and anything else default to OpenAI.
        (
            Schema::OpenaiChatCompletions,
            OPENAI_BASE_URL,
            key_for(keys.map(|keys| &keys.openai)),
        )
    };

    if api_key.is_empty() {
        return Err(Error::NoApiKey {
            model: model.to_string(),
        });
    }

    Ok(ProviderTarget {
        schema,
        base_url: base_url.to_string(),
        api_key,
        model: model.to_string(),
        is_custom: false,
    })
}

/// Removes a trailing slash so that `endpoint_url` never builds a double slash.
fn trim_base_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
