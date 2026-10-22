# SimpleWarp — Plan, part 5: leftovers after the remote-service removal

Survey date: 2026-10-22, HEAD `6c45f3ab3` (after the terminal-surface rename). The remote-service
queue in [plan.md](plan.md) is empty; this plan covers what remains of Warp's multi-build,
multi-front-end scaffolding.

**Constraint: no behavior change.** Every item here keeps what the shipping `simplewarp` build does
today. Nothing is a feature removal, so the deletion-scope decision in plan.md is not in play.
Results of each round are recorded in plan.md's round log, as before.

## A — `cargo machete` noise

No dependency to remove. `cargo machete` reports ten rows, and all ten are the false positives
P4-1 restored and [plan-p4c.md](plan-p4c.md) re-verified: macro expansions (`http_client/log`
under `report_error!`; `warp_core` and `app` `concat-idents` under `define_settings_group!`;
`warpui_core` and `warp_core` `strum`, `warpui_core` `num-traits` under derives), attribute paths
(`serde_regex` in `cloud_object_models` and `app`, `serde_bytes` in `app`) and the
`warpui::assets::asset_cache` re-export (`app/asset_cache`).

The only work: record them in each manifest's `[package.metadata.cargo-machete] ignored` list
(`http_client`, `warp_core`, `warpui_core`, `cloud_object_models`, `app`) so the next survey starts
from a clean run and doesn't re-verify them a third time.

## B — "GUI" names left from the two-front-end design

With one front-end the prefix carries no meaning. Pure renames:

| Now | Becomes |
| --- | --- |
| `GuiInputModePolicy` (`ai/blocklist/agent_view/gui_input_mode_policy.rs`) | `AgentViewInputModePolicy` (`input_mode_policy.rs` in the same module) |
| `classify_gui_list_entry` and its `gui_list_policy_*` tests | `classify_list_entry`, `list_policy_*` |
| `BlockSpacing::for_gui` | `BlockSpacing::new` (or `Default`, if it needs no context) |
| `warp_core::paths::gui_config_local_dir` | `app_config_local_dir` |
| `warp_core::paths::gui_mcp_config_file_path` | `app_mcp_config_file_path` |
| bundled-skill variables `{{gui_settings_file_path}}`, `{{gui_mcp_config_file_path}}` | `{{settings_file_path}}`, `{{mcp_config_file_path}}`, with every bundled skill that uses them |

Also fix the doc comments that still say "from any frontend" / "the GUI's".

Kept on purpose:

- `gui_config_local_dir` is **not** merged into `config_local_dir`: under a macOS development
  data profile the first returns `None` rather than guess a source directory. Only the name
  changes.
- `InputModePolicy` and `ConversationSelection` stay traits: each has one production
  implementation plus test stubs (`StubPolicy`, `TestConversationSelection`,
  `MockConversationSelection`), so they are test injection points, not leftovers.
- The `gui-*` skill names in `.agents/skills` follow the AGENTS.md skill convention.

## C — Always-on feature flags

132 of the 137 `FeatureFlag` variants are mapped to a cargo feature in the `simplewarp` set, and
`default = ["simplewarp"]` is the only build. Production code never turns a flag off (the only
production `set_enabled` is `init_feature_flags` setting them on), so these flags always take
the "on" branch in the app. Between them they have 1,385 `FeatureFlag::` references.

Kept: `DebugMode` (debug builds only), `RunGeneratorsWithCmdExe` and `MSYS2Shells` (not in the
set), `DragTabsToWindows` (macOS/Windows only), `KittyImages` (off on Windows).

### Method

Per flag, with the `remove-feature-flag` skill: keep the enabled branch, delete the disabled one,
remove the enum variant, its cargo feature, its `features.rs` mapping and its `simplewarp` entry.
Settings, keybindings and command-palette entries that only exist behind the flag stay (they are
shown today).

