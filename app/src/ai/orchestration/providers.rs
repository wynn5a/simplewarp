//! `AppContext`-backed catalog lookups and default resolution for
//! orchestration edit flows. No GUI types.

use warp_cli::agent::Harness;
use warpui::{AppContext, SingletonEntity};

use crate::LLMPreferences;
use crate::ai::harness_availability::HarnessAvailabilityModel;

/// Returns whether the given model_id is present in the harness-filtered
/// model choices. Used to detect when a harness change invalidates the
/// current model selection.
pub fn is_model_in_filtered_choices(model_id: &str, harness_type: &str, ctx: &AppContext) -> bool {
    let harness = Harness::parse_orchestration_harness(harness_type);
    match harness {
        Some(Harness::Oz) | None => LLMPreferences::as_ref(ctx)
            .get_base_llm_choices_for_agent_mode()
            .any(|llm| llm.id.to_string() == model_id),
        Some(Harness::Codex) => model_id.is_empty(),
        Some(harness) => {
            // Empty string is always valid (the "Default model" entry).
            if model_id.is_empty() {
                return true;
            }
            let availability = HarnessAvailabilityModel::as_ref(ctx);
            availability
                .models_for(harness)
                .is_some_and(|models| models.iter().any(|m| m.id == model_id))
        }
    }
}

/// Returns the default model_id for the given harness.
///
/// For Oz this is the first Warp LLM; for non-Oz harnesses it is an empty
/// string (the "Default model" entry).
pub fn first_filtered_model_id(harness_type: &str, ctx: &AppContext) -> Option<String> {
    let harness = Harness::parse_orchestration_harness(harness_type);
    match harness {
        Some(Harness::Oz) | None => {
            let llm_prefs = LLMPreferences::as_ref(ctx);
            llm_prefs
                .get_base_llm_choices_for_agent_mode()
                .next()
                .map(|llm| llm.id.to_string())
        }
        Some(_) => Some(String::new()),
    }
}

/// Normalizes a harness_type string for use as a HashMap key in
/// per-harness model memory. Empty string (the wire representation
/// of Oz) is mapped to "oz" so saves and lookups are consistent.
pub fn harness_save_key(harness_type: &str) -> &str {
    if harness_type.is_empty() {
        "oz"
    } else {
        harness_type
    }
}
