//! [`OrchestrationEditState`] and the logic for applying user edits to
//! the orchestration config. Changing the harness cascades into the model
//! fallback, so those updates live here as `apply_*` methods rather than in
//! each card view. Each public method takes an `AppContext` for catalog
//! access; the cores are parameterized over catalog callbacks so they can be
//! unit-tested without app singletons.

use std::collections::HashMap;

use warpui::AppContext;

use super::config_state::OrchestrationConfigState;
use super::providers::{first_filtered_model_id, harness_save_key, is_model_in_filtered_choices};

impl OrchestrationConfigState {
    /// Revalidates the state after a live catalog change: resets a
    /// vanished model to the harness default.
    pub fn revalidate_after_catalog_change(&mut self, ctx: &AppContext) {
        self.revalidate_after_catalog_change_core(
            &|id, harness| is_model_in_filtered_choices(id, harness, ctx),
            &|harness| first_filtered_model_id(harness, ctx),
        );
    }

    /// Core of [`Self::revalidate_after_catalog_change`].
    fn revalidate_after_catalog_change_core(
        &mut self,
        model_is_valid: &dyn Fn(&str, &str) -> bool,
        default_model_id: &dyn Fn(&str) -> Option<String>,
    ) {
        self.sanitize_for_local_execution();
        if !model_is_valid(&self.model_id, &self.harness_type)
            && let Some(first_id) = default_model_id(&self.harness_type)
        {
            self.model_id = first_id;
        }
    }

    /// Resets `model_id` when it is invalid for the current harness:
    /// prefers the (validated) fallback, then the harness default.
    fn reset_model_if_invalid(
        &mut self,
        fallback_base_model_id: Option<String>,
        model_is_valid: &dyn Fn(&str, &str) -> bool,
        default_model_id: &dyn Fn(&str) -> Option<String>,
    ) {
        if !model_is_valid(&self.model_id, &self.harness_type) {
            let reset_id = fallback_base_model_id
                .filter(|id| model_is_valid(id, &self.harness_type))
                .or_else(|| default_model_id(&self.harness_type))
                .unwrap_or_default();
            self.model_id = reset_id;
        }
    }
}

/// The edit state for one orchestration card: the run-wide config being
/// edited plus the per-harness model memory, which is UI state rather
/// than request state. Card views own one of these; the executor keeps
/// constructing a bare [`OrchestrationConfigState`] and never carries the
/// memory map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationEditState {
    pub orchestration_config_state: OrchestrationConfigState,
    /// Per-harness model memory so switching harnesses preserves the
    /// user's previous model selection for each harness. Keyed by
    /// [`harness_save_key`].
    pub saved_model_per_harness: HashMap<String, String>,
}

impl OrchestrationEditState {
    /// Wraps `orchestration_config_state` with empty per-harness memory.
    pub fn new(orchestration_config_state: OrchestrationConfigState) -> Self {
        Self {
            orchestration_config_state,
            saved_model_per_harness: HashMap::new(),
        }
    }

    /// Handles a harness change: saves the current model for the old
    /// harness, then restores a previously saved (still valid) model for the
    /// new harness or falls back to a default.
    pub fn apply_harness_change(
        &mut self,
        new_harness_type: &str,
        fallback_base_model_id: Option<String>,
        ctx: &AppContext,
    ) {
        self.apply_harness_change_core(
            new_harness_type,
            fallback_base_model_id,
            &|id, harness| is_model_in_filtered_choices(id, harness, ctx),
            &|harness| first_filtered_model_id(harness, ctx),
        );
    }

    /// Core of [`Self::apply_harness_change`]; catalog access is injected
    /// for unit testing.
    fn apply_harness_change_core(
        &mut self,
        new_harness_type: &str,
        fallback_base_model_id: Option<String>,
        model_is_valid: &dyn Fn(&str, &str) -> bool,
        default_model_id: &dyn Fn(&str) -> Option<String>,
    ) {
        let old_key = harness_save_key(&self.orchestration_config_state.harness_type).to_string();
        self.saved_model_per_harness
            .insert(old_key, self.orchestration_config_state.model_id.clone());
        self.orchestration_config_state.harness_type = new_harness_type.to_string();
        self.orchestration_config_state
            .sanitize_for_local_execution();

        let new_key = harness_save_key(&self.orchestration_config_state.harness_type);
        let restored = self
            .saved_model_per_harness
            .get(new_key)
            .filter(|id| model_is_valid(id, &self.orchestration_config_state.harness_type))
            .cloned();
        if let Some(saved_id) = restored {
            self.orchestration_config_state.model_id = saved_id;
        } else {
            // No saved model — fall back to conversation base model
            // for Oz, or default for non-Oz.
            self.orchestration_config_state.reset_model_if_invalid(
                fallback_base_model_id,
                model_is_valid,
                default_model_id,
            );
        }
    }
}

#[cfg(test)]
#[path = "edit_state_tests.rs"]
mod tests;
