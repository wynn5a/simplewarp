use serde::{Deserialize, Serialize};
use warp_cli::agent::Harness;
use warp_core::features::FeatureFlag;
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::ai::harness_display;

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

pub struct HarnessAvailabilityModel {
    harnesses: Vec<HarnessAvailability>,
}

impl HarnessAvailabilityModel {
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            harnesses: default_harnesses(),
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
}

impl Entity for HarnessAvailabilityModel {
    type Event = ();
}

impl SingletonEntity for HarnessAvailabilityModel {}
