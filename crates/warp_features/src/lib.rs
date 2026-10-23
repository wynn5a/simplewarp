use std::sync::atomic::{AtomicBool, Ordering};

use enum_iterator::{Sequence, cardinality};
#[cfg(feature = "test-util")]
pub use overrides::{get_overrides, set_overrides};

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug, Sequence)]
pub enum FeatureFlag {
    DebugMode,

    /// If set, generators are executed using cmd.exe on Windows.
    RunGeneratorsWithCmdExe,

    /// Warp Agent Mode.
    AgentMode,

    /// Enable multiselect in Notebooks and Warp Text.
    RichTextMultiselect,

    /// Enables workflows for use with Agent Mode.
    AgentModeWorkflows,

    /// Enables AI rules for use with Agent Mode.
    AIRules,

    DragTabsToWindows,

    /// Enables Kitty image rendering
    KittyImages,

    /// MCP server v0 functionality.
    McpServer,

    /// Enables image as context for AM.
    ImageAsContext,

    /// UNIX shells running "natively" on Windows via MSYS2.
    MSYS2Shells,

    /// Enables reading images with the `read_files` tool.
    ReadImageFiles,

    /// Enables the AI context menu, or at-menu.
    AIContextMenuEnabled,

    /// Enables the AI context menu outside of AI input mode.
    AtMenuOutsideOfAIMode,

    /// Enables the resume button for cancelled AI conversations.
    AIResumeButton,

    /// Enables the agent to decide whether to execute a command.
    AgentDecidesCommandExecution,

    /// Enables the fast-forward autoexecute button
    FastForwardAutoexecuteButton,

    /// Remembers the per-conversation fast-forward state across local session restoration.
    RememberFastForwardState,

    /// Enables the find/replace in code editor
    CodeFindReplace,

    /// Enables the AI context menu nesting and commands
    AIContextMenuCommands,

    /// Enables sending stderr warnings in FileGlobV2 results.
    FileGlobV2Warnings,

    /// Enables code symbols in AI context menu
    AIContextMenuCode,

    /// Expands code diff edits to replace the current pane instead of opening in a new tab.
    ExpandEditToPane,
    /// Enables fallback model load output messaging in the warping indicator.
    FallbackModelLoadOutputMessaging,

    /// Enables us to render linked code blocks
    LinkedCodeBlocks,

    /// Enables the tabbed file viewer
    TabbedEditorView,

    /// Enables displaying imported PR review comments in the blocklist.
    PRCommentsV2,

    /// Enables selection-as-context functionality in the code editor.
    SelectionAsContext,

    /// Enables the prompt chip that displays the GitHub PR for the current branch.
    GithubPrPromptChip,

    /// Enables vim keybindings in the code editor.
    VimCodeEditor,

    /// Enables revert button for diff hunks in the gutter.
    RevertDiffHunk,

    /// Enables saving code review pane changes
    CodeReviewSaveChanges,

    /// Enables the one-time modal on app startup for existing users for the Code launch.
    CodeLaunchModal,

    /// Enables OAuth support for MCP.
    McpOauth,

    /// Enables attaching diff sets (multiple hunks from multiple files) as context in Agent Mode.
    DiffSetAsContext,

    /// Enables discarding per-file and discarding all changes
    DiscardPerFileAndAllChanges,

    /// Shows a confirmation dialog when cancelling an active summarization via Ctrl-C or stop.
    SummarizationCancellationConfirmation,

    /// Enables find/search in code review pane
    CodeReviewFind,

    /// Enables auto-opening code review pane on first agent change and its setting UI.
    AutoOpenCodeReviewPane,

    /// Enables inline code review functionality
    InlineCodeReview,

    /// Enables the local docker sandbox entrypoints in the client.
    LocalDockerSandbox,

    /// Enables the /compact slash command.
    SummarizationConversationCommand,

    /// Enables rendering Mermaid diagrams in markdown notebooks.
    MarkdownMermaid,
    /// Enables editable Mermaid diagrams to behave atomically in notebook and plan editors.
    EditableMarkdownMermaid,

    /// Enables rendering markdown tables in notebooks.
    MarkdownTables,

