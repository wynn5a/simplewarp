use std::collections::HashSet;

use warp_core::channel::ChannelState;
pub use warp_core::features::*;

/// Mark all features which should be enabled on the current channel as enabled.
/// This sets global feature flag state and should never be called in a unit test.
pub fn init_feature_flags() {
    for flag in enabled_features() {
        flag.set_enabled(true);
    }
    mark_initialized();
}

/// Returns all feature flags which should be enabled in the current channel.
fn enabled_features() -> HashSet<FeatureFlag> {
    let mut flags = ChannelState::additional_features();
    flags.extend(compiled_features());
    flags
}

/// Returns the feature flags turned on by this build's cargo features, independent of the channel.
pub fn compiled_features() -> HashSet<FeatureFlag> {
    HashSet::from([
        #[cfg(feature = "run_generators_with_cmd_exe")]
        FeatureFlag::RunGeneratorsWithCmdExe,
        #[cfg(all(
            feature = "drag_tabs_to_windows",
            any(target_os = "macos", target_os = "windows")
        ))]
        FeatureFlag::DragTabsToWindows,
        #[cfg(all(not(windows), feature = "kitty_images"))]
        FeatureFlag::KittyImages,
        #[cfg(feature = "msys2_shells")]
        FeatureFlag::MSYS2Shells,
        #[cfg(feature = "vim_code_editor")]
        FeatureFlag::VimCodeEditor,
        #[cfg(feature = "agent_view")]
        FeatureFlag::AgentView,
    ])
}
