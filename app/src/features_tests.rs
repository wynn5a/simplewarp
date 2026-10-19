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
// functionality (mermaid editing, IME marked text, iTerm images, the
// local Docker sandbox pane, the Tantivy palette search, terminal and prompt polish)
// with no cloud dependency.
#[test]
#[cfg(feature = "simplewarp")]
fn simplewarp_enables_the_local_feature_set() {
    let enabled = enabled_features();
    for flag in [
        FeatureFlag::EditableMarkdownMermaid,
        FeatureFlag::ImeMarkedText,
        FeatureFlag::ITermImages,
        FeatureFlag::LocalDockerSandbox,
        FeatureFlag::ProgramStatusProtocol,
        FeatureFlag::UseTantivySearch,
        FeatureFlag::NldPromptHistoryMatch,
        FeatureFlag::CycleNextCommandSuggestion,
        FeatureFlag::PartialNextCommandSuggestions,
        FeatureFlag::ResizeFix,
        FeatureFlag::SelectablePrompt,
        FeatureFlag::AgentViewPromptChip,
        FeatureFlag::AIContextMenuCommands,
        FeatureFlag::CommandCorrectionsHistoryRule,
        FeatureFlag::AlacrittySettingsImport,
        FeatureFlag::McpJsonTreeView,
        FeatureFlag::BoxDrawingGlyphs,
        FeatureFlag::RemoveAutosuggestionDuringTabCompletions,
        FeatureFlag::FileGlobV2Warnings,
        FeatureFlag::RememberFastForwardState,
        FeatureFlag::RestorePromptOnInlineModelSelectorSearch,
        FeatureFlag::ToggleBootstrapBlock,
        FeatureFlag::CodeReviewScrollPreservation,
        FeatureFlag::SshDragAndDrop,
        FeatureFlag::Projects,
    ] {
        assert!(enabled.contains(&flag), "{flag:?} should be enabled");
    }
}
