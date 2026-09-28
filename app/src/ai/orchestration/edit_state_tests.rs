use std::collections::HashMap;

use super::OrchestrationEditState;
use crate::ai::orchestration::config_state::OrchestrationConfigState;

fn model_valid_among<'a>(valid: &'a [&'a str]) -> impl Fn(&str, &str) -> bool + 'a {
    move |id, _harness| valid.contains(&id)
}

#[test]
fn harness_change_to_disabled_codex_forces_oz_and_preserves_model_memory() {
    let state = OrchestrationConfigState::from_run_agents_fields(Some("auto"), Some("oz"));
    let mut edit_state = OrchestrationEditState {
        orchestration_config_state: state,
        saved_model_per_harness: HashMap::from([("codex".to_string(), "gpt-5".to_string())]),
    };
    let model_is_valid = |id: &str, harness: &str| match harness {
        "oz" => id == "auto",
        "codex" => id == "gpt-5",
        _ => false,
    };
    let default_model_id = |harness: &str| match harness {
        "oz" => Some("auto".to_string()),
        "codex" => Some(String::new()),
        _ => None,
    };

    edit_state.apply_harness_change_core(
        "codex",
        Some("auto".to_string()),
        &model_is_valid,
        &default_model_id,
    );

    // Local Codex is product-disabled, so the change lands back on Oz.
    assert_eq!(edit_state.orchestration_config_state.harness_type, "oz");
    assert_eq!(edit_state.orchestration_config_state.model_id, "auto");
    assert_eq!(
        edit_state.saved_model_per_harness.get("codex"),
        Some(&"gpt-5".to_string())
    );
}

#[test]
fn harness_change_saves_and_restores_per_harness_model_memory() {
    let state = OrchestrationConfigState::from_run_agents_fields(Some("auto"), Some("oz"));
    let mut edit_state = OrchestrationEditState {
        orchestration_config_state: state,
        saved_model_per_harness: HashMap::from([("claude".to_string(), "sonnet".to_string())]),
    };

    edit_state.apply_harness_change_core(
        "claude",
        None,
        &model_valid_among(&["auto", "sonnet", ""]),
        &|_| Some(String::new()),
    );

    // Restored the saved claude model and remembered the oz model.
    assert_eq!(edit_state.orchestration_config_state.harness_type, "claude");
    assert_eq!(edit_state.orchestration_config_state.model_id, "sonnet");
    assert_eq!(
        edit_state.saved_model_per_harness.get("oz"),
        Some(&"auto".to_string())
    );
}

#[test]
fn harness_change_prefers_valid_fallback_over_default_model() {
    let state = OrchestrationConfigState::from_run_agents_fields(Some("stale"), Some("claude"));
    let mut edit_state = OrchestrationEditState::new(state);

    edit_state.apply_harness_change_core(
        "oz",
        Some("fallback".to_string()),
        &model_valid_among(&["fallback", "first"]),
        &|_| Some("first".to_string()),
    );

    assert_eq!(edit_state.orchestration_config_state.model_id, "fallback");
}

#[test]
fn revalidate_resets_vanished_model_to_default() {
    let mut state = OrchestrationConfigState::from_run_agents_fields(Some("gone"), Some("claude"));

    state.revalidate_after_catalog_change_core(&model_valid_among(&[""]), &|_| Some(String::new()));

    assert_eq!(state.model_id, "");
}
