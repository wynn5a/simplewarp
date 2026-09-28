//! Run-wide orchestration edit state shared by the confirmation card and the
//! plan-card config block.

use ai::agent::orchestration_config::OrchestrationConfig;
use warp_cli::agent::Harness;

use crate::ai::local_harness_setup::local_harness_product_disabled_message;

/// Run-wide configuration fields shared between the confirmation card
/// editor and the plan-card config block. Card-specific fields
/// (agent_run_configs, base_prompt, summary, skills)
/// remain on the per-view state structs. Children always run locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationConfigState {
    pub model_id: String,
    pub harness_type: String,
}

impl OrchestrationConfigState {
    pub(crate) fn sanitize_for_local_execution(&mut self) {
        let Some(harness) = Harness::parse_local_child_harness(&self.harness_type) else {
            return;
        };
        if local_harness_product_disabled_message(harness).is_some() {
            self.harness_type = "oz".to_string();
            self.model_id.clear();
        }
    }

    /// `None` (or an empty string wrapped in `Some`) leaves the field
    /// unset, matching the wire encoding where absence is emptiness.
    pub fn from_run_agents_fields(model_id: Option<&str>, harness_type: Option<&str>) -> Self {
        Self {
            model_id: model_id.unwrap_or_default().to_string(),
            harness_type: harness_type.unwrap_or_default().to_string(),
        }
    }

    pub fn from_orchestration_config(config: &OrchestrationConfig) -> Self {
        let mut state = Self {
            model_id: config.model_id.clone(),
            harness_type: config.harness_type.clone(),
        };
        state.sanitize_for_local_execution();
        state
    }

    /// Returns `Some(reason)` if Accept / Apply must be disabled because the
    /// selected local harness is product-disabled.
    pub fn accept_disabled_reason(&self) -> Option<&'static str> {
        Harness::parse_local_child_harness(&self.harness_type)
            .and_then(local_harness_product_disabled_message)
    }

    /// Fills in empty fields from the approved orchestration config.
    /// When the LLM omits harness/model to inherit from the active config,
    /// the raw request arrives with empty values. This resolves those to the
    /// config values so the UI shows the intended settings.
    pub fn resolve_from_config(&mut self, config: &OrchestrationConfig) {
        if self.harness_type.is_empty() && !config.harness_type.is_empty() {
            self.harness_type = config.harness_type.clone();
        }
        if self.model_id.is_empty() && !config.model_id.is_empty() {
            self.model_id = config.model_id.clone();
        }
        self.sanitize_for_local_execution();
    }

    /// Unconditionally overrides model and harness from the approved
    /// orchestration config. The plan config is the user-approved source of
    /// truth — the LLM's run_agents call may omit or set these differently,
    /// but the config always wins.
    pub fn override_from_approved_config(&mut self, config: &OrchestrationConfig) {
        self.model_id = config.model_id.clone();
        self.harness_type = config.harness_type.clone();
    }

    /// Converts to a native `OrchestrationConfig` for storage.
    pub fn to_orchestration_config(&self) -> OrchestrationConfig {
        OrchestrationConfig {
            model_id: self.model_id.clone(),
            harness_type: self.harness_type.clone(),
        }
    }
}

#[cfg(test)]
#[path = "config_state_tests.rs"]
mod tests;
