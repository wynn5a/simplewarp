use super::accept_disabled_reason_with_setup;
use crate::ai::orchestration::config_state::OrchestrationConfigState;

fn state(harness: &str) -> OrchestrationConfigState {
    OrchestrationConfigState::from_run_agents_fields(Some("auto"), Some(harness))
}

#[test]
fn accept_allowed_for_oz() {
    assert_eq!(accept_disabled_reason_with_setup(&state("oz")), None);
}

#[test]
fn accept_blocked_for_product_disabled_local_codex() {
    assert_eq!(
        accept_disabled_reason_with_setup(&state("codex")),
        Some("Local Codex child agents are temporarily disabled.".to_string())
    );
}
