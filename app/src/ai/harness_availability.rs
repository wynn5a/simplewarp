use std::collections::HashMap;
use std::time::Duration;

use ai::agent::AgentHarness;
use instant::Instant;
use serde::{Deserialize, Serialize};
use warp_cli::agent::Harness;
use warp_core::features::FeatureFlag;
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::ai::harness_display;

const AUTH_SECRET_FETCH_FAILURE_COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarnessModelInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_level: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarnessAvailability {
    pub harness: Harness,
    pub display_name: String,
    pub enabled: bool,
    #[serde(default)]
    pub available_models: Vec<HarnessModelInfo>,
}

/// There is no server to list harnesses in this build, so this is the only
/// state the model ever holds: Oz enabled so the UI is usable.
fn default_harnesses() -> Vec<HarnessAvailability> {
    vec![HarnessAvailability {
        harness: Harness::Oz,
        display_name: harness_display::display_name(Harness::Oz).to_string(),
        enabled: true,
        available_models: vec![],
    }]
}

#[derive(Debug, Clone)]
pub enum AuthSecretFetchState {
    NotFetched,
    Failed(#[allow(dead_code)] String),
}

pub enum HarnessAvailabilityEvent {
    /// Emitted when a lazy auth-secrets fetch fails. Subscribers should
    /// re-render so any "Loading…" placeholders can transition to an
    /// error state — without this signal the picker would otherwise be
    /// stuck on the loading placeholder until the next refetch.
    AuthSecretsFetchFailed,
}

pub struct HarnessAvailabilityModel {
    harnesses: Vec<HarnessAvailability>,
    auth_secrets: HashMap<Harness, AuthSecretFetchState>,
    auth_secret_retry_after: HashMap<Harness, Instant>,
}

impl HarnessAvailabilityModel {
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            harnesses: default_harnesses(),
            auth_secrets: HashMap::new(),
            auth_secret_retry_after: HashMap::new(),
        }
    }

    pub fn available_harnesses(&self) -> &[HarnessAvailability] {
        &self.harnesses
    }

    pub fn display_name_for(&self, harness: Harness) -> &str {
        self.harnesses
            .iter()
            .find(|h| h.harness == harness)
            .map(|h| h.display_name.as_str())
            .unwrap_or_else(|| harness_display::display_name(harness))
    }

    /// Whether the harness selector should be shown (>1 known harness, including disabled).
    pub fn should_show_harness_selector(&self) -> bool {
        FeatureFlag::AgentHarness.is_enabled() && self.harnesses.len() > 1
    }

    pub fn models_for(&self, harness: Harness) -> Option<&[HarnessModelInfo]> {
        self.harnesses
            .iter()
            .find(|h| h.harness == harness)
            .map(|h| h.available_models.as_slice())
            .filter(|m| !m.is_empty())
    }

    pub fn auth_secrets_for(&self, harness: Harness) -> &AuthSecretFetchState {
        self.auth_secrets
            .get(&harness)
            .unwrap_or(&AuthSecretFetchState::NotFetched)
    }

    pub fn ensure_auth_secrets_fetched(&mut self, harness: Harness, ctx: &mut ModelContext<Self>) {
        match self.auth_secrets_for(harness) {
            AuthSecretFetchState::NotFetched => self.fetch_auth_secrets(harness, ctx),
            AuthSecretFetchState::Failed(_) if self.can_retry_auth_secret_fetch(harness) => {
                self.fetch_auth_secrets(harness, ctx);
            }
            AuthSecretFetchState::Failed(_) => {}
        }
    }

    /// There is no server to ask for harness auth secrets in this build, so this always
    /// resolves to `Failed` immediately rather than round-tripping through a client that
    /// could only ever answer with an error.
    fn fetch_auth_secrets(&mut self, harness: Harness, ctx: &mut ModelContext<Self>) {
        if to_agent_harness(harness).is_none() {
            return;
        }

        self.auth_secrets.insert(
            harness,
            AuthSecretFetchState::Failed("Auth secrets are not available".to_string()),
        );
        self.auth_secret_retry_after
            .insert(harness, Instant::now() + AUTH_SECRET_FETCH_FAILURE_COOLDOWN);
        ctx.emit(HarnessAvailabilityEvent::AuthSecretsFetchFailed);
    }

    fn can_retry_auth_secret_fetch(&self, harness: Harness) -> bool {
        self.auth_secret_retry_after
            .get(&harness)
            .map(|retry_after| Instant::now() >= *retry_after)
            .unwrap_or(true)
    }
}

fn to_agent_harness(harness: Harness) -> Option<AgentHarness> {
    match harness {
        Harness::Oz => Some(AgentHarness::Oz),
        Harness::Claude => Some(AgentHarness::ClaudeCode),
        Harness::Gemini => Some(AgentHarness::Gemini),
        Harness::Codex => Some(AgentHarness::Codex),
        Harness::OpenCode | Harness::Unknown => None,
    }
}

impl Entity for HarnessAvailabilityModel {
    type Event = HarnessAvailabilityEvent;
}

impl SingletonEntity for HarnessAvailabilityModel {}
