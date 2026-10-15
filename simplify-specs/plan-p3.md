# SimpleWarp — Plan, part 3: the dead-code audit

Survey date: 2026-10-11, HEAD `87a6a158`. Six parallel sweeps (cloud objects/persistence,
server/auth/network, AI surface, flags/settings/actions, workspace crates, misc UI), every HIGH
claim re-verified by hand with `rg`. A `cargo check --no-default-features --features simplewarp
--lib --bins` over the touched crates emitted **zero dead_code warnings**, which is itself a
result: whatever is left dead is invisible to rustc (cross-crate pub API, data, UI surfaces) —
exactly what this audit found.

Scope rule (unchanged from plan-p2): a feature is deleted only if it **requires a remote
service**; local-but-disabled features are enable candidates, decided per feature. Everything
below keeps the simplewarp build capable of every non-remote feature. Compat-only shapes
(persisted/parse-only) are listed in §7 and stay.

Headline numbers: ~8,000–10,000 lines reclaimable, **4 crates deletable outright**, plus one TS
subproject. And one real bug found along the way (§0).

## 0. Real bug found during the audit (fix first, not a deletion)

`update_object_metadata` (`crates/cloud_object_persistence/src/objects.rs:393`) filters
`server_id.eq(Some(hashed_id))`, but the only caller passes `SyncId::sqlite_uid_hash()` =
`"Client-{uuid}"` (`app/src/server/cloud_objects/update_manager.rs:103` →
`app/src/persistence/sqlite.rs:456-458`). It was the RTC server's `ObjectMetadataUpdated`
endpoint; post-fork the filter can never match. **Consequence: trash/untrash never persists
`trashed_ts` / `folder_id` / `current_editor` — the state is lost on restart.** Content edits are
unaffected (`upsert_cloud_object` correctly filters `client_id.or(server_id)`,
`objects.rs:162-165`). Fix: filter on `client_id` (one line). This also retires the
`server_id`-column reads in `upsert_cloud_object` :164 and `delete_cloud_object` :290 as
harmless dead reads (pre-fork rows only).

## 1. Whole-crate / whole-subsystem deletions

| Target | Size | Evidence |
| --- | --- | --- |
| `crates/warp_js` + the whole plugin_host subsystem | ~2,700 lines | `plugin_host` (`app/Cargo.toml:710`) is pulled only by `completions_v2` (:385), and neither is in `default` (:391) nor `simplewarp` (:515) — 0 hits, verified. With it: `app/src/plugin/` (1,262 lines, `#[cfg(feature = "plugin_host")]` at `app/src/lib.rs:43`), `app/src/completer/js.rs` + the `js_ctx` cfg branches, `WorkerCommand::PluginHost` (`crates/warp_cli/src/lib.rs:208-213`), `rquickjs`. See §6 for the `warp_completer` v2 signature modules that ride on the same gates. |
| `crates/command-signatures-v2` + root `command-signatures-v2/` | 60 lines Rust + a yarn/TS subproject | Only referenced via `completions_v2` → plugin-host (`app/src/plugin/host/native/plugin_ref.rs:27`). Do not confuse with the live git dep `warp-command-signatures` (`app/Cargo.toml:260`). |
| `crates/http_server` | 82 lines | Started unconditionally when not headless (`app/src/lib.rs:1302-1306`), binds 127.0.0.1:9282, and its router's only route is behind `jemalloc_pprof` (`app/src/profiling.rs:220-227`) — a feature in neither set, so every GUI launch runs an empty listener + a 1-worker tokio runtime. The MCP OAuth loopback receiver is independent (`crates/mcp/src/oauth.rs:90`). |
| `crates/prevent_sleep` | 255 lines | Only dependent is `http_client`; its guard arms on `prevent_sleep_reason`, set only by `RequestBuilder::prevent_sleep()` / `eventsource()` — zero external callers (local_inference streams SSE via `reqwest_eventsource` on a plain `reqwest::Client`, `crates/local_inference/src/stream.rs:10,50`). |
| `crates/warp_cli/src/scope.rs` | 13 lines | `ObjectScope` (`--team`/`--personal` Drive scoping): zero references repo-wide. |
| `SecureSetting` trait + `SecureStorage::write_value_with_owner_only_fallback` | ~75 lines | Zero implementors / zero callers (`crates/settings/src/lib.rs:546-616`, `crates/warpui_extras/src/secure_storage/mod.rs:96-101`); the keychain path is fully live through `AiApiKeys` and MCP OAuth only. |

