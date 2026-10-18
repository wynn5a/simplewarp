# SimpleWarp — Plan, part 4b: dead-code sweep, semantic pass (2026-10-17)

Companion to `plan-p4.md` (same day, HEAD `74c21c93`), which was the *mechanical* sweep (unused
deps, allow→warn probe, orphan files, features). This pass covers what that one explicitly did
not: **cross-crate `pub` API, action/settings/menu wiring, whole dead modules, DB tables, assets,
and a full audit of all 255 `allow(dead_code)` sites.** Method: five parallel evidence-gated
sweeps (every candidate re-verified by repo-wide `rg`, including non-Rust files for string-keyed
channels), plus `cargo-machete` with per-dep verification, plus hand re-verification of every
headline item. `warnalyzer` fails in this environment (`StrErr("No such file or directory")` even
for `-p sum_tree`); the greps cover its ground. plan-p3's intentional keeps were excluded
throughout.

**Totals for this pass: ≈3,800–3,900 deletable Rust lines (≈3,000 high confidence, before
deduping the ~1,400 lines shared with plan-p4's probe findings — see §8), ~22.4 MB of dead
assets/fixtures (~60 files), 47 stale annotations, 1 deprecated-with-zero-callers fn.**
Dependency findings merge into plan-p4 §1 (§8 below); nothing has been deleted yet.

## 1. Largest single wins

| Item | Where | Size | Evidence |
| --- | --- | --- | --- |
| `code/file_tree/snapshot` module | `app/src/code/file_tree/snapshot.rs` + `snapshot/iterator.rs` + `snapshot_tests.rs` | ~910 lines | `FileTreeSnapshot` referenced only by its own 3 files (re-derived here; = plan-p4 §2, which also found it) |
| `capture_recorder.rs`, whole file | `crates/warpui_core/src/integration/capture_recorder.rs` | ~495 lines | `CaptureRecorder`/`CAPTURE_RECORDER_KEY` have zero refs outside the file on **any** feature (grep verified; plan-p4's "live under integration_tests" caution is disproved — the live path is `video_recorder.rs`) |
| MCP loopback-OAuth flow | `crates/mcp/src/oauth/loopback.rs` + `Loopback` variants + plumbing in `oauth.rs` | ~155 lines | product only constructs `OAuthCallbackMode::CustomScheme`; loopback reachable only from `oauth_tests.rs` |
| 9 dead tables in `persistence/src/schema.rs` | `cloud_objects_refreshes`, `current_user_information`, `server_experiments`, `team_members`, `team_settings`, `teams`, `user_profiles`, `users`, `workspace_teams` | ~65 lines + joinable/allow-tables lines | zero Diesel query/model refs anywhere (migrations excluded by design); `schema::users` grep = 0 |
| `common_path` + `WslUncPath` chain | `crates/warp_util/src/path.rs:688-748`, `:515-583` | ~120 lines | zero callers (WSL half: only `path_tests.rs`) |
| Dead assets | see §7 | ~22.4 MB | zero references |

## 2. app/src — ≈740 lines

### 2a. Dead action enum variants (never constructed; def + handler arm) — 29 variants, ~215 lines
All HIGH: no serde, no macro-by-name construction; the string-keyed channels (`add_action`
registry, `CustomAction` tags) audited separately and clean.

- `workspace/action.rs:534` `WorkspaceAction::OpenWorktreeInRepo` (~8)
- `util/bindings.rs:68,69,87` `CustomAction::{ActivateNextTab, ActivatePreviousTab, TriggerWelcomeBlock}` (~23)
- `code_review/code_review_view.rs:300` `CodeReviewAction::EmitPaneEvent` (~5; the 18 name hits are the different `ContextMenuAction::EmitPaneEvent`)
- `workflows/info_box.rs:1067` `WorkflowsInfoBoxViewAction::SelectEnvironmentVariables` (~5)
- `workflows/workflow_view.rs:171` `WorkflowAction::ToggleViewMode` — chain-deads private `toggle_view_mode` (42) (~50)
- `workflows/workflow_view.rs:176` `WorkflowAction::CloseEnumDialog` (~4)
- `context_chips/display.rs:70` `PromptDisplayAction::SelectGitBranch` (~4)
- `ai/facts/view/mod.rs:54,55` `AIFactViewAction::{AddRule, UpdatePage}` (distinct from the live `RuleViewAction::AddRule`) (~7)
- `terminal/input.rs:897` `InputAction::ToggleInputAutoDetection` (~4)
- `ai/execution_profiles/editor/mod.rs:80,86,92` `{AddToCommandAllowlist, AddToCommandDenylist, AddToDirectoryAllowlist}` (~18)
- `notebooks/notebook.rs:224` `NotebookAction::CopyToClipboard` — chain-deads `copy_notebook_contents_to_clipboard` (~10)
- `notebooks/file/mod.rs:118` `FileNotebookAction::OpenAsCode` (~4)
- `ai/blocklist/block/status_bar.rs:1115` `BlocklistAIStatusBarAction::SwitchCommandControlToUser` (~4)
- `ai/blocklist/block.rs:3915` `AIBlockAction::ClearOtherSelections` (~6)
- `ai/blocklist/agent_view/agent_input_footer/mod.rs:788` `AgentInputFooterAction::ToggleCodeReview` (live twin: `DisplayChipAction::ToggleCodeReview`) (~4)
- `workspace/view/conversation_list/view.rs:112,143,144` `{DeleteConversation, FinishRename, CancelRename}` (~14)
- `search/ai_context_menu/view.rs:146` `AIContextMenuAction::SelectCurrentItem` (~4)
- `settings_view/agent_profiles_page.rs:1200,1201,1214` `{RemoveFromCommandExecutionAllowlist, RemoveFromCommandExecutionDenylist, RemoveDirectoryFromCodeReadAllowlist}` (~15)
- `settings_view/mod.rs:830` `SettingsAction::Tab` (~3)
- `settings_view/warp_agent_page.rs:990` `WarpAgentPageAction::OpenUrl` (~4)

### 2b. Dead pub fns (zero refs anywhere, incl. tests) — ~380 lines
All HIGH, brace-matched bodies. Highlights: `ai/blocklist/controller.rs:726
send_agent_query_in_conversation` (17); `terminal/cli_agent.rs:265 supported_skill_providers` (29);
`terminal/view.rs:3887 enqueue_prompt` (16), `:4154 is_conversation_selected`, `:6429
set_show_pane_accent_border`; `workspace/view.rs:2614 is_conversation_transcript_viewer_focused`;
`workspace/util.rs:265 get_context_target_terminal_view`; `terminal/model/blocks.rs:1268
toggle_visibility_of_block`; `terminal/local_tty/docker_sandbox.rs:58 resolve_sbx_path`;
`terminal/local_tty/terminal_manager.rs:217 create_tui_model`; `terminal/ssh/util.rs:14
convert_script_to_one_line`; the `ai/agent/mod.rs` cluster (`is_rejected`,
`calculate_action_index`, `reparse_markdown`, `skill_invoked`, `time_since_start`,
`text_from_agent_reasoning`, `is_request_file_edit`,
`is_agent_monitored_request_command_output`, `mut_text`); `ai/blocklist/action_model.rs`
(`set_view_only`, `get_pending_actions`, `get_pending_actions_for_conversation`);
`ai/blocklist/history_model/conversation_loader.rs:127 load_conversation_by_server_token`;
`ai/skills/listed_skill.rs:23 is_project_skill`; `ai/mcp/templatable_installation.rs:94,109`;
`settings_view/mcp_servers/edit_page.rs:560 save_mcp_server_env_vars`;
`workflows/aliases.rs:58 get_all_aliases`; the `pane_group` cluster (`with_homepage_hidden`,
`env_var_collection_pane_by_pane_id`, `workflow_pane_by_pane_id`, `ai_fact_pane_by_pane_id`,
`tree.rs:215 num_hidden_panes`, `pane/mod.rs:364 is_warp_drive_object_pane`);
`editor/view/model/buffer/mod.rs:398 clear_background_color`;
`code_review_view.rs:6411 set_pane_id`; `context_chips/mod.rs:63 as_git_diff_stats`;
`context_chips/directory_fetcher.rs:144 change_directory`;
`env_vars/env_var_collection_block.rs:201 get_block_id`; `terminal/model_events.rs:59
set_active_session_id`; `terminal/warpify/trigger_state.rs` (6 fns);
`terminal/warpify/settings.rs:466 remove_denylisted_ssh_host`; `terminal/find/model/async_find.rs`
(`unregister_rich_content_view`, `matches_for_ai_block`); `terminal/input.rs:5512
workflows_info_box_open_workflow_cloud_id`.

### 2c. UNGATED product fns whose only callers are tests — ~145 lines (+ test sites)
Delete with their tests or `#[cfg(test)]`-gate like their siblings (`clear_buffer_for_testing` is
the gated example): `terminal/history.rs:287 command_at_time`, `:343 for_completed_block`;
`terminal/input.rs:724 should_preserve_input`; `workspace/view.rs:11349
set_active_terminal_input_contents_and_focus_app`; `ai/blocklist/history_model.rs:434
set_server_conversation_token_for_conversation_and_persist`; `ai/agent/conversation.rs:497
status_error_message`; `terminal/cli_agent.rs:310 supports_bash_mode`; `pane_group/mod.rs:885
has_terminal_panes`; `pane_group/working_directories.rs:394 clear_selected_review_repo`;
`search/slash_command_menu/static_commands/mod.rs:196 argument_hint`;
`terminal/model/mod.rs:206 SizeUpdate::from_cell_dimensions`; `terminal/mod.rs:353
SizeInfo::with_rows_and_columns`. (Snapshot items are superseded by §1.)

### 2d. Machine-sweep follow-up
The sweep produced 739 zero-external-ref pub-fn candidates in the 21 biggest module trees; §2b is
the ~52-item hand-verified subset. The tail likely holds more of the same shape but needs
per-item triage for trait-impl obligations (`BackingView`/`TypedActionView` methods must keep the
trait signature even when unused).

### 2e. Wiring sweeps that came back CLEAN
All 177 `define_settings_group!` fields read somewhere; all 107 keymap context flags both set and
matched; all 25 `add_action` registrations dispatched; all 36 slash commands have executors; no
warp_features flag is dead (all 137 variants checked). No stale keybinding/menu/palette rows.

## 3. UI/render crates — ≈270 lines

- `crates/editor/src/content/text.rs:514,508` `BufferBlockItem::{as_plain_text_sliced, as_plain_text}` chain (12); `:1013 IndentUnit::char_unit`; `:1415 TextStyles::get_weight` (24)
- `crates/editor/src/selection.rs:488 has_pending_selection` (6)
- `crates/editor/src/render/element/mod.rs:287` trait method `is_embedded_comment` + its only override (`app/src/code/editor/embedded_comment.rs:181`) — zero call sites repo-wide (12)
- `crates/warpui_core/src/elements/gui/debug.rs` — ENTIRE FILE (128) + 4 cfg-gate lines: `DebugElement`, `DebugOptions`, trait `Debug`; `#[cfg(debug_assertions)]`-gated but invoked nowhere, not even in debug builds
- `crates/warpui_core/src/core/app.rs:4550 view_ids_for_window` (7)
- `crates/warpui_core/src/keymap.rs:356-366` trait `ActionArg` + blanket impl — never used as a bound (11)
- `crates/warpui_core/src/accessibility.rs:209-220` 6 never-constructed `WarpA11yRole` variants (`CheckboxRole`, `ImageRole`, `LinkRole`, `ListRole`, `ScrollareaRole`, `TextfieldRole`) (12, MEDIUM — could be deliberate vocabulary)
- `crates/warp_completer/src/meta.rs:204-241` traits `IntoSpanned`/`HasFallibleSpan` + 4 impls, zero call sites (38)
- `crates/warp_completer/src/parsers/errors.rs:89,108` `ArgumentError::{MissingMandatoryFlag, InvalidValueForFlag}` (4)
- `crates/warpui_extras/src/secure_storage/mod.rs:132` `SecureStorageError::InvalidLocation` (cfg windows, no `#[from]`) (3)
- Borderline test-support: `size_constraint_switch.rs:23-29` `HeightLessThan`/`SizeSmallerThan` (+ private `Size`) constructed only by their tests (~25 incl. tests); `editor text.rs` `colliding_style` ×2; `Edit::resolved_length` (~9 + test updates)

## 4. Data/platform crates — ≈430 lines

- persistence: 9 dead tables (§1) + `model.rs:587 NewMCPServerPane` — never constructed; `mcp_server_panes` is only ever DELETEd from (6); `model.rs:839 tasks_are_restorable` is pub needlessly (visibility-only fix)
- `crates/cloud_objects/src/ids.rs:199 ServerId::sqlite_type_and_uid_hash` (5)
- `crates/cloud_object_models`: `workflow.rs:245 Argument::with_default` (6); `ai_fact.rs:23-33` `Display`/`From<String>` for `SuggestedLoggingId` (11, MEDIUM)
- `crates/settings`: `manager.rs:244 default_values_for_settings_file` + `SettingsInfo.{file_serialized_default_value, max_table_depth}` + the `register_setting` macro params feeding them — one connected removal (~33); `lib.rs:149 SupportedPlatforms::WEB` (2)
- `crates/settings_value`: `lib.rs:99 impl_snake_case!` macro (6); `HashSet` SettingsValue impl — no setting uses HashSet (12, MEDIUM)
- `crates/ipc`: `client.rs:121 wait_for_disconnect` (3); `InitializationError::UnsupportedPlatform` in both client and server enums (4)
- `crates/warp_core`: `interval_timer.rs:46 compute_duration_for_interval` (17); `compute_stats` + `TimingDataPoint` (45, test-only); `channel/state.rs:152 mcp_oauth_provider_by_client_id` (12); `set_app_version` + `APP_VERSION` static (7, MEDIUM); `cli_agent_protocol.rs:10 WARP_CLI_AGENT_PROTOCOL_VERSION_ENV` const (3 — bootstrap scripts export the env var, nothing reads it via this const)
- `crates/warp_util`: §1 path items + `standardized_path.rs:48 try_with_encoding` (12); `content_version.rs:25 from_raw` (6, test-only)
- `crates/repo_metadata`: `repository.rs:349 watcher_count` (5); `local_model.rs:137 wait_until_indexed` chain (25); `local_model.rs:302 GetContentsArgs::include_ignored` (6, test-only); `standing_queries.rs:217 as_snapshot_delta` (13)
- `crates/command`: `blocking.rs:745 kill_on_parent_process_close` (cfg windows, 10); `windows.rs:68 JobObject::assign_process` (7)
- `crates/warp_cli`: `lib.rs:41 is_worker_invocation` (13, test-only); `agent.rs:136 parse_local_child_harness` (12, test-only — with it `parse_orchestration_harness` loses its last product caller)

## 5. AI/network crates — ≈700 lines

- `crates/ai`:
  - `src/index/locations.rs` whole module (38); `file_outline`: `to_file_symbols` + `FileSymbols` + `file_count` + `gitignores` + `to_string` (77); `index/mod.rs:7-8` dead re-exports (2)
  - `workspace.rs:69 WorkspaceMetadataEvent` enum (6); `:56 is_expired` (9)
  - `agent/task_state.rs` `AgentTaskState::{Claimed, InProgress, Succeeded}` variants (3)
  - `agent/harness.rs` `AgentHarness` enum — distinct from the live flag; app uses its own `agent_sdk::Harness` (8, MEDIUM — confirm)
  - `agent/action/mod.rs:338` unused generated `AIAgentPtyWriteModeDiscriminants` (1)
  - `agent/action_result/mod.rs:581 triggers_server_subagent` (17)
  - `diff_validation/mod.rs:662 find_similar_sections` + `section_similarity` (30); `:954-1009 mod test_util` (56 — app enables `ai/test-util`, so this is not gate-protected)
  - `gfm_table.rs:120 maybe_parse_gfm_table` (19); `project_context/model.rs:612 rules_for_workspace` (7)
  - `api_keys.rs`: `from_display_name` (10), `reload_keys_from_secure_storage` (9), `persist_provider_key`/`has_any_key`×2/`provider_key_count` (test-only, 48), `clear_custom_endpoints` (10)
  - `skills/parse_skill.rs:92 is_bundled` (3); `skills/conversion.rs:96 skill_reference_from_api_skill_ref` (test-only, 14)
- `crates/mcp`: loopback OAuth (§1); `sse_transport/client_side_sse.rs` `ExponentialBackoff`/`NeverRetry` — only `FixedInterval::default()` ever used (40, MEDIUM — rmcp-fork API surface)
- `crates/lsp`: `LspServerLogLevel::Warn` (2); `LspEvent::Idle` (1)
- `crates/jsonrpc`: `service.rs:133 set_server_request_handler` — with it the handler field/branch plumbing is unreachable (~12 direct, ~40 incl. plumbing)
- `crates/warp_terminal`: `shell/mod.rs:377,385 {and,or}_combiner` (16); `escape_sequences.rs:117 C1::DECID`, `:144 tmux_passthrough` (13); `indexing.rs:140,162 Point::{sub_absolute, add_absolute}` (45); `control_sequence_parameters.rs:246 NamedColor::to_bright` (24); `grid/cell.rs:18 DEFAULT_CHAR_STR` (1)
- `crates/input_classifier`: `util.rs:36 is_one_off_shell_command_keyword` (3); `InputClassifierDecisionSource::ShellCommandAllowList` variant (1)
- `crates/markdown_parser`: `parse_markdown_to_raw_text` (4); `FormattedText::is_empty_line` (3); `FormattedTextFragment::bold_italic` (test-only, 10)
- Clean: node_runtime, natural_language_detection, ipynb_parser, warp_ripgrep, watcher, warp_files, http_client, local_inference, isolation_platform.

## 6. allow(dead_code) audit — 255 sites

- **(a) STALE — item alive, remove the annotation (47 sites, 1-line cleanups).** Notables:
  `app/src/lib.rs:17 mod context_chips` — **correction vs plan-p4 §3: the module-level allow is
  doing real work for the ~25 dead items inside; keep it until those are cut, then drop it**
  (plan-p4 P4-3 already plans exactly this). Other confirmed-stale sites: `terminal/model/secrets.rs:1`
  (file-level; `SECRETS_REGEX` used by `secret_redaction.rs`), `terminal/find/mod.rs:1`,
  `terminal/find/model.rs:4`, `editor/view/model/display_map/mod.rs:332 anchor_after`,
  `workspace/view.rs:3711,10236` (unused_variables on arms that DO use the variable),
  `view_components/dropdown.rs:359 set_style` (item itself dead — see (b)), `dismissible_toast.rs:338
  with_href`, `compactible_split_action_button.rs:19,29`, `action_button.rs:374 with_width`,
  `:389 with_height`, `menu.rs:2415 Menu::add_item`, `undo_close/stack.rs:173 is_empty`,
  `context_chips/mod.rs:31 pub use` (partially; see (c) suspicious), `server/ids.rs:4 pub use`,
  `ai/blocklist/mod.rs:76 pub use`, `ai/agent/mod.rs:818 ProgrammingLanguage`,
  `ai/agent/conversation.rs:84 AddedExchange.task_id`, `ai/mcp/templatable_manager.rs:284,286`
  (both variants constructed), `settings_view/appearance_page.rs:555 thin_strokes_dropdown`,
  `code/editor/scroll.rs:8 AlwaysHandle`, `:15 should_handle`, `code/editor_management.rs:241,298`,
  `terminal/view.rs:2156 active_session`, `terminal/mod.rs:430 ClipboardType`,
  `terminal/model/ansi/mod.rs:90 xparse_color`, `terminal/model/block.rs:314 background_executor`,
  `terminal/view/docker_sandbox/mod.rs:28 sbx_path`, `terminal/input.rs:267 use {…}`,
  `integration_testing/agent_mode/assertions.rs:876`, `crates/ai skills/parser.rs:10,23`,
  `crates/warp_completer parsers/errors.rs:1` (file-level; inner fns need a clippy pass when
  removed), `completer/suggest/test.rs:34 TEST_ROOT_DIR`, `crates/sum_tree cursor.rs:228 Cursor::prev`,
  `crates/warp_terminal control_sequence_parameters.rs:475 StandardCharset::map`, `grid/cell.rs:339
  Cell::flags`, `crates/warpui platform/mod.rs:46 AsInnerMut`, `new_scrollable/mod.rs:1`
  (file-level, module alive), `windowing/winit/delegate.rs:1` (file-level),
  `crates/warp_core ui/theme/color.rs:447 accent_pressed`, `crates/editor text.rs:890 cache field`,
  `crates/integration user_defaults.rs:7 input_mode`.
- **(b) DEAD — deletable (46 sites, ~1,750 lines; §1 headline items included).** Rest:
  `app/src/test_util/virtual_fs.rs` whole file (59); `crates/virtual_fs`
  `git_repository_fixture`/`Warp::executable`/`Warp::fixtures` (28); `crates/sum_tree`
  `SumTree::{first, insert}` then `enum Edit` (40 — Edit's last constructors are these two plus
  the dead snapshot module); `crates/warp_search_core`
  `insert_document`/`delete_document`/`get_all_documents`/`get_all_doc_ids`/`add_search_field`/`add_id_field`/`FullTextSearchFieldTypes`
  (73 with tests); `ai/mcp/mod.rs:119 items_to_hashmap` (12; = plan-p4 §4);
  `agent_sdk/driver/terminal.rs:357 current_directory` (15; = plan-p4);
  `global_search/view.rs:1492 get_or_create_directory_entry` (19; = plan-p4);
  `alert.rs:163 Alert::success` + `Success` variant (4; = plan-p4); `action_button.rs:332
  with_callout` (4); `filterable_dropdown.rs:217 reset_selection` (8); `terminal/view.rs:907
  last_banner_id` (8; = plan-p4); `lib.rs:287 LaunchMode::add_url` (7; = plan-p4);
  `CodePaneData.{window_id, locator}` written-never-read (4; cf. plan-p4's `action_id` fields);
  `ai/agent/api.rs:77 coding_model` field (2); `terminal/input.rs:121 use crate::ASSETS;` (1);
  `input.rs:1389 ai_follow_up_icon_mouse_state` (2; = plan-p4);
  `code_review_view_tests.rs` dead test helper `create_editor_with_diff` + unused `window_id` (31);
  `MAX_EXCHANGE_DURATION` (1); `blocklist/mod.rs` two unused re-exports, one with a comment still
  claiming a TUI frontend (3); `formatted_text_element.rs:327 with_alignment`, `:1342
  LaidOutTextFrame::contains` (10; contains = plan-p4 §5);
  `warpui_core/src/platform/test/app.rs` **`App::new`/`run` stubs only** — the rest of
  `platform/test` is alive (`FontDB`/`AppDelegate`/`WindowManager` have callers; resolves the
  plan-p4 §5 "used by tests" caution) (25); `warpui frame_capture.rs:96 create_capture_texture`
  (28; = plan-p4).
- **(c) NECESSARY (~162).** Platform cfg-gates with real other-platform callers (~60), macro
  (~14), serde/persistence/schema keeps, event payloads constructed-but-never-read, documented
  TODOs, test infra. Conflicts with plan-p4 §4/§5 resolved in §8.
- **Suspicious keeps worth a decision:** `CodeManagerEvent` is emitted (`complete_pending_diffs`)
  but has **zero subscribers** — the whole pending-diffs event path looks inert;
  `context_chips` re-export of `ChipDisabledReason`/`ExternalCommandsAvailability` has no users;
  `font_handle.rs:95 data()` has no callers under its gate.
- **#[deprecated]:** `menu.rs:1498 MenuItem::submenu` — zero callers, deletable (6);
  `MCPContext.resources/.tools` intentional.
- **Product-reachable `todo!()`/`unimplemented!()`** (dead/incomplete, not deletable):
  `terminal/bootstrap.rs:294 PowerShell => todo!()` (PLAT-750), `local_tty/shell.rs:739` WSL
  bootstrap, `lsp/servers/rust.rs:52`, `pane_group/mod.rs:3595` banner actions,
  `terminal/view.rs:15902` (debug-only).

## 7. Dead assets / fixtures / config — ~22.4 MB, ~60 files

- `app/assets/async/png/` whole dir (12 Warp-marketing images, 7.8 MB); 28 dead SVGs in
  `app/assets/bundled/svg/` (132 KB; incl. 8 `referral-*`, `warp.svg` — distinct from the used
  `script/warp.svg`); `bundled/bootstrap/subshell_bootstrap_block_command.txt` +
  `legacy_remote_subshell_bootstrap_block_output.txt`; 6 Windows `.pdb`/`.lib` stubs
  (~24 KB; build.rs/installer copy only the 4 named DLL/EXE files)
- `crates/integration/tests/data/cloud_objects.sqlite` (14.3 MB; zero refs, siblings all
  referenced) + 8 git-tracked `*-shm`/`*-wal` WAL byproducts (128 KB; fixtures should be pristine)
- `docker/linux-dev/` (plan.md already claimed it removed); `wayne5a-progress.md` at repo root
  (personal log — delete or move out); `.github/workflows/ci.yml:372` stale comment referencing
  the deleted `warp_tui` crate
- MEDIUM: `app/assets/async/jpg/mountains.jpg` — only `theme_tests.rs:464` uses it (cut only with
  that test retargeted)
- Verified CLEAN: all fonts, bootstrap scripts, `bundled/svg/file_type/*` (20/21), stretchable
  glyphs, base16 theme, `bundled/png/*`, mac/linux/windows bundle resources, bundled skills
  (loaded by directory), `.warp/workflows/*.yaml`, channel icons, nextest config, all script/CI
  paths, all 3 WGSL shaders.

## 8. Reconciliation with plan-p4.md

- **Dependencies:** plan-p4 §1 is the superset (its machete pass covered every crate; mine was
  truncated) — execute deps from plan-p4 §1 only. My per-dep grep verification independently
  confirmed the `warp`/`ai`/`cloud_object_persistence`/`node_runtime`/`warpui*`/`warp_cli`/
  `warp_terminal`/`editor`/`command` rows, including the same three false-positive keeps
  (`serde_bytes`, `serde_regex`, `asset_cache`-via-reexport; plan-p4 also lists
  `blocking`-via-`command::blocking`).
- **Agreement (high confidence):** snapshot module, context-chips inner items, `items_to_hashmap`,
  `get_or_create_directory_entry`, `LaunchMode::add_url`, `last_banner_id`,
  `ai_follow_up_icon_mouse_state`, `Alert::success`, `current_directory`,
  `LaidOutTextFrame::contains`, `create_capture_texture`, `formatted_text_element` items.
- **Resolved conflicts:**
  - `platform/test` module: alive (FontDB/AppDelegate/WindowManager used); only its `App` struct
    is dead. Cut the struct, keep the module.
  - `capture_recorder.rs`: dead on all features (grep), despite plan-p4's caution. Cut whole file.
  - context_chips module-level allow: keep until plan-p4 P4-3 lands, then drop.
- **Items plan-p4 lists as cuts that this audit classifies as keeps/decisions:** `telemetry_event
  Cancelled` (serde wire shape — decision), `templatable_manager uuid` fields / `queued_query
  query_id` / `dropdown Naked` / `SkipUndoRedoRecord` / `shell_terminated_banner Normal` /
  `gutter_button CommentButton` (constructed-but-never-read event payloads and match-exhaustiveness
  variants — deletable only together with their match arms; MEDIUM), `skill_manager skill_watcher`
  / `edit_page database_connection` (handle-lifetime keeps), `secrets set_is_obfuscated` /
  `RangeMapPoint::as_point` / `async_find work_queue` methods / `editor line_at_vertical_offset` /
  `scroll {OnlyHandleOnFocus, NeverHandle}` / `gutter_button Disabled` / `editor_management
  action_id` fields / `agent_conversations_model ServerToken` / `radio_buttons::new` (from the
  probe; individually plausible cuts but not hand-verified here — triage in the same round).
  `image_cache evict_size` (TODO APP-3877) and `cli_agent_sessions` logging fields: keep.

## 9. Merged queue

| Round | Item | Source |
| --- | --- | --- |
| P4-1 | Unused dependencies | plan-p4 §1 (superset; both passes agree) |
| P4-2 | Delete `code/file_tree/snapshot` (+ `sum_tree::Edit` cascade) | both |
| P4-3 | Context-chips cluster, then drop the module-level allow | plan-p4 §3 |
| P4-4 | App items: §2a action variants, §2b pub fns, §2c test-only fns, plan-p4 §4 (minus §8 keeps) | both |
| P4-5 | §1 capture_recorder + §6(b) annotated deletions (test_util/virtual_fs, sum_tree chain, warp_search_core, platform/test `App`) | this pass |
| P4-6 | §3 + §4 crates rounds | this pass |
| P4-7 | §5 ai/mcp/jsonrpc round (loopback-OAuth decision, `diff_validation::test_util`) | this pass |
| P4-8 | Assets/fixtures (§7), stale annotations §6(a), `MenuItem::submenu` | this pass |
| P4-9 | Decisions: CodeManagerEvent inert path; a11y role variants; `AgentHarness` enum; SSE retry policies; `HashSet` impl; `mountains.jpg`; §8 MEDIUM variants; §2d 739-candidate tail | user |

Verification per round as usual: `./script/format`, clippy (both feature sets), nextest. The
compiler surfaces newly-orphaned items after each round.

## 10. Execution log (2026-10-17)

All rounds landed on master, each verified with `./script/format --check`, the three presubmit
clippy gates (workspace excl. warp_completer, `-p warp`, `-p warp_completer`, all `-D warnings`),
the simplewarp-set gate where Cargo.toml changed, and targeted nextest; the full workspace suite
ran as the final gate. Subagents did the cuts; plan corrections are folded into this section.

| Round | Commit | What |
| --- | --- | --- |
| P4-1 | `cb99ec06` | 100 dependency entries removed across 26 manifests + 8 orphaned `[workspace.dependencies]` entries. Restored 6 deps that only macro expansion referenced (`strum`/`num-traits` under `strum_macros`/`num_derive`, `concat-idents` under `define_settings_group!`, `log` under `report_error!`); `rust-embed` stays (referenced by the `release_bundle` feature); `futures-lite` moved to command's dev-deps for its doctests. |
| P4-2 | `1967fc71` | snapshot module (913) + `sum_tree::Edit`/`SumTree::edit` (45). `KeyedItem` left impl-less for P4-5. |
| P4-3 | `68e98d24` | context-chips cluster, −477 net in 21 files; module-level allow removed and stays removed; unused chip-event forwarding chain cut through display/input/terminal/palette. `Prompt::set_config_for_test` added for a test that relied on the deleted eager `update`. |
| P4-4a | `43c4d6e6` | 29/29 action variants, ~380 lines incl. cascades (blocklist permission removers, `SettingsPageMeta::on_tab_pressed`, `UnsavedChangeType::ForEdit`). `PromptDisplayAction`/`AIFactViewAction` remain as empty enums (trait obligations). |
| P4-4b | `e8d058b4` | 48 dead pub fns, ~670 lines incl. the whole `TuiTerminalManager`. **Kept against the plan:** `get_all_aliases` (its tests are the only reader; plan claim wrong) and `set_active_session_id` (already `#[cfg(test)]`-gated). `QueuedQuery::new` gated instead of deleted (7 test sites). |
| P4-4c | `9635558d` | §2c: 5 fns `#[cfg(test)]`-gated, 6 deleted with their tests. plan-p4 §4 probe items cut (scroll/gutter variants, work_queue methods, `ServerToken` subject, event uuid/tuple payloads, `coding_model` field); §8 keeps stand. **`items_to_hashmap` is alive** (product callers added by the MCP round) — plan-p4 §4 was stale. |
| P4-5 | `fae5b0ea` | ~980 lines: capture_recorder.rs whole, test_util/virtual_fs.rs whole, sum_tree `KeyedItem` chain (trait + impl block + insert/first/leftmost_leaf), warp_search_core document-dump API, platform/test `App` stubs, `MenuItem::submenu`, app leftovers. **Kept against the plan:** `FormattedTextElement::with_alignment` (4 live callers) and `FullTextSearchFieldTypes` (feeds `define_search_schema!`). |
| P4-6 | `feba1283` | UI + data crates: editor/warpui_core/warp_completer dead traits & helpers, `elements/gui/debug.rs` whole, 9 persistence tables + `NewMCPServerPane`, settings file-default plumbing, ipc disconnect plumbing, warp_core timer/oauth statics, warp_util `common_path`, repo_metadata chains, command windows builders, warp_cli harness parsers. **Kept against the plan:** `WslUncPath`/`parse_wsl_unc_path` (live callers in `git.rs`); `wait_until_indexed` chain (sole wait-primitive for 9 view tests — test-infra round candidate); `max_table_depth` trait method (live; only the dead manager plumbing went); `SizeSmallerThan`+`Size` (tests pin AND-semantics of a live element). |
| P4-7 | `38288332` | ai dead chains (index locations/file_outline/api_keys/diff_validation test_util incl. removing the whole `ai/test-util` feature), `AgentHarness` enum (the flag and `agent_sdk::Harness` stay), mcp loopback OAuth (DCR test converted to custom-scheme), jsonrpc handler plumbing, warp_terminal helpers. SSE retry policies kept (vendored rmcp file — fork-API surface). |
| P4-8 | `b18090d4` | 61 files / 22.6 MB: async/png dir, 28 SVGs (incl. `referral-hydroflask.svg`), 2 bootstrap txts, 6 Windows pdb stubs, `cloud_objects.sqlite` + 8 WAL sidecars, `docker/linux-dev/`, ci.yml stale comment. warp_core `McpOAuthLoopbackClientConfig` orphan cut. 37 stale annotations removed; 3 sites reverted (items genuinely dead: `server/ids.rs` `ServerId` re-export arm, `input.rs` unused import pair, `block.rs background_executor`); `new_scrollable` file-level allow replaced by 3 targeted ones on its dead `scroll_offset` methods. |

Follow-ups found en route (decision-round candidates): `set_callout` (zero callers),
`EntrypointType::AgentInitiated` (unconstructed since P4-4b but serde wire vocabulary),
`ContentVersion::{as_i32, as_u64}` (pub, now caller-free), `containing_pane_id`/`ssh_warpify_timeout_handle`
(read but never set to Some), the 3 `new_scrollable` `scroll_offset` methods, the §6(a) reverted
three, and everything in the P4-9 decision list (CodeManagerEvent inert path, a11y role variants,
SSE retry policies, `HashSet` impl, `mountains.jpg`, the 739-candidate pub-fn tail).
`wayne5a-progress.md` was left alone (user's personal log, not repo dead weight).

## 11. P4-9 decision-round log (2026-10-18)

Every P4-9 item was re-verified individually; confirmed-dead ones were cut, the rest have
recorded evidence for staying. Commits, all on master:

| Round | Commit | What |
| --- | --- | --- |
| P4-9a | `d5781f18` | Quick confirmed-dead cuts: `ServerId` re-export arm, unused import pair, `scroll_offset` trio, the `set_callout` chain (field provably always `None`; `Callout` enum + `callout_padding` went too), `ContentVersion::{as_i32, as_u64}`, the `Warp` struct (empty husk), `containing_pane_id` + `ssh_warpify_timeout_handle` chains. **Kept against the claim:** `background_executor` — read by the `cfg(not(test))` `wakeup_after_delay()`; dead only under test compilation, annotation needed. |
| P4-9b | `73671fbe` | 6 a11y role variants; warp_completer parser error constructors (file-level allow now fully removed); `EntrypointType::AgentInitiated` ( Serialize-only in-memory metadata — no tombstone needed); `EditCancelled.query_id`; never-constructed `TerminationType::Normal`; outbound-only `GitDialogStatus::Cancelled`. **Kept:** `SkipUndoRedoRecord` — `skip_undo()` matches it by name and its behavior differs from every other variant (test-pinned). |
| P4-9c | `a2ac2e8d` | `CodeManagerEvent` proven zero-subscriber on all features: enum + emit + AIAction gate deleted, `complete_pending_diffs` keeps its non-event work, `type Event = ()`. SSE `ExponentialBackoff`/`NeverRetry` cut (documented divergence from the vendored rmcp file). `mountains.jpg` deleted, palette test retargeted to `pink_city_bg.jpg` with constants re-derived empirically. `wait_until_indexed` chain gated behind the existing `repo_metadata/test-util` pattern (3-line diff). `SizeSmallerThan` + `Size` deleted with their 3 self-only tests (the tested code is the deleted code). |
| P4-9d | `7e536758` | Fixpoint sweep of app/src definition-line-only pub fns: 125 deleted + 11 private cascades + `MenuItemLabel::{MultilineText, StackedText}` variant cascade, `ContainerConfiguration` collapse, `extract_fields!` macro, 3 husk files (~2,280 lines). `cargo check --features integration_tests` run separately. Re-scan: 0 candidates remain. New follow-up: `MenuItemFields.timestamp` never set to `Some`. |

Still intentionally kept (evidence recorded above): `SkipUndoRedoRecord`, `background_executor`
annotation, `SizeConstraintCondition` single-variant enum shape, the vendored-file doc comment on
SSE. P4-9 closed except `wayne5a-progress.md` (user's personal log, untouched by decision).