    /// Renders `.ipynb` (Jupyter) files as a formatted, read-only notebook in
    /// Warp's notebook viewer instead of showing the raw JSON in the code editor.
    JupyterNotebookRendering,

    /// Enables rendering markdown tables inline in AI block list responses.
    BlocklistMarkdownTableRendering,
    /// Enables rendering markdown images inline in AI block list responses.
    BlocklistMarkdownImages,

    /// Enables the /fork-from slash command.
    ForkFromCommand,

    /// Enables the revert to checkpoints feature.
    RevertToCheckpoints,

    /// Enables the /rewind slash command.
    RewindSlashCommand,

    AgentView,

    /// Enables block context functionality in Agent View.
    AgentViewBlockContext,

    /// Enables support for AM file diffs backed by the V4A patch format.
    V4AFileDiffs,

    /// Enables loading conversations in the Agent Management View.
    InteractiveConversationManagementView,

    /// Enables agent tips displayed below the warping indicator in Agent Mode.
    AgentTips,

    /// Enables the "New agent" prompt chip in terminal mode when AgentView is enabled.
    ///
    /// When disabled (the default), the terminal message bar is shown instead.
    AgentViewPromptChip,

    /// Enables editing the agent input footer layout from the prompt context menu.
    AgentToolbarEditor,

    // Enables a side panel conversation list view for AgentView mode.
    AgentViewConversationListView,

    /// Enables sending the server a list of Skills that the client has access to.
    ///
    /// If disabled, the server will send None as the SkillsContext.
    ListSkills,

    /// Enables loading and returning bundled skills in the SkillManager.
    BundledSkills,

    /// Enables file-based MCP server support via .mcp.json files in repo roots.
    FileBasedMcp,

    /// Enables passing user query arguments to skill invocations ($ARGUMENTS, $N).
    SkillArguments,

    /// When enabled, a conversation is only considered "active" once a new query has been
    /// sent since opening (rather than the moment its agent view is expanded).
    ActiveConversationRequiresInteraction,

    /// Enables attaching conversations as context in Agent Mode via the @ menu.
    ConversationsAsContext,

    /// Enables scroll position preservation in the code review pane when file
    /// content changes via auto-reload.
    CodeReviewScrollPreservation,

    /// Shows a pending user query indicator during summarization when a follow-up
    /// prompt is queued via `/fork-and-compact` or `/compact-and`.
    PendingUserQueryIndicator,

    /// Gates the `/queue` slash command, which lets users queue a follow-up prompt
    /// while the agent is mid-response.
    QueueSlashCommand,
    /// Extends queued prompts to shell commands, summarization, and follow-up draining.
    QueuedPromptsV2,

    /// Detects the word "figma" in the terminal input in real-time and shows a
    /// contextual button above the input.
    FigmaDetection,

    /// Clears the current prompt when opening the inline model selector from the
    /// model chip, then restores that prompt when the selector closes.
    RestorePromptOnInlineModelSelectorSearch,

    /// Enables the `--harness` flag for `oz agent run`, allowing external agent
    /// CLIs (e.g. `claude`) to execute prompts instead of Warp's agent harness.
    AgentHarness,

    /// Enables commit, push, and create-PR actions in the code review panel.
    GitOperationsInCodeReview,

    /// Renders MCP tool-call request and response JSON as an interactive
    /// collapsible tree with typed colors and per-row Copy JSON, instead of
    /// a flat pretty-printed blob.
    McpJsonTreeView,
}

static FLAG_STATES: [AtomicBool; cardinality::<FeatureFlag>()] =
    [const { AtomicBool::new(false) }; { cardinality::<FeatureFlag>() }];

/// Flag for whether or not feature flags have been globally initialized. Outside
/// of tests, this ensures that feature flags are only used after they're set
/// up by the app's `run_internal` function.
#[cfg(debug_assertions)]
static FEATURES_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Features used in debugging.
pub const DEBUG_FLAGS: &[FeatureFlag] = &[FeatureFlag::DebugMode];

