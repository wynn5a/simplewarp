use ai::agent::orchestration_config::OrchestrationConfig;

use super::OrchestrationConfigState;

fn local_config(harness_type: &str, model_id: &str) -> OrchestrationConfig {
    OrchestrationConfig {
        model_id: model_id.to_string(),
        harness_type: harness_type.to_string(),
    }
}

#[test]
fn resolve_from_config_preserves_local_claude() {
    let mut state = OrchestrationConfigState::from_run_agents_fields(None, None);

    state.resolve_from_config(&local_config("claude", "sonnet"));
    assert_eq!(state.harness_type, "claude");
    assert_eq!(state.model_id, "sonnet");
}

#[test]
fn config_round_trips_through_state() {
    let config = local_config("oz", "auto");
    let state = OrchestrationConfigState::from_orchestration_config(&config);
    assert_eq!(state.to_orchestration_config(), config);
}

#[test]
fn resolve_from_config_sanitizes_disabled_local_codex() {
    let mut state = OrchestrationConfigState::from_run_agents_fields(None, None);

    state.resolve_from_config(&local_config("codex", "gpt-5"));

    assert_eq!(state.harness_type, "oz");
    assert_eq!(state.model_id, "");
}

#[test]
fn from_orchestration_config_sanitizes_disabled_local_codex() {
    let state =
        OrchestrationConfigState::from_orchestration_config(&local_config("codex", "gpt-5"));

    assert_eq!(state.harness_type, "oz");
    assert_eq!(state.model_id, "");
}
