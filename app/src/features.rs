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
        #[cfg(feature = "agent_mode")]
        FeatureFlag::AgentMode,
        #[cfg(feature = "richtext_multiselect")]
        FeatureFlag::RichTextMultiselect,
        #[cfg(feature = "am_workflows")]
        FeatureFlag::AgentModeWorkflows,
        #[cfg(feature = "ai_rules")]
        FeatureFlag::AIRules,
        #[cfg(all(
            feature = "drag_tabs_to_windows",
            any(target_os = "macos", target_os = "windows")
        ))]
        FeatureFlag::DragTabsToWindows,
        #[cfg(all(not(windows), feature = "kitty_images"))]
        FeatureFlag::KittyImages,
        #[cfg(feature = "mcp_server")]
        FeatureFlag::McpServer,
        #[cfg(feature = "markdown_tables")]
        FeatureFlag::MarkdownTables,
        #[cfg(feature = "jupyter_notebook_rendering")]
        FeatureFlag::JupyterNotebookRendering,
        #[cfg(feature = "blocklist_markdown_table_rendering")]
        FeatureFlag::BlocklistMarkdownTableRendering,
        #[cfg(feature = "blocklist_markdown_images")]
        FeatureFlag::BlocklistMarkdownImages,
        #[cfg(feature = "markdown_mermaid")]
        FeatureFlag::MarkdownMermaid,
        #[cfg(feature = "editable_markdown_mermaid")]
        FeatureFlag::EditableMarkdownMermaid,
        #[cfg(feature = "image_as_context")]
        FeatureFlag::ImageAsContext,
        #[cfg(feature = "msys2_shells")]
        FeatureFlag::MSYS2Shells,
        #[cfg(feature = "read_image_files")]
        FeatureFlag::ReadImageFiles,
        #[cfg(feature = "ai_context_menu")]
        FeatureFlag::AIContextMenuEnabled,
        #[cfg(feature = "at_menu_outside_of_ai_mode")]
        FeatureFlag::AtMenuOutsideOfAIMode,
        #[cfg(feature = "ai_resume_button")]
        FeatureFlag::AIResumeButton,
        #[cfg(feature = "figma_detection")]
        FeatureFlag::FigmaDetection,
        #[cfg(feature = "agent_decides_command_execution")]
        FeatureFlag::AgentDecidesCommandExecution,
        #[cfg(feature = "fast_forward_autoexecute_button")]
        FeatureFlag::FastForwardAutoexecuteButton,
        #[cfg(feature = "code_find_replace")]
        FeatureFlag::CodeFindReplace,
        #[cfg(feature = "ai_context_menu_commands")]
        FeatureFlag::AIContextMenuCommands,
        #[cfg(feature = "ai_context_menu_code")]
        FeatureFlag::AIContextMenuCode,
        #[cfg(feature = "expand_edit_to_pane")]
        FeatureFlag::ExpandEditToPane,
        #[cfg(feature = "fallback_model_load_output_messaging")]
        FeatureFlag::FallbackModelLoadOutputMessaging,
        #[cfg(feature = "linked_code_blocks")]
        FeatureFlag::LinkedCodeBlocks,
        #[cfg(feature = "tabbed_editor_view")]
        FeatureFlag::TabbedEditorView,
        #[cfg(feature = "pr_comments_v2")]
        FeatureFlag::PRCommentsV2,
        #[cfg(feature = "selection_as_context")]
        FeatureFlag::SelectionAsContext,
        #[cfg(feature = "github_pr_prompt_chip")]
        FeatureFlag::GithubPrPromptChip,
        #[cfg(feature = "vim_code_editor")]
        FeatureFlag::VimCodeEditor,
        #[cfg(feature = "revert_diff_hunk")]
        FeatureFlag::RevertDiffHunk,
        #[cfg(feature = "code_review_save_changes")]
        FeatureFlag::CodeReviewSaveChanges,
        #[cfg(feature = "code_launch_modal")]
        FeatureFlag::CodeLaunchModal,
        #[cfg(feature = "mcp_oauth")]
        FeatureFlag::McpOauth,
        #[cfg(feature = "file_based_mcp")]
        FeatureFlag::FileBasedMcp,
        #[cfg(feature = "diff_set_as_context")]
        FeatureFlag::DiffSetAsContext,
        #[cfg(feature = "discard_per_file_and_all_changes")]
        FeatureFlag::DiscardPerFileAndAllChanges,
        #[cfg(feature = "summarization_cancellation_confirmation")]
        FeatureFlag::SummarizationCancellationConfirmation,
        #[cfg(feature = "code_review_find")]
        FeatureFlag::CodeReviewFind,
        #[cfg(feature = "auto_open_code_review_pane")]
        FeatureFlag::AutoOpenCodeReviewPane,
        #[cfg(feature = "inline_code_review")]
        FeatureFlag::InlineCodeReview,
        #[cfg(feature = "summarize_conversation_command")]
        FeatureFlag::SummarizationConversationCommand,
        #[cfg(feature = "fork_from_command")]
        FeatureFlag::ForkFromCommand,
        #[cfg(feature = "revert_to_checkpoints")]
        FeatureFlag::RevertToCheckpoints,
        #[cfg(feature = "rewind_slash_command")]
        FeatureFlag::RewindSlashCommand,
        #[cfg(feature = "agent_view")]
        FeatureFlag::AgentView,
        #[cfg(feature = "agent_view_block_context")]
        FeatureFlag::AgentViewBlockContext,
        #[cfg(feature = "v4a_file_diffs")]
        FeatureFlag::V4AFileDiffs,
        #[cfg(feature = "interactive_conversation_management_view")]
        FeatureFlag::InteractiveConversationManagementView,
        #[cfg(feature = "agent_tips")]
        FeatureFlag::AgentTips,
        #[cfg(feature = "local_docker_sandbox")]
        FeatureFlag::LocalDockerSandbox,
        #[cfg(feature = "agent_toolbar_editor")]
        FeatureFlag::AgentToolbarEditor,
        #[cfg(feature = "agent_view_prompt_chip")]
        FeatureFlag::AgentViewPromptChip,
        #[cfg(feature = "agent_view_conversation_list_view")]
        FeatureFlag::AgentViewConversationListView,
        #[cfg(feature = "list_skills")]
        FeatureFlag::ListSkills,
        #[cfg(feature = "bundled_skills")]
        FeatureFlag::BundledSkills,
        #[cfg(feature = "skill_arguments")]
        FeatureFlag::SkillArguments,
        #[cfg(feature = "active_conversation_requires_interaction")]
        FeatureFlag::ActiveConversationRequiresInteraction,
        #[cfg(feature = "conversations_as_context")]
        FeatureFlag::ConversationsAsContext,
        #[cfg(feature = "pending_user_query_indicator")]
        FeatureFlag::PendingUserQueryIndicator,
        #[cfg(feature = "queue_slash_command")]
        FeatureFlag::QueueSlashCommand,
        #[cfg(feature = "queued_prompts_v2")]
        FeatureFlag::QueuedPromptsV2,
        #[cfg(feature = "restore_prompt_on_inline_model_selector_search")]
        FeatureFlag::RestorePromptOnInlineModelSelectorSearch,
        #[cfg(feature = "agent_harness")]
        FeatureFlag::AgentHarness,
        #[cfg(feature = "git_operations_in_code_review")]
        FeatureFlag::GitOperationsInCodeReview,
        #[cfg(feature = "code_review_scroll_preservation")]
        FeatureFlag::CodeReviewScrollPreservation,
        #[cfg(feature = "mcp_json_tree_view")]
        FeatureFlag::McpJsonTreeView,
        #[cfg(feature = "file_glob_v2_warnings")]
        FeatureFlag::FileGlobV2Warnings,
        #[cfg(feature = "remember_fast_forward_state")]
        FeatureFlag::RememberFastForwardState,
    ])
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