impl FeatureFlag {
    pub fn is_enabled(&self) -> bool {
        #[cfg(all(debug_assertions, not(feature = "test-util")))]
        {
            use std::sync::atomic::Ordering;
            assert!(
                FEATURES_INITIALIZED.load(Ordering::Relaxed),
                "Tried to check FeatureFlag::{self:?} before feature flags were initialized"
            );
        }

        overrides::get_override(*self)
            .or(Some(FLAG_STATES[*self as usize].load(Ordering::Relaxed)))
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn set_enabled(self, enabled: bool) {
        // Allow calling this in integration tests because we sometimes use it in the app
        // during flows that integration tests cover.
        if cfg!(test) && cfg!(not(feature = "integration_tests")) {
            panic!(
                "Tried to globally enable {self:?} in a test. Use FeatureFlag::{self:?}.override_enabled instead"
            );
        }
        FLAG_STATES[self as usize].store(enabled, Ordering::Relaxed);
    }

    /// Sets a thread-local test override for this flag. The override lasts
    /// until the returned guard is dropped.
    ///
    /// **Warning**: overrides do not work for tests of multi-threaded code. If
    /// you need to test multi-threaded code that's behind a feature flag, you'll
    /// need to set an override in _each_ thread.
    ///
    /// Tests should create overrides early on and allow them to be
    /// dropped automatically when they finish. This keeps overrides scoped to
    /// the duration of the test, since Rust doesn't have test lifecycle hooks.
    #[cfg(feature = "test-util")]
    pub fn override_enabled(self, enabled: bool) -> overrides::OverrideGuard {
        overrides::override_flag(self, enabled)
    }
}

/// Marks that feature flags have been globally initialized.
pub fn mark_initialized() {
    #[cfg(debug_assertions)]
    FEATURES_INITIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(feature = "test-util"))]
mod overrides {
    #[inline(always)]
    pub fn get_override(_flag: super::FeatureFlag) -> Option<bool> {
        None
    }
}

/// Thread-local feature flag overrides for unit tests. For isolation, tests
/// should use overrides instead of globally modifying flags with [`super::FeatureFlag::set_enabled`].
#[cfg(feature = "test-util")]
mod overrides {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::FeatureFlag;

    thread_local! {
        static FLAG_OVERRIDES: RefCell<HashMap<FeatureFlag,bool>> = RefCell::new(HashMap::new());
    }

    /// RAII guard to set feature flag overrides in tests. When the guard is
    /// dropped, it reverts to the global flag state.
    #[must_use = "if unused the override will be immediately cleared"]
    pub struct OverrideGuard {
        flag: FeatureFlag,
    }

    /// Gets the overridden state for a flag, if set.
    pub fn get_override(flag: FeatureFlag) -> Option<bool> {
        FLAG_OVERRIDES.with(|overrides| overrides.borrow().get(&flag).copied())
    }

    /// Gets the set of overridden flags.
    pub fn get_overrides() -> HashMap<FeatureFlag, bool> {
        FLAG_OVERRIDES.with(|overrides| overrides.borrow().clone())
    }

    /// Applies a set of overrides.
    ///
    /// This is intended to be used with [`get_overrides`] to apply a set of
    /// existing overrides to a newly-spawned thread.  If you are trying to
    /// override a single feature flag, use [`FeatureFlag::override_enabled`]
    /// instead.
    pub fn set_overrides(new_overrides: HashMap<FeatureFlag, bool>) {
        FLAG_OVERRIDES.with(|overrides| *overrides.borrow_mut() = new_overrides);
    }

    /// Set a thread-local override for a flag.
    pub fn override_flag(flag: FeatureFlag, enabled: bool) -> OverrideGuard {
        set_override(flag, enabled);
        OverrideGuard { flag }
    }

    fn set_override(flag: FeatureFlag, enabled: bool) {
        FLAG_OVERRIDES.with(|overrides| {
            let previous = overrides.borrow_mut().insert(flag, enabled);
            // We could support nested overrides, but it requires some care around
            // out-of-order drops - if overrides are set and then cleared out of
            // order, what should the state after each drop be?
            if previous.is_some() {
                panic!("Multiple overrides set for {flag:?}");
            }
        });
    }

    fn clear_override(flag: FeatureFlag) {
        FLAG_OVERRIDES.with(|overrides| {
            let previous = overrides.borrow_mut().remove(&flag);
            if previous.is_none() {
                panic!("Cleared override for {flag:?}, but none was set");
            }
        });
    }

    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            clear_override(self.flag);
        }
    }
}