In the same pass, strip `http_client`'s dead builder API (~200 lines, zero external callers):
`proto`, `multipart`, `form`, `basic_auth`, `bearer_auth`, `put`, `patch`, `delete`, `post`,
`timeout`, `eventsource`, `prevent_sleep`, and `impl oauth2::AsyncHttpClient for Client`. The
live surface is `get`/`header`/`send` + `Response::{status,text,json,bytes}` + the request/response
hooks (network logging) — all of it used by the LSP install path (kept, P12).

Also delete the dead `warp_features` API: `set_user_preference` / `USER_PREFERENCE_MAP` /
`RUNTIME_FEATURE_FLAGS` (zero callers; the runtime-flags menu is always empty) and the
zero-gate `CrossRepoContext` flag (enum def `crates/warp_features/src/lib.rs:154`, DOGFOOD list
:522, cfg mapping `app/src/features.rs:109` — no `.is_enabled()` site anywhere).

## 2. Cloud-object / persistence residue

Dead, no compat value (HIGH):

- Generic-string-object server machinery with zero consumers: `uniqueness_key` /
  `GenericStringObjectUniqueKey` / `UniquePer` (`crates/cloud_objects/src/cloud_object/mod.rs:128-142`)
  + `StringModel::uniqueness_key` and its 6 impls + `should_clear_on_unique_key_conflict`;
  `StringModel::should_enforce_revisions` + 5 impls; `CloudModelType::supports_linking` +
  `StringModel::supports_linking` + impls (~95 lines total). Revision conflicts, uniqueness and
  link-sharing were the server's job.
- `SerializedNotebook` (`crates/cloud_object_models/src/notebook.rs:9-14`) + the dedicated
  `serialized()` bodies for Notebook/Workflow/Folder (`app/src/notebooks/mod.rs:27-35`,
  `app/src/workflows/mod.rs:147-150`, `app/src/cloud_object/folders.rs:44-46`) — the only live
  `serialized()` goes through `GenericStringModel`. Plus `CloudNotebookModel.conversation_id`
  (always `None`, never read) (~40 lines).
- The `AIExecutionProfile` **cloud-object wrapper**: `StringModel` impl
  (`app/src/ai/execution_profiles/mod.rs:102-160`), `JsonModel` impl + aliases
  (`crates/cloud_object_models/src/ai_execution_profile.rs:338-346`), loader arm
  (`json_model/persistence.rs:108-120`), `drive_object_type.rs:52`. Profiles persist via
  `ExecutionProfilesConfig` settings now; loaded cloud rows have zero readers. The
  `AIExecutionProfile` struct itself is alive — only the wrapper dies (~90 lines).
- `CloudObjectEventEntrypoint` (`crates/cloud_objects/src/cloud_object/mod.rs:327-338`),
  `CloudObjectTypeAndId::{drive_row_position_id, as_folder_id}`, `CloudModel::get_all_active_folders`,
  `CloudFolderModel::new` (~80 lines).
- `From<PlanArtifact> for Artifact` (`app/src/ai/artifacts/mod.rs:202-218`) +
  `artifact_from_fork_proto` (`app/src/ai/agent/conversation.rs:293-306`) + the
  `Event::ForkArtifacts` streaming arm — the local adapter never emits artifact events and the
  restore path replays them as `NoClientRepresentation`. This is the last value-producer of
  `NotebookId` (~85 lines).

**Correction to plan-p2 §9.2:** `JsonObjectType::MCPServer` is **not dead** — old rows still load,
convert to templatables at startup, show in Settings and are deletable
(`app/src/ai/mcp/templatable_manager/native.rs:1371-1401,515-533`,
`settings_view/mcp_servers/list_page.rs:191-236`). Keep. The genuinely dead variants are
`CloudEnvironment` / `ScheduledAmbientAgent` / `CloudAgentConfig` (compat tombstones, ~25 lines —
deleting them changes nothing since the loader skips those rows anyway).

Compat-only (keep): the ServerId-backed marker types (`NotebookId`, `WorkflowId`, `FolderId`,
`GenericStringObjectId`) are the `IdType` of the `GenericCloudObject<K, M>` machinery — removing
them is a rename-to-`SyncId` refactor, not a deletion; `server_id_traits!`'s value-level API dies
with §2's `From<PlanArtifact>` chain. Tombstone columns stay as-is (`current_editor_uid`,
`AIMemory.is_autogenerated` + `suggested_logging_id`, `Window.team_uid`, `ObjectMetadata`
constants, `is_warp_pack`, `ObjectPermissions` NULL columns). The AmbientAgent pane persistence
chain (`LeafContents::AmbientAgent` write path, ~70 lines) is unreachable except as old
session-config compat — tombstone-grade, decide later.

## 3. Server / auth / network residue

