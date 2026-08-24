mod auth_secret_ftux_dropdown;
mod auth_secret_ftux_view;
pub(crate) mod auth_secret_selector;
mod block;
mod delete_auth_secret_confirmation_dialog;
mod first_time_setup;
mod footer;
mod harness_selector;
mod host_selector;
mod loading_screen;
mod model;
mod model_selector;
mod progress;
mod progress_ui_state;
mod tips;
mod view_impl;

pub use auth_secret_ftux_view::{
    AuthSecretFtuxAction, AuthSecretFtuxView, AuthSecretFtuxViewEvent,
};
pub use auth_secret_selector::{
    AuthSecretSelector, AuthSecretSelectorAction, AuthSecretSelectorEvent,
};
pub use block::*;
pub use first_time_setup::{FirstTimeCloudAgentSetupView, FirstTimeCloudAgentSetupViewEvent};
pub use footer::{render_error_footer, render_loading_footer};
pub use harness_selector::{HarnessSelector, HarnessSelectorAction, HarnessSelectorEvent};
pub use host_selector::{
    Host, HostSelector, HostSelectorAction, HostSelectorEvent, NakedHeaderButtonTheme,
};
pub use loading_screen::{render_cloud_mode_error_screen, render_cloud_mode_loading_screen};
pub use model::{AgentProgress, AmbientAgentViewModel, AmbientAgentViewModelEvent, Status};
pub use model_selector::{
    HarnessSelection, ModelSelection, ModelSelector, ModelSelectorAction, ModelSelectorEvent,
};
pub use progress::{ProgressProps, ProgressStep, ProgressStepState, render_progress};
pub use progress_ui_state::AmbientAgentProgressUIState;
pub use tips::{CloudModeTip, get_cloud_mode_tips};
use warp_core::features::FeatureFlag;
use warp_terminal::shell::{ShellName, ShellType};
use warpui::geometry::vector::Vector2F;
use warpui::{AppContext, ModelHandle, ViewHandle, WindowId};

use crate::ai::blocklist::agent_view::{AgentViewController, AgentViewState};
use crate::pane_group::TerminalViewResources;
use crate::terminal::{
    MockTerminalManager, ShellLaunchState, TerminalManager, TerminalModel, TerminalView,
};

/// Creates a cloud mode terminal view and manager for ambient agent sessions.
/// See `viewer::TerminalManager::enable_orchestration_polling` for the flag.
pub fn create_cloud_mode_view(
    resources: TerminalViewResources,
    view_bounds_size: Vector2F,
    window_id: WindowId,
    enable_orchestration_polling: bool,
    ctx: &mut AppContext,
) -> (
    ViewHandle<TerminalView>,
    ModelHandle<Box<dyn TerminalManager>>,
) {
    // A cloud-mode pane has no local shell and, without session sharing, no remote session
    // to attach to either. It is backed by a mock manager so the input and blocklist render.
    let _ = enable_orchestration_polling;
    let terminal_init = MockTerminalManager::create_model(
        ShellLaunchState::ShellSpawned {
            available_shell: None,
            display_name: ShellName::blank(),
            shell_type: ShellType::Zsh,
        },
        resources,
        None,
        None,
        view_bounds_size,
        window_id,
        /* is_ambient_agent */ true,
        ctx,
    );
    (terminal_init.view, terminal_init.manager)
}

/// Returns `true` when a cloud agent shared session is in any pre-first-exchange phase —
/// either still spawning (loading screen) or running setup commands before the first
/// agent turn. In this state, we hide the interactive input and render a loading footer.
pub fn is_cloud_agent_pre_first_exchange(
    ambient_agent_view_model: Option<&ModelHandle<AmbientAgentViewModel>>,
    agent_view_controller: &ModelHandle<AgentViewController>,
    terminal_model: &TerminalModel,
    app: &AppContext,
) -> bool {
    if !(FeatureFlag::CloudMode.is_enabled() && FeatureFlag::AgentView.is_enabled()) {
        return false;
    }

    let Some(ambient_agent_view_model) = ambient_agent_view_model else {
        return false;
    };

    let view_model = ambient_agent_view_model.as_ref(app);

    let is_in_pre_first_exchange_status = matches!(
        view_model.status(),
        Status::WaitingForSession { .. } | Status::AgentRunning
    );
    if !is_in_pre_first_exchange_status {
        return false;
    }

    let agent_view_state = agent_view_controller.as_ref(app).agent_view_state().clone();
    let AgentViewState::Active { origin, .. } = agent_view_state else {
        return false;
    };

    // Handoff panes enter agent view with `RestoreExistingConversation` because they restore the
    // forked conversation, not `CloudAgent`. The `is_local_to_cloud_handoff` flag is the
    // authoritative "this is a cloud agent pane" signal for that path. Shared-session viewers of
    // an ambient run (raw link join / attach-to-running) enter agent view via
    // `SharedSessionSelection` / `ThirdPartyCloudAgent`, so `is_shared_ambient_agent_session()` is
    // the authoritative signal for that path — e.g. a post-death cloud follow-up spinning up a new
    // VM must still count as pre-first-exchange so the setup progress + prompt-queuing UI render.
    if !origin.is_cloud_agent() && !view_model.is_local_to_cloud_handoff() {
        return false;
    }

    // For non-oz harness runs, there is no Oz `AppendedExchange` to key off of, so we also
    // exit the pre-first-exchange phase when the harness CLI (e.g. `claude`, `gemini`) has
    // been detected. See `mark_harness_command_started`.
    if view_model.harness_command_started() {
        return false;
    }

    // Loading phase (`WaitingForSession`): no setup commands have started yet, but we're
    // still pre-first-exchange. Skip the block-list flag check.
    if matches!(view_model.status(), Status::WaitingForSession { .. }) {
        return true;
    }

    terminal_model
        .block_list()
        .is_executing_oz_environment_startup_commands()
}
