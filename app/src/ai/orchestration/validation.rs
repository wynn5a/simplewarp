//! Frontend-neutral validation predicates for orchestration edit flows.

use warp_cli::agent::Harness;

use super::config_state::OrchestrationConfigState;
use crate::ai::local_harness_setup::{LocalHarnessSetupState, local_harness_setup_state};

/// [`OrchestrationConfigState::accept_disabled_reason`] plus the local
/// harness CLI setup gate. Card views should prefer this.
pub fn accept_disabled_reason_with_setup(state: &OrchestrationConfigState) -> Option<String> {
    if let Some(reason) = state.accept_disabled_reason() {
        return Some(reason.to_string());
    }
    let harness = Harness::parse_local_child_harness(&state.harness_type)?;
    match local_harness_setup_state(harness) {
        LocalHarnessSetupState::MissingHarness { tooltip } => Some(tooltip.to_string()),
        LocalHarnessSetupState::ProductDisabled { message } => Some(message.to_string()),
        LocalHarnessSetupState::Ready => None,
    }
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