- **The ledger note is obsolete:** the AIClient/AuthClient/ServerApi/BaseClient client chain no
  longer exists (`rg "struct AIClient|impl AIClient|struct BaseClient" ` = zero). The only
  remaining `AuthClient` is `rmcp`'s, in the user-configured MCP OAuth path (keep).
  `ServerApiProvider` hands out the shared `http_client` to the LSP install path only (6 call
  sites) — keep, minus §1's dead API. Its doc comment still claims "standard Warp request
  headers" (`app/src/server/server_api.rs:154`).
- **`AIRequestUsageModel` is a fake quota model** (`app/src/ai/request_usage_model.rs`):
  `request_limit_info` is always default, `has_any_ai_remaining()` is literally `true` (:115-119).
  Dead around it: `RequestLimitRefreshDuration`, the codebase-indexing fields,
  `new_for_evals`, `should_offer_default_model_switch` + its two call sites
  (`settings_view/warp_agent_page.rs:776-779,814,845`), and the always-false
  `!has_any_ai_remaining()` guards at `terminal/view.rs:7540`, `terminal/input.rs:4322`,
  `code_review/comment_list_view.rs:291,905,913` (incl. the "requires AI credits" tooltip :939).
  ~90 lines + 6 guard sites. **Tie to P7**: either delete now or fold into P7 (map provider token
  usage into `StreamFinished`) so the gates come back with real data.
- `AgentConfigSnapshot` dead fields: `worker_host`, `harness_auth_secrets` /
  `HarnessAuthSecretsConfig`, `profile_id`, `additional_source_repos`
  (`crates/cloud_object_models/src/agent_config_snapshot.rs:25-32`) — read only by `is_empty()`.
  The config file is `deny_unknown_fields`, so the `host` and `environment_id` **keys** must stay
  accepted; the struct fields can go (~30 lines).
- `MCPSpec::Uuid` + hidden legacy `--mcp-server UUID`: half-live (locally installed templatables
  resolve); the unresolvable-UUID path always errors. Keep the meaningful error; optional shrink.
- Compat keeps: `UriHost::Auth`/`Team` log-and-drop stubs (`app/src/uri/mod.rs:62-63,95-106`),
  the `"Account"` settings-slug mapping (`settings_view/mod.rs:304-307`), tombstone schema tables
  (teams/team_members/team_settings, `user_profiles.firebase_uid`,
  `current_user_information.email` — no diesel model touches them).
- Copy fixes: "should use Warp Business or Enterprise" (`warp_agent_page.rs:2352`), "public
  ambient-agent API" (`app/src/ai/agent_sdk/mcp_config.rs:11`), and drop
  `#[allow(dead_code)]` on `AmbientConversationStatus::Blocked` — plus the stale
  `#[allow(dead_code)]`s that are wrong in the other direction (§6).

## 4. AI surface residue (~3,500 lines, the largest coherent block)

Root constant: `AISettings::is_code_suggestions_enabled` returns literal `false`
(`app/src/settings/ai.rs:850-852`, plan-p2 P3). Everything below hangs off that or off the
adapter's fixed output shape (`crates/local_inference/src/emit.rs:249-257` — text, tool calls,
`StreamFinished::default()`; no usage/cost/suggestions/citations ever emitted).

1. **Passive-suggestion dead halves + SuggestPrompt + SuggestNewConversation (~2k lines, HIGH).**
   In `passive_suggestions/legacy.rs`: the unit-test-suggestion machinery
   (`should_generate_unit_test_suggestion` :459, `send_unit_test_suggestions_request`
   `controller.rs:1318-1360`, the `git show HEAD` spawn, future handles) and the passive
   code-diff machinery (`maybe_generate_passive_code_diff` :280-401,
   `send_passive_code_diff_request` `controller.rs:1273-1316`, timeout handles, the five
   `PASSIVE_CODE_DIFF_*` constants). Then `PromptSuggestionExecutor`
   (`action_model/execute/suggest_prompt.rs`, whole file), `SuggestedUnitTestsView` +
   accept/dismiss plumbing (`block.rs:2801-2977`), `suggest_new_conversation.rs` executor,
   `PassiveSuggestionTrigger::{CommandRun, FilesChanged, AgentResponseCompleted}` +
   `PassiveSuggestionTriggerType` + `EntrypointType::TriggerPassiveSuggestion`,
   `PromptSuggestion::coding_query_context` / `is_coding_query` / the banner hide-while-generating
   paths (`terminal/view.rs:7593+`). Restore paths are untouched → not compat. The **static**
   command-suggestion banner is live — don't touch it.
