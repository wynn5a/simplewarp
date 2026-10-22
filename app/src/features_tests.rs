use super::*;

// SimpleWarp keeps Jupyter notebook rendering on: it is purely local
// (parse + render in-process) and turns `.ipynb` files from raw JSON
// into the notebook viewer.
#[test]
#[cfg(feature = "jupyter_notebook_rendering")]
fn jupyter_notebook_rendering_is_compiled_into_the_build() {
    assert!(enabled_features().contains(&FeatureFlag::JupyterNotebookRendering));
}

// SimpleWarp enables these compile-time flags: each is purely local
// functionality (mermaid editing, the local Docker sandbox pane, the Tantivy palette search,
// prompt polish) with no cloud dependency.
#[test]
#[cfg(feature = "simplewarp")]
fn simplewarp_enables_the_local_feature_set() {
    let enabled = enabled_features();
    for flag in [
        FeatureFlag::EditableMarkdownMermaid,
        FeatureFlag::LocalDockerSandbox,
        FeatureFlag::UseTantivySearch,
        FeatureFlag::AgentViewPromptChip,
        FeatureFlag::AIContextMenuCommands,
        FeatureFlag::AlacrittySettingsImport,
        FeatureFlag::McpJsonTreeView,
        FeatureFlag::FileGlobV2Warnings,
        FeatureFlag::RememberFastForwardState,
        FeatureFlag::RestorePromptOnInlineModelSelectorSearch,
        FeatureFlag::CodeReviewScrollPreservation,
        FeatureFlag::Projects,
    ] {
        assert!(enabled.contains(&flag), "{flag:?} should be enabled");
    }
}