### The risk: tests run with flags off

Unit tests do not call `init_feature_flags`, so in unit tests every flag is **off** unless the
test overrides it. Today's suite therefore mostly exercises the branches the shipped app never
takes. Removing a flag moves those tests onto the on-branch, and some will need changing:

- 39 explicit `override_enabled(false)` / `set_enabled(false)` sites in 16 files test the
  off-path on purpose: delete those cases (the behavior they cover no longer exists).
- Tests that implicitly relied on a flag being off: update them to the on-branch behavior, which is
  what the app does. A failure here is a test change, never a reason to keep a dead branch.
- `override_enabled(true)` / `set_enabled(true)` calls in tests and `crates/integration` become
  no-ops; delete them.

If a batch's test changes turn out larger than mechanical, stop and review before continuing.

### Batches

One commit per batch, smallest blast radius first; the two largest flags go alone at the end.

**C1 — terminal, shell, rendering** — 17 flags, 152 refs (**done 2026-10-22, 16 flags**;
`AsyncFind` moved to C1b):
`AsyncFind`, `BoxDrawingGlyphs`, `ITermImages`, `ImeMarkedText`, `KittyKeyboardProtocol`, `Ligatures`, `OscHyperlinks`, `ProgramStatusProtocol`, `RectSelection`, `ResizeFix`, `SelectablePrompt`, `ShellSelector`, `SshDragAndDrop`, `TerminalLifecycleRecovery`, `ToggleBootstrapBlock`, `TrimTrailingBlankLines`, `WarpifyFooter`.

**C1b — AsyncFind (alone, needs review)** — **done 2026-10-22.** Pulled out of C1. With the flag on, the find model
always has an `AsyncFindController`, so the sync block-list find path (`block_list_find_run` and
the `else` arms in `find/model.rs`) is dead in the app. Four view tests (`test_case_sensitive_find`,
`test_find_in_blocks`, `test_find_in_blocks_inverted_blocklist`, `test_find_bar_prefix_search`)
assert sync match counts right after a query and fail on the async path; they need to wait for
the background scan. Also delete the hidden `AsyncFindWidget`, `ToggleAsyncFind`, the inert
`experimental.async_find_enabled` setting and `render_beta_chip`.

**C1c — in-block banner machinery (follow-up of C1)** — **done 2026-10-22.** `WarpifyFooter` was the only producer of
`WithinBlockBanner::WarpifyBanner`; with the banner path gone, nothing sets a block banner. What
is left is dead: `terminal/view/block_banner/` (`WarpifyBannerState`,
`render_warpification_banner`), `Block::block_banner` and its height accounting in `block.rs`
(`block_banner_height`, `BlockSection::BlockBanner`), `BlockList::set_active_block_banner`, the
banner slot in `block_list_element.rs`, `BLOCK_BANNER_HEIGHT`, and the `DismissWarpifyBanner`
action if the footer's dismiss no longer needs it. Not mechanical (block-height layout), so it
gets its own round.

**C2 — input, completions, history, workflows** — 16 flags, 84 refs:
`AllowIgnoringInputSuggestions`, `ClassicCompletions`, `ClearAutosuggestionOnEscape`, `CommandCorrectionKey`, `CommandCorrectionsHistoryRule`, `CycleNextCommandSuggestion`, `DynamicWorkflowEnums`, `ForceClassicCompletions`, `InlineHistoryMenu`, `InlineMenuHeaders`, `NativeShellCompletions`, `NldPromptHistoryMatch`, `PartialNextCommandSuggestions`, `RemoveAutosuggestionDuringTabCompletions`, `ValidateAutosuggestions`, `WorkflowAliases`.