2. **`SlashCommandRequest::CreateEnvironment` + `AIAgentInput::{CreateEnvironment,
   StartFromAmbientRunPrompt}` (~230 lines, HIGH).** `/create-environment` no longer exists; the
   last emitters are gone; zero constructors repo-wide.
3. **The Suggestions pipeline (~200 lines, HIGH).** `Suggestions` + `extend`
   (`agent/mod.rs:2899-2938`), `existing_suggestions_for_conversation` and the
   every-request round trip (`RequestParams.existing_suggestions` → adapter ignores it), and a
   literal dead store at `block.rs:1820-1826` (builds `suggestions`, never reads it).
4. **Per-model pricing / spec / context-window (~800 lines, HIGH).** All three catalog producers
   (`app/src/ai/llms.rs:431-450,1150-1191`) set `spec: None`, `discount_percentage: None`,
   non-configurable context windows. Dead: `LLMSpec` + `model_spec_scores.rs` (whole file,
   "Intelligence/Speed/Cost" bars) + the `spec.is_some()` selector paths; the discount chip;
   `DisableReason` + its reconcile loop; `LLMUsageMetadata::request_multiplier`;
   `LLMContextWindow` + the profile context-window slider/editor
   (`agent_profiles_page.rs:1442,1881+`) + `context_window_limit_for_request` +
   `base_model_context_window_limit` (always 0). Keep the `ExecutionProfileFile`
   `context_window_limit` key as load-compat (`#[serde(default)]`).
5. **Always-zero credits/usage UI (~200 lines).** `/cost`, the usage footer's "Credits spent"
   rows, "Credits used" in the details panel, the ContextWindowUsageV2 chip panel — all fed by
   `update_conversation_cost_and_usage_for_request` which always receives `None`/`[]`. Product
   decision with P7: give it data first or hide the rows.
6. Unreachable `StreamFinished` handler arms: `Reason::ContextWindowExceeded`
   (`controller.rs:2154-2166`) and `Reason::QuotaLimit` — the adapter emits only
   Done/MaxTokenLimit/Other. Wire variants stay.
7. `AIAgentOutput::api_metadata_bytes` — only ever assigned `None` (server-telemetry relic).
8. Unoffered-tool entries and stale request settings in `api/impl.rs:233-263` /
   `:72-104` (`supports_suggest_prompt`, `supports_started_child_task_message`,
   `web_context_retrieval_enabled`, `research_agent_enabled`, `planning_enabled`…) — wire-compat,
   LOW value; the `RequestParams` fields can go with them.
