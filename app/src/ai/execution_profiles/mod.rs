pub use cloud_object_models::{
    AIExecutionProfile, ActionPermission, PROFILE_NAME_MAX_LENGTH, WriteToPtyPermission,
};
use warpui::{AppContext, SingletonEntity};

use super::llms::{LLMContextWindow, LLMInfo, LLMPreferences, LLMProvider};
use crate::settings::AISettings;

mod config;
pub mod editor;
pub mod model_menu_items;
pub mod profiles;
pub use config::{ExecutionProfileId, ExecutionProfilesConfig, ProfileLookupError};

fn effective_base_model<'a>(profile: &AIExecutionProfile, app: &'a AppContext) -> &'a LLMInfo {
    let prefs = LLMPreferences::as_ref(app);
    profile
        .base_model
        .as_ref()
        .and_then(|id| prefs.get_llm_info(id))
        .unwrap_or_else(|| prefs.get_default_base_model(app))
}

// Eval builds always use the hard-coded eval profile, so every caller of these helpers is
// compiled out there (see `profiles::implicit_default_profile`).
#[cfg(not(feature = "agent_mode_evals"))]
pub fn create_default_from_legacy_settings(app: &AppContext) -> AIExecutionProfile {
    create_default_from_legacy_settings_with_profile(AIExecutionProfile::default(), app)
}

#[cfg(not(feature = "agent_mode_evals"))]
fn create_default_from_legacy_settings_with_profile(
    default_profile: AIExecutionProfile,
    app: &AppContext,
) -> AIExecutionProfile {
    // Note that the legacy "Autonomy" and "Code Access" settings are not imported here.
    // The "Code Access" setting defaulted to "Always Ask", which is the most restrictive, so
    // it's impossible for us to infer some hesitancy about autonomy from the setting and we should
    // ignore it. The same applies to "Autonomy".
    let ai_settings = AISettings::as_ref(app);
    AIExecutionProfile {
        name: "Default".to_string(),
        is_default_profile: true,
        command_denylist: ai_settings.agent_mode_command_execution_denylist.clone(),
        // We initialize the command allowlist to be anything the user added, excluding all
        // the pre-populated defaults.
        command_allowlist: ai_settings
            .agent_mode_command_execution_allowlist
            .iter()
            .filter(|cmd| !crate::settings::DEFAULT_COMMAND_EXECUTION_ALLOWLIST.contains(cmd))
            .cloned()
            .collect(),
        directory_allowlist: ai_settings.agent_mode_coding_file_read_allowlist.clone(),
        ..default_profile
    }
}

pub trait AIExecutionProfileAppExt {
    fn configurable_context_window(&self, app: &AppContext) -> Option<LLMContextWindow>;

    fn context_window_display_value(&self, app: &AppContext) -> Option<u32>;
    fn context_window_limit_for_request(&self, app: &AppContext) -> Option<u32>;
}

impl AIExecutionProfileAppExt for AIExecutionProfile {
    fn configurable_context_window(&self, app: &AppContext) -> Option<LLMContextWindow> {
        let llm = effective_base_model(self, app);
        if has_configurable_context_window(llm) {
            Some(llm.context_window.clone())
        } else {
            None
        }
    }

    fn context_window_display_value(&self, app: &AppContext) -> Option<u32> {
        let cw = self.configurable_context_window(app)?;
        Some(self.context_window_limit.unwrap_or(cw.default_max))
    }
    fn context_window_limit_for_request(&self, app: &AppContext) -> Option<u32> {
        let llm = effective_base_model(self, app);
        if !has_configurable_context_window(llm) {
            return None;
        }

        self.context_window_limit
            .map(|limit| limit.clamp(llm.context_window.min, llm.context_window.max))
    }
}

pub(crate) fn has_configurable_context_window(llm: &LLMInfo) -> bool {
    llm.context_window.is_configurable
        && llm.context_window.max > 0
        && llm.provider != LLMProvider::OpenAI
}
