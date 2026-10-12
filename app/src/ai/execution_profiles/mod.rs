pub use cloud_object_models::{
    AIExecutionProfile, ActionPermission, PROFILE_NAME_MAX_LENGTH, WriteToPtyPermission,
};
use warpui::{AppContext, SingletonEntity};

use crate::settings::AISettings;

mod config;
pub mod editor;
pub mod model_menu_items;
pub mod profiles;
pub use config::{ExecutionProfileId, ExecutionProfilesConfig, ProfileLookupError};

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