**C3 — tabs, workspace, settings, search** — 30 flags, 316 refs:
`AlacrittySettingsImport`, `AllowOpeningFileLinksUsingEditorEnv`, `AvatarInTabBar`, `CommandPaletteFileSearch`, `ConfigurableToolbar`, `CreateProjectFlow`, `DirectoryTabColors`, `FileTree`, `FullScreenZenMode`, `GetStartedTab`, `GlobalSearch`, `GroupedTabs`, `IncrementalAutoReload`, `InlineProfileSelector`, `InlineRepoMenu`, `MinimalistUI`, `MultiProfile`, `NewTabStyling`, `PinnedTabs`, `PluggableNotifications`, `ProfilesDesignRevamp`, `Projects`, `SettingsFile`, `TabCloseButtonOnLeft`, `TabConfigs`, `UIZoom`, `UndoClosedPanes`, `UseTantivySearch`, `VerticalTabs`, `VerticalTabsSummaryMode`.

**C4 — code editor, code review, notebooks, markdown** — 22 flags, 181 refs:
`AutoOpenCodeReviewPane`, `BlocklistMarkdownImages`, `BlocklistMarkdownTableRendering`, `CodeFindReplace`, `CodeLaunchModal`, `CodeReviewFind`, `CodeReviewSaveChanges`, `CodeReviewScrollPreservation`, `DiscardPerFileAndAllChanges`, `EditableMarkdownMermaid`, `ExpandEditToPane`, `FigmaDetection`, `GitOperationsInCodeReview`, `InlineCodeReview`, `JupyterNotebookRendering`, `LinkedCodeBlocks`, `MarkdownMermaid`, `MarkdownTables`, `PRCommentsV2`, `RevertDiffHunk`, `RichTextMultiselect`, `TabbedEditorView`.

**C5 — AI, agent mode, MCP, skills (everything else)** — 45 flags, 303 refs:
`AIContextMenuCode`, `AIContextMenuCommands`, `AIContextMenuEnabled`, `AIResumeButton`, `AIRules`, `ActiveConversationRequiresInteraction`, `AgentDecidesCommandExecution`, `AgentHarness`, `AgentMode`, `AgentModeWorkflows`, `AgentTips`, `AgentToolbarEditor`, `AgentViewBlockContext`, `AgentViewConversationListView`, `AgentViewPromptChip`, `AtMenuOutsideOfAIMode`, `BundledSkills`, `ConversationsAsContext`, `DiffSetAsContext`, `FallbackModelLoadOutputMessaging`, `FastForwardAutoexecuteButton`, `FileBasedMcp`, `FileGlobV2Warnings`, `ForkFromCommand`, `GithubPrPromptChip`, `ImageAsContext`, `InteractiveConversationManagementView`, `ListSkills`, `LocalDockerSandbox`, `McpJsonTreeView`, `McpOauth`, `McpServer`, `PendingUserQueryIndicator`, `QueueSlashCommand`, `QueuedPromptsV2`, `ReadImageFiles`, `RememberFastForwardState`, `RestorePromptOnInlineModelSelectorSearch`, `RevertToCheckpoints`, `RewindSlashCommand`, `SelectionAsContext`, `SkillArguments`, `SummarizationCancellationConfirmation`, `SummarizationConversationCommand`, `V4AFileDiffs`.

**C6 — VimCodeEditor (alone)** — 1 flag, 74 refs:
`VimCodeEditor`.

**C7 — AgentView (alone, last)** — 1 flag, 275 refs:
`AgentView`.

## Verification, every round

- `./script/format` clean.
- The three clippy gates from `script/presubmit` with `-D warnings`.
- `cargo nextest run --no-fail-fast --workspace`: same pass count minus deleted tests; the only
  allowed failure is the known `test_clone_slash_command_clones_and_enters_the_repository` flake.
- For C: `cargo run` smoke check of the batch's area (open the feature, use it once).
- Commit, push, record the round in plan.md.

## Order

1. A + B together, one commit (A is manifest metadata only).
2. C1 → C7, one commit each; C1b and C1c (split out of C1) go after C1, each in its own round.
