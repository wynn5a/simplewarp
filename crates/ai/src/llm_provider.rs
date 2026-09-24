use serde::{Deserialize, Serialize};
use warp_core::ui::icons::Icon;

use crate::api_keys::ApiKeys;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LLMProvider {
    OpenAI,
    Anthropic,
    Google,
    Unknown,
}

impl LLMProvider {
    pub const API_KEY_PROVIDERS: [Self; 3] = [Self::OpenAI, Self::Anthropic, Self::Google];

    pub fn icon(self) -> Option<Icon> {
        match self {
            Self::OpenAI => Some(Icon::OpenAILogo),
            Self::Anthropic => Some(Icon::ClaudeLogo),
            Self::Google => Some(Icon::GeminiLogo),
            Self::Unknown => None,
        }
    }
    pub fn api_key_placeholder(self) -> Option<&'static str> {
        match self {
            Self::OpenAI => Some("sk-..."),
            Self::Anthropic => Some("sk-ant-..."),
            Self::Google => Some("AIzaSy..."),
            Self::Unknown => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::OpenAI => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Google => "Google",
            Self::Unknown => "this provider",
        }
    }

    pub fn api_key(self, keys: &ApiKeys) -> Option<&str> {
        match self {
            Self::OpenAI => keys.openai.as_deref(),
            Self::Anthropic => keys.anthropic.as_deref(),
            Self::Google => keys.google.as_deref(),
            Self::Unknown => None,
        }
    }

    pub(crate) fn set_api_key(self, keys: &mut ApiKeys, key: Option<String>) -> bool {
        match self {
            Self::OpenAI => keys.openai = key,
            Self::Anthropic => keys.anthropic = key,
            Self::Google => keys.google = key,
            Self::Unknown => return false,
        }
        true
    }
}

#[cfg(test)]
#[path = "llm_provider_tests.rs"]
mod tests;
