use super::*;

// SimpleWarp enables these compile-time flags: each is purely local
// functionality (the local Docker sandbox pane, prompt polish) with no cloud dependency.
#[test]
#[cfg(feature = "simplewarp")]
fn simplewarp_enables_the_local_feature_set() {
    let enabled = enabled_features();
    for flag in [
        FeatureFlag::LocalDockerSandbox,
        FeatureFlag::AgentViewPromptChip,
        FeatureFlag::AIContextMenuCommands,
        FeatureFlag::McpJsonTreeView,
        FeatureFlag::FileGlobV2Warnings,
        FeatureFlag::RememberFastForwardState,
        FeatureFlag::RestorePromptOnInlineModelSelectorSearch,
    ] {
        assert!(enabled.contains(&flag), "{flag:?} should be enabled");
    }
}