9. **`pr-comments` bundled skill is a dead end**: it instructs the model to call
   `insert_code_review_comments`, which is not among the adapter's 7 supported tools — the call
   is silently dropped (`emit.rs:205-212`). Delete the skill (+ `scripts/fetch_github_review_comments.py`
   + the `icon_for_bundled_skill` arm) or implement the tool in local_inference. Same for the
   dogfood skills under `resources/channel-gated-skills` (`test-warp-ui` says "use the
   `computer_use` tool"; `verify-ui-change-in-cloud` spawns a cloud agent).
10. `mcp_context` is built and converted (~100 lines) on every request but the adapter never
    reads it — **Phase 3b decision item** (it becomes live when MCP tool calls land).
11. InsertReviewComments: live executor chain unreachable (tool unoffered), restore path alive —
    stays per plan-p2 §10 (compat).
12. Keep-list verified live: MCP tool-call plumbing, `TransferShellCommandControlToUser`,
    static suggestions, `ai_assistant/` (`AskAIType` + execution context), `agent_tips.rs`,
    `field_mask` (used by `task.rs:662,701` for message merges).

## 5. Flags / settings / actions

- 149 `FeatureFlag` variants: 148 have gating sites; the only zero-gate flag is
  `CrossRepoContext` (§1). `DOGFOOD_FLAGS`/`PREVIEW_FLAGS` feed only the schema generators;
  `PREVIEW_FLAGS` is empty; runtime additions are `DEBUG_FLAGS` only.
- **Progress-note correction:** EditableMarkdownMermaid, ImeMarkedText and ITermImages (and
  JupyterNotebookRendering, LocalDockerSandbox) are **already in the simplewarp set** — they are
  not enable candidates anymore (they remain off in the warp-oss `default` set only; pinned by
  `features::tests`).
- **31 constant-false flags** (cfg feature in neither set). Two families:
  - *Local features switched off* — per-feature decision to enable or delete flag + gated code:
    `CodeReviewScrollPreservation` (whole module `app/src/code_review/scroll_preservation.rs`),
    `NldPromptHistoryMatch` (3 sites), `CycleNextCommandSuggestion`,
    `PartialNextCommandSuggestions`, `ResizeFix` (8 sites), `SelectablePrompt`,
    `AlacrittySettingsImport`, `SequentialStorage`, `InBandGeneratorsForSSH`,
    `AIContextMenuCommands`, `CommandCorrectionsHistoryRule`, `RecordPtyThroughput`,
    `MaximizeFlatStorage`, `AgentViewPromptChip`, `LSPAsATool` (tool also unoffered).
  - *Dogfood-only*: `McpJsonTreeView`, `BoxDrawingGlyphs`, `ToggleBootstrapBlock`,
    `RemoveAutosuggestionDuringTabCompletions`, `FileGlobV2Warnings`, `RememberFastForwardState`,
    `CodeReviewScrollPreservation` (also here), `SshDragAndDrop`, `DragTabsToWindows` (RELEASE),
    `RunGeneratorsWithCmdExe`, `MSYS2Shells`, `Projects`, `PromptCacheExpiryWarning`,
    `RestorePromptOnInlineModelSelectorSearch`.
- Six settings with zero readers: `agent_mode_onboarding_block_shown`,
  `build_plan_migration_modal_dismissed`, `should_add_agent_mode_chip`,
  `notification_toast_duration_secs`, `did_add_handoff_chip_to_toolbar`, and
  `git_prompt_dirty_indicator` (its comment claims compat, but nothing reads it — wire it or
  delete it).
- Context flags (`app/src/settings_view/mod.rs:371-509`): 4 zero-reference constants
  (`GIT_PROMPT_CONTEXT_FLAG`, `SAME_LINE_PROMPT`, `LOG_OUT_WARNING_MODAL`,
  `IS_BLOCK_AI_SUMMARIES_ENABLED`) + 10 insert-only ones that no binding/predicate ever matches.
- **~30 never-dispatched action variants** (defined + Display + handler arm, zero construction
  sites): `WorkspaceAction` ×22 (incl. `NewTabInAgentMode` whose handler
  `add_terminal_tab_in_ai_mode` dies whole), `TerminalAction` ×7 (`InsertCommandCorrection`,
  `OpenWorkflowModalForBlock`, `HyperlinkClick`, `ShowWarpifySettings`, `StartLspServer`,
  `ToggleLongRunningCommandControl`, `OpenFilesPalette`), `PaneGroupAction` ×5, `EditorAction` ×3
  (`ImeCommit`, `CutAllLeft`, `ClearLines` — not the live
  `PlainTextEditorViewAction::ClearLines`), `FeaturesPageAction` ×2, `WarpifyPageAction::OpenUrl`,
  `ContextMenuAction::OpenWorkflowModal`, `InputContextMenuAction::SaveAsWorkflow`,
  `PlainTextEditorViewAction::Unindent`.

## 6. Misc UI

- **Zero-state prompt-suggestion chips are dead end-to-end (~280 lines, HIGH).**
  Install/Code/Deploy/Something else (`terminal/view/inline_banner/prompt_suggestions.rs:34-84`)
  — the only entry point `InputAction::InsertZeroStatePromptSuggestion` has zero dispatch sites,
  and `start_agent_mode_in_new_pane`'s `zero_state_prompt_suggestion_type` is `None` at every
  reachable call site. Dead with it: `NewWorkspaceSource::AgentSession` (orphan of the deleted
  create-environment deeplink), `ZeroStatePromptSuggestionTriggeredFrom`,
  `WorkspaceAction::{NewTabInAgentMode, FixInAgentMode}`, the `send_zero_state_prompt_suggestion`
  chain. The local `QueryWithCannedResponse` backend (plan-p2 P2) stays — it just has no UI
  entry anymore.
- `SystemStats` (`app/src/system.rs`): events with zero subscribers; replace with the two
  existing `log::info!` calls (~35 lines).
- Prompt-editor same-line-prompt section (`app/src/prompt/editor_modal.rs:563-624`,
  `#[allow(dead_code)]`, compiler-confirmed) + unreachable `EditorModalAction::
  {ToggleSameLinePrompt, SetWarpPromptSeparator}` + a per-open phantom dropdown (~120 lines).
- **Voltron:** `VoltronItem::AiCommands` has zero constructors, and ctrl-r dispatches
  `VoltronItem::History` which is never registered — a user-reachable no-op that logs and closes
  (`input.rs:9026 → 9060`, `voltron.rs:324-337`). Both files already carry the "remove voltron"
  TODO; only `Workflows` is live.
- Small: tips gamified-counter machinery, `RefreshConfig::{Periodically, OnFileChanges}`,
  `WorkflowsFocusState`, `BannerTextContent::plain_text`, `render_search_bar` in
  `ai_context_menu/view.rs`.
- Wrong `#[allow(dead_code)]` (the code is LIVE — remove the annotations):
  `app/src/quit_warning/mod.rs:32,251,421,428` and
  `app/src/search/ai_context_menu/{rules,commands}/data_source.rs`.
- Ledger corrections: `ToolPanelView::WarpDrive`/`DrivePanel` (the 4bg "live empty-views
  fallback" keep) no longer exist — fully deleted since. `Tab::WarpDrive` remains as a tested
  parse tombstone (correct).

## 7. Compat-only keep-list (do NOT treat as dead)

`ContextChipKind::AgentPlanAndTodoList`, `AgentToolbarItemKind::VoiceInput`, `Tab::WarpDrive`,
`UserQueryMode::Orchestrate`, the AutoCodeDiff restore-rendering family
(`is_passive_code_diff()` etc.), all `AIAgentCitation` variants (incl. the docs.warp.dev link),
the InsertReviewComments restore leg + `code_review/comments/*` + `PRCommentsV2`, persisted
`GalleryData`, `AIAgentAttachment::DocumentContent`, `GetRelevantFiles {}` tombstone,
`AIDocumentId`, `warp_cli::skill::SkillSpec`, `JsonObjectType::MCPServer`, profile-file keys
(`autosync_plans_to_warp_drive`, `context_window_limit`), agent-config keys `host` /
`environment_id`, `TipAction::WarpAI`, all tombstone sqlite columns/tables, Windows/Linux
platform code, `http_server`-independent MCP OAuth loopback, LSP install downloads,
docker sandbox, external secrets, git/gh operations.

## 8. Corrections to earlier docs

- plan.md / wayne5a-progress.md: "AIClient 34 methods, AuthClient 12 methods … blocked on
  feature rounds" — the whole client chain is gone; nothing left to survey there.
- plan-p2 §9.2: `JsonObjectType::MCPServer` is compat-alive, not dead (see §2).
- plan-p2 §4 / 4bg keep-note: `ToolPanelView::WarpDrive`/`DrivePanel` no longer exist.
- wayne5a-progress.md "Remaining enable-candidates: EditableMarkdownMermaid, ImeMarkedText,
  ITermImages" — all three are already enabled in simplewarp (§5).

## 9. Proposed queue

| ID | Item | Type |
| --- | --- | --- |
| R1 | §0 fix `update_object_metadata` to filter `client_id`; retire the dead `server_id` reads | bug fix |
| R2 | §1 crate deletions: `warp_js` (+ plugin_host surface), `command-signatures-v2` (+ root dir), `http_server`, `prevent_sleep` (+ `http_client` dead API), `scope.rs`, `SecureSetting`, `warp_features` dead API + `CrossRepoContext` | mechanical |
| R3 | §2 cloud-object machinery (~300 lines) + §5 dead actions (~30 variants) + dead settings/context flags | mechanical |
| R4 | §4 AI blocks: passive-suggestion halves + SuggestPrompt + SuggestNewConversation (~2k) → CreateEnvironment/StartFromAmbientRunPrompt/Suggestions (~430) → pricing/spec/context-window (~800) | large, flag-anchored |
| R5 | §6 zero-state chips (~280) + SystemStats + SLP section + Voltron arms + small items + annotation fixes | medium |
| R6 | Decisions: credits UI vs P7; `mcp_context` (Phase 3b); the 31 constant-false flags one by one; `pr-comments` skill; `warp_completer` v2 modules (~3,439 lines under `completions_v2`/`v2` — deleting changes the `-p warp_completer --features v2` test flow in AGENTS.md); AgentConfigSnapshot field trim (keep file keys parseable) | user decision |

Verification per round as usual: `./script/format`, clippy, nextest (excluding the 10 known
integration timeouts). After each round the compiler's dead_code warnings will surface any
newly-orphaned items.

## 10. Execution log (2026-10-12)

Rounds landed on master, each verified with format, `clippy --workspace --all-targets -D warnings`
(simplewarp set) and the warp lib nextest suite (3,8xx tests, 0 fail):

| Round | Commit | What |
| --- | --- | --- |
| R1 | `1282690eb` | §0 fixed: `update_object_metadata` filters `client_id`; regression test `update_object_metadata_persists_trash_state` fails without the fix; dead `server_id` reads dropped |
| R2 | `b4381d345` | `http_server`, `prevent_sleep`, `scope.rs`, `SecureSetting` + owner-only fallback write, `http_client` dead builder API, `warp_features` dead API, `RuntimeFeatureFlags` + `CrossRepoContext` flags |
| R3/R4/R5 part 1 | `c8dbec2dc` | §2 cloud-object machinery, `AIExecutionProfile` cloud wrapper, `PlanArtifact` chain, 6 zero-reader settings, 13 dead context flags, `CreateEnvironment`/`StartFromAmbientRunPrompt`, Suggestions round trip + `api_metadata_bytes`, `SystemStats`, dead `CtrlR` chain + Voltron arms, stale allows/copy |
| R3/R4 part 2 | `eba0c1b5f` | passive-suggestion unit-test + code-diff halves (model reduced to static prompt suggestions), ~45 never-dispatched action variants + the handlers they orphaned |
| R5 | `42e0eebcb` | zero-state prompt-suggestion chips and their parameter chain, `NewWorkspaceSource::AgentSession`, editor-modal same-line-prompt section |
| R4.4 part | `5376b74cd` | profile context-window UI/plumbing in both editors, `LLMContextWindow`, discount chip, `request_multiplier` |
| R2 tail | `8928dfdd9` | never-enabled `completions_v2` stack (see below) |
| Tips + worktree | `31b82b11b` | welcome-tips popup, `TipsCompleted` model and its two settings, `TerminalViewResources` (empty afterwards), dead `tab_configs/telemetry.rs`; new `/new-worktree` slash command (local, shell/terminal view) opens the existing new-worktree modal |
| Credits UI + `pr-comments` | `9fd56adca` | `/cost`, usage footer + `ConversationUsageView`, context-window toolbar chip (variant kept as a never-rendered tombstone), details-panel "Credits used", `AIConversation` usage/cost state, persisted `ConversationUsageMetadata` types, `RequestCost`, `ContextWindowUsageV2`/`Breakdown`/`PromptCacheExpiryWarning` flags (`V2` folded on: split panes keep their header), `context-remaining-*` icons, `pr-comments` bundled skill |
| R4.1 tail | `6bfca9749` | `AIAgentInput::TriggerPassiveSuggestion`, `PassiveSuggestionTrigger*`, `PromptSuggestion`/`TriggerPassiveSuggestion` entrypoints, `SuggestPrompt`/`SuggestNewConversation` actions + results + executors + `SuggestedUnitTestsView`; saved tool calls/results map to "gone" like the other removed tools, persisted rows still deserialize |
| R5 small | `da44f77db` | `RefreshConfig::OnFileChanges`, `WorkflowsFocusState`, prompt-editor configuration row, `NotificationsSettings::is_password_prompt_enabled` (struct is `serde(default)`, old files still load) |
| R3 tail | `efef75dec` | `AgentConfigSnapshot` keeps only live fields (`host` file key still accepted, test added), `source_repo` module |
| §3 fake quota | `8d08071a9` | `AIRequestUsageModel` + its always-false guards, "requires AI credits" tooltip, credits-based default-model switch modal in the agent settings page |
| §4.10 MCP (Phase 3b) | `58923a77d` | `crates/local_inference` now reads `Request::mcp_context`: each connected server tool is offered to the model as `mcp_<server id>_<tool>` and run as `CallMCPTool`; one `read_mcp_resource` tool reads resources (the prompt lists them). `CallMCPToolResult`/`ReadMCPResourceResult` are rendered back to the model (text kept, images/binary named); history calls keep the same name. |
| §4.8/§4.9 request settings | (this commit) | `get_supported_tools` lists only the 9 tools the adapter offers (+ MCP); dead `RequestParams` fields (`planning_enabled`, `research_agent_enabled` + its pref read, `allow_use_of_warp_credits` + `api_keys_with_warp_credit_fallback_setting`) and the fixed-false / unread `Settings` literals are gone (`..Default::default()`); dogfood skills `test-warp-ui` and `verify-ui-change-in-cloud` deleted. |
| leftovers | (this commit) | `MCPGroupedServerContext` flag folded (always grouped by server; the flat-list request branch and `TemplatableMCPServerManager::tools` are gone); `WorkspaceAction::OpenVerticalTabsPanel` (test-only) removed. | The prompt-suggestion banner was checked and is live (local regex-matched static suggestions in `passive_suggestions/legacy.rs` + `static_prompt_suggestions.rs`), so it stays.
| §5 flags | (this commit) | Deleted: `SequentialStorage` (reversed grid storage is now the only layout), `MaximizeFlatStorage`, `InBandGeneratorsForSSH`, `RecordPtyThroughput` (+ `local_tty/recorder.rs`), `LSPAsATool` (+ the LSP repo watcher). Enabled in the `simplewarp` set: `NldPromptHistoryMatch`, `CycleNextCommandSuggestion`, `PartialNextCommandSuggestions`, `ResizeFix`, `SelectablePrompt`, `AgentViewPromptChip`, `AIContextMenuCommands`, `CommandCorrectionsHistoryRule`, `AlacrittySettingsImport`, `McpJsonTreeView`, `BoxDrawingGlyphs`, `RemoveAutosuggestionDuringTabCompletions`, `FileGlobV2Warnings`, `RememberFastForwardState`, `RestorePromptOnInlineModelSelectorSearch`, `ToggleBootstrapBlock`, `CodeReviewScrollPreservation`, `SshDragAndDrop`, `Projects`, and `DragTabsToWindows` (macOS/Windows). Kept off on purpose: `RunGeneratorsWithCmdExe`, `MSYS2Shells` (Windows-only, unverifiable here). |
| Passive code diff | (this commit) | `AIAgentInput::AutoCodeDiffQuery`, `AIRequestType`/`PassiveRequestType`, `is_entirely_passive*`/`is_passive_conversation`, `DisplayMode::InlineBanner` and the whole passive banner of `CodeDiffView` (speedbump, iterate/accept-and-continue, dismiss, scroll-to-expand), `CodeDiffAction`/`TryHandlePassiveCodeDiff`, the passive keybinding context flag, `request_follow_up_after_actions`, the `show_code_suggestion_speedbump` and `code_suggestions_enabled_internal` settings (+ the hidden "Suggested Code Banners" toggle). Restoring a saved conversation now drops whole exchanges that began as an `AutoCodeDiff` system query (test added); the proto types in persisted/saved data are untouched. |

### Findings that change earlier claims

- **§2/§4 `CtrlR` correction:** `InputAction::CtrlR` had no constructor at all, so the Voltron
  History no-op was not user-reachable (the live ctrl-r path is `CustomAction::CommandSearch`).
- **§4.4 `LLMSpec` is not dead UI:** the Auto/Reasoning sidecars in `profile_model_selector.rs`
  still render Intelligence/Speed/Cost bars from a defaulted spec, so removing `LLMSpec` is a
  sidecar redesign, not a deletion. `DisableReason` is likewise pinned by the reconcile tests in
  `llms_tests.rs`. Both left in place.
- **§4.6 left alone:** `RenderableAIError::QuotaLimit` is reachable through the provider-429
  mapping (`mod_tests.rs`), so only the wire `StreamFinished` arms are unreachable; not worth a cut.
- **Orphaned local features (resolved 2026-10-12):** the welcome-tips popup was deleted; the
  new-worktree modal now opens from the `/new-worktree` slash command (`OpenNewWorktreeModal`),
  and submitting it writes a tab config whose commands run `git worktree add` locally.
- `OpenVerticalTabsPanel` is constructed only by two tests in `workspace/view_tests.rs`.

### Not done yet

- **R2 tail (done, 2026-10-12):** the `completions_v2` flow was never enabled by any build
  (not in `simplewarp` or `default`), so it was deleted whole: `warp_js`, `command-signatures-v2`
  (crate and root TS dir), `app/src/plugin/`, `app/src/completer/js.rs`, `WorkerCommand::PluginHost`,
  the `plugin_host`/`completions_v2`/`warp_completer/v2` features, `rquickjs`, and every `v2` module
  in `warp_completer` (`legacy` files were renamed to `imp`). CI/presubmit no longer run the v2 test
  pass.
- R3: `JsonObjectType` tombstone variants (`CloudEnvironment`/`ScheduledAmbientAgent`/
  `CloudAgentConfig`) deliberately kept so old object-metadata rows still parse.
- R4.8 and R4.10: done (see §10).
- `mcp_context` Phase 3b and the constant-false flags: done (see the two rows above). The LSP
  `did_change_watched_files` / `WatchedFilesRegistry` path went too (the client no longer advertises
  `didChangeWatchedFiles`, `globset` and `warp_util::path::normalize_relative_path_for_glob` are gone). The newly enabled flags, MCP, `/new-worktree`
  and vim-mode Ctrl+C were tried by hand in a live `simplewarp` build on 2026-10-13 and worked.
- **Integration suite (2026-10-15):** `cargo nextest run -p integration` was 236/245 with 9 stale
  tests, none a product regression: the model-selector test searched the removed `auto` model (now
  `gpt`); the two rich-input entries outlived their bodies (rich input went in 4bo); six settings-nav
  tests assumed the old Account default page (4ge made the first Agents subpage the default, so the
  Agents umbrella starts expanded, and `CodeIndexing` is no longer in the nav). After the fixes:
  243/243, 59 skipped.
- **CI:** `.github/workflows/ci.yml` runs only on pull requests and uses Warp's
  `namespace-profile-*` runners, so it never runs on this fork (pushes go straight to master).
