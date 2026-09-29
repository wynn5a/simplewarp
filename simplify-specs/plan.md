# SimpleWarp — Plan

Per-round details (file lists, test counts, acceptance runs) were in the old version of this file
and live on in git history: `git log -p -- simplify-specs/plan.md`, plus each round's commit.

## Goal

Make **SimpleWarp**: a fast terminal with bring-your-own-key (BYOK) AI.
No cloud, no login, no subscription, no Warp Drive.

- The app works offline. The only network traffic goes to the AI provider the user configures.
- No login screen and no anonymous Firebase user.
- No Warp Drive, shared sessions, cloud mode, ambient agents, or billing UI.
- AI keys stay on the machine. The app calls the provider direct.
- Keep: terminal emulation, tabs, panes, settings, themes, shell management, completions,
  command palette, and the GUI front-end.

## Decisions

| Topic | Decision |
| --- | --- |
| AI | Local adapter (`crates/local_inference`). The app calls the provider direct. |
| Code removal | Gate, then hide, then delete. |
| TUI | Deleted. It is not part of the GUI app. |
| Name | SimpleWarp. Bin `simplewarp`, id `dev.simplewarp.SimpleWarp`, scheme `simplewarp`. |
| Deletion scope (2026-09-15, reaffirmed 2026-09-25 in 4fu) | Delete only what **requires a remote service**. A feature that works fully locally is never a deletion target, even when its flag is constant-false in the simplewarp build. Local-but-disabled features are *enable-in-simplewarp* candidates, a separate per-feature product decision. |
| Warp Drive (2026-09-26) | This fork has no Warp Drive functionality at all; the whole drive surface is deleted (4gi–4gl). |
| Persisted shapes | Serde/persisted enum variants and setting keys stay (or get a tombstone) so old configs still load. |

## Status

| Phase | Status |
| --- | --- |
| 0 — Baseline build green | DONE |
| 1 — `simplewarp` binary: starts offline, straight to a terminal, 0 startup errors, no outbound connections | DONE |
| 2 — Hide the cloud UI (login, billing, Drive, sharing) | DONE for every surface checked; cloud mode / ambient agents / remote-server UI checked only by deletion since |
| 3 — Local AI adapter, verified by a real conversation in the app | DONE |
| 3b — Built-in model list, MCP tool support | Model list DONE (2026-09-29); MCP tool support OPEN |
| 4 — Delete the dead cloud code and the TUI | DONE through 4id: the 4hp remote-only queue (R1–R7) and its follow-ups are done; what remains under Next is product decisions |

Enabled in the simplewarp build: `jupyter_notebook_rendering` (2026-09-15) and, since 4id
(2026-09-29), EditableMarkdownMermaid, ImeMarkedText, ITermImages, LocalDockerSandbox and
local computer use — all pinned by `features::tests`. The enable-candidate list is empty.
With `local_computer_use` on, the `RequestComputerUse`/`UseComputer` tools are advertised
when the computer-use setting and platform allow; real-model verification of a computer-use
run is still TODO (needs a live session).

## Phase 4 progress

Workspace is down to 62 packages (`cargo metadata`). Gone, in rough order:

- **TUI** (1–1c): the front-end, its rendering engine, `ratatui`, every surface marker (~120k lines).
- **Server-only features** (2–3aa): billing, experiments, referrals, resource center, changelog,
  login UI, onboarding wizard, discoverable teams; every warp-server request fails locally.
- **AI path** (4, 4b–4d): `warp_multi_agent_client`, IAP layer, billing/credits/usage UI,
  request quota. The local adapter is the only AI path.
- **Session sharing** (4e–4j): sharer, viewer, network layers, modals (~43k lines).
- **Cloud clients** (4l–4p, 4be–4bg): Block/ManagedMcp/Factory/Integrations clients, cloud
  environments, attachment upload, managed secrets, ObjectClient + sync queue + sharing dialog.
- **Constant-false remote flag verticals** (4z–4ag, 4bp–4bu): SuperGrok OAuth, xAI, Codex/HOA
  plugins, notifications mailbox, SharedWithMe, team billing, Autoupdate, remote codebase
  indexing, MAA prompt suggestions, predicted AM queries.
- **AIClient / AuthClient walls** (4bh–4cg): memory-store, named agents, skills, observability,
  artifacts, api-key CLI, StoreClient + full-source embedding, assistant surfaces, conversation sync.
- **Cloud runs** (4ch–4co): cloud-run lifecycle, ambient UI, transcript upload, HTTP retry layer.
- **GraphQL** (4cp–4db): all orphaned query / mutation / subscription modules.
- **Cloud object model** (4du–4ey): sharing/subject/server-object types and traits, dead deps.
- **Telemetry** (4ez–4fe): remote send path, event catalog, queue, flags and config.
- **The fold** (4ff–4fj): `warp_server_client` and `firebase` crates, `BaseClient`, `ServerApi`.
- **Endgame** (4fk–4fy): server-intake half of CloudModel, `warp_graphql` shrink, conversation
  link/deep-link and always-None verticals.
- **Login vertical** (4fz–4gg): auth redirect, logout/sign-up, AuthManager, the wire, account UI,
  `warp_graphql` crate.
- **Warp Drive** (4gh–4gl): tools panel, menus, settings, index, browse/import/export,
  breadcrumbs, dead cargo features (~17,900 lines).
- **Residue** (4gm–4gn): remote-shaped leftovers, naming, test stubs, notebook flake fix.

## Next

**Remaining queue (4hp survey, 2026-09-29).** No Warp server URL, GraphQL/websocket client or
`.invalid` host is left in Rust. What still assumes a Warp service, in rough order:

- ~~**R1 — cloud-agent OTLP tracing export**~~ — **4hq done (2026-09-29).**
- ~~**R2 — local-to-cloud handoff stubs**~~ — **4hr done (2026-09-29).**
- ~~**R3 — dead cloud-agent context in the agent view**~~ — **4hr done (2026-09-29).**
- ~~**R4 — always-logged-out `AuthState` walls**~~ — **4hs done (2026-09-29).**
- ~~**R5 — `channel_versions` crate**~~ — **4ht done (2026-09-29).**
- ~~**R6 — Warp server error shapes**~~ — **4ht done (2026-09-29)**, with the `X-Warp-*` request
  headers.
- ~~**R7 — scripts / test infra**~~ — **4hu done (2026-09-29).**

The queue is empty.

Product decisions, not deletions: the Oz branding of the local CLI install and ~23 user-visible
"Oz" strings (rebrand). The Help menu / `JoinSlack` / feedback / docs.warp.dev links campaign is
**done** (4hy): dropped, not repointed. The local Claude/Codex child-harness `oz run message` bug is fixed (4hv);
`agent run --harness claude` no longer needs the Oz platform plugin (4hw). The onboarding tutorial is
deleted (4hx).

Outstanding product decisions — ALL RESOLVED (2026-09-29 user rulings, rounds 4ia–4id):
orchestration vertical **deleted** (4ia); provider 429 → `QuotaLimit` **mapped** (4ib); Oz
branding → SimpleWarp **rebranded** (4ic); enable-candidates **all enabled** (4id); Claude
notification plugin **keeps** its one-time GitHub fetch (no change). Nothing left under Next
except the open Phase 3b (built-in model list + MCP tool support).
Deliberately kept: `ServerId` / `SyncId::ServerId` / `server_conversation_token` (the local adapter
sets the token), `AmbientAgentTaskId` (minted locally for child runs), `Harness::Oz`, the serde
`CloudAgent` / `ScheduledAmbientAgent` shapes, `crates/isolation_platform` (local detection) and
`crates/http_server` (local profiling).

**4go done (2026-09-28): the Warp/Oz server-URL readers in desktop code.** In simplewarp every
server URL is a `.invalid` host (`aa4c271c0`), so each reader was a dead end. Removed:
`upgrade_link_for_team`; Factory MCP (flag, cargo feature, manager and driver paths; proven
unreachable: the client always sends an empty token and it also required a logged-in user); the
`factory-mcp`, `factory-files` and `oz-platform` bundled skills (Warp cloud products; the
factory-files validator ran against `app.warp.dev`) plus the `RequiresFeature` skill gate they
were the only user of; the `warp provider` OAuth CLI and `ProviderCommand` flag; Oz
memory/skill/run links; `conversation_link`/`debug_link` (the debug payload is always the id
JSON); every cloud-object "Copy link" / "Open on Desktop" / "Run in Warp" item and the
`object_link` trait method under them; the agent CLI's plan `artifact_created` output (it fired
only for server-backed plans); the privacy page's delete-your-account widget; the dev
server-URL override flags, env vars, `ChannelState::override_*` and their child-process
propagation; the `warp_server_url` skill variable; the desktop web-URL-to-intent rewrite
(`web_intent_parser` is now wasm-only). Kept, being local: `OzPlatformSkills` (local `--skill`
resolution), `warp_cli_binary_name`, all nine local bundled skills. Tests 4,523 default / 4,524
simplewarp (−16, all deleted with their code), 0 failed; clippy and format clean.

Queue, in order:

1. ~~wasm/web target~~ — **4gp done (2026-09-28, user decision: desktop only).** −12.5k lines in
   408 files: every wasm cfg site, the `serve-wasm`, `warp_web_event_bus` and `websocket` crates,
   wasm platform backends in warpui/warpui_core, wasm-only app modules (font fallback, NUX dialog,
   web intent parser, browser URL handler, …), wasm deps/profiles/CI jobs/scripts, the pane
   `shareable_link` machinery, `ContextFlag` (all flags were true on desktop; only the web build
   disabled them, so read sites collapsed to true), the web home page, mobile/soft-keyboard
   plumbing, remote assets, and the `rtc_server_url` / `session_sharing_server_url` /
   `oz_root_url` channel fields. `server_root_url` stays: `workload_audience_url()` still falls back
   to it (read by `crates/isolation_platform`). Orphaned private setting `UserAppInstallStatus`
   (web-only, never read on desktop). Tests unchanged (4,392 / 4,393); app launch verified.
   Follow-ups: collapse `local_fs` / `local_tty` (now always on) and their `not(...)` stubs; the
   inert external fallback-font subsystem in warpui_core; decide on `app-installation-detection`
   (desktop still serves `/install_detection` for warp.dev); `ChannelState::firebase_api_key` (zero
   readers); `.clippy.toml` wasm wording.
2. ~~Sentry crash reporting~~ — **4gq done (2026-09-28).** −3.4k lines in 69 files:
   `app/src/crash_reporting/` (Rust + minidump server + Cocoa bridge and its objc files), the
   `crash_reporting` / `cocoa_sentry` cargo features in app, `warp_logging`, `warp_errors` and `ai`,
   the `sentry` / `sentry-log` / `minidumper` / `crash-handler` deps (~40 crates out of
   Cargo.lock), the `CrashReporting` / `CocoaSentry` flags, the `minidump-server` worker
   subcommand, `CrashReportingConfig` / `sentry_url` / `is_crash_reporting_available`, the
   privacy-page "Send crash reports" widget, its command-palette toggle and context flag, the
   `IsCrashReportingEnabled` setting (the orphaned `privacy.crash_reporting_enabled` /
   `CrashReportingEnabled` keys still load harmlessly), the spawner's cocoa-sentry uninit/reinit
   around pty spawn, the debug "Crash the app" action, the dogfood process-sample upload, the
   Sentry-tag-only `AntivirusInfo` model, the Sentry build.rs/framework download, the bundle
   `osx_frameworks`, and the Sentry steps/scripts in CI and bundle scripts. `report_error!` keeps
   working as local logging (the capture half and `with_error_context` are gone; `is_actionable`
   still picks Error vs Warn). Kept, being local: `heap_usage_tracking` (now just
   `jemalloc_pprof` + the "Write heap profile to disk" command; its Sentry auto-upload on
   excessive memory is gone), crash recovery, `report_if_error!`, the logging skill (retitled as
   local-only). Tests unchanged (4,392 / 4,393). Follow-ups: `SystemInfoEvent::MemoryUsageHigh` has
   no subscriber and `memory_footprint::memory_breakdown()` is computed and discarded (the whole
   excessive-memory check is dead); `crash_recovery::Event::CrashRecoveryProcessTornDown` is now
   unobserved; warpui's `on_gpu_driver_selected` hook is always `None` from the app; the telemetry
   / cloud-conversation privacy toggles and their Warp Drive pref sync; Sentry-grouping wording
   in comments and the rest of the logging skill; stale TUI branches in the bundle scripts.
3. ~~SSH remote server~~ — **4gr done (2026-09-28, user decision: delete).** Its install
   downloaded a Warp binary from the `.invalid` server URL, so it could never work here. Gone
   (−30.6k lines): `app/src/remote_server/`, `crates/remote_server/`, the `SshRemoteServer` and
   `RemoteCodeReview` flags, the install prompt / failure banner / loading footer / "Install SSH
   extension" setting, the daemon worker subcommands and `ExecutionMode::RemoteServerDaemon`,
   persistence scopes (the app sqlite path is unchanged), the daemon bearer-token plumbing
   (`Credentials::Bearer`), and every remote buffer/diff/git/search/skill/context path fed only by
   the daemon. Plain SSH is on its old path (ControlMaster `RemoteCommandExecutor`). The orphaned
   setting key `warpify.ssh.ssh_extension_install_mode` still loads harmlessly. Tests 4,392 /
   4,393 (−131, all deleted with their code); app launch verified. Follow-ups found: the
   now-unfed remote half of `crates/repo_metadata` and file-tree remote roots,
   `FileSaveError::RemoteError`, three single-variant enums (`DiffStateModel`,
   `GitRepoStatusModel`, `GitHubRepoModel`) and `SessionType` ≡ `BootstrapSessionType` to
   flatten, stale "SSH extension" text (tmux deprecation banner, migration comments, `specs/`,
   `EXCLUDE_REMOTE_SERVER_TESTS_FILTER` in `ci.yml`).
4. ~~`server_root_url`~~ — **4gs done (2026-09-28).** −594 lines in 21 files: `ChannelConfig`
   has no server fields left — `WarpServerConfig` (`server_root_url`, `firebase_auth_api_key`) and
   `OzConfig` (`workload_audience_url`) are gone with `ChannelState::server_root_url` /
   `workload_audience_url` / `firebase_api_key`, the test-util mockito `MOCK_SERVER` behind
   `server_root_url` (and the now-unused `mockito` dep in warp_core, app, integration and the
   workspace). Channel configs are still loadable: serde ignores the old keys. The workload-token
   half of `crates/isolation_platform` (`issue_workload_token`, the Namespace `nsc` token call and
   its JWT parsing + 5 tests, `docker_sandbox`, `WorkloadToken`, `IsolationPlatformError`,
   `WARP_WORKLOAD_TOKEN`) had zero callers and only fed Warp's workload-identity service. The
   `app-installation-detection` crate (`/install_detection` for warp.dev's website) and the
   dangling `installation_detection_server_subcommand` are gone; `http_server` now serves only the
   local profiling router (app gains `tracing-subscriber/env-filter`, which it had been getting via
   that crate). Kept, being local: `isolation_platform::detect()` (Docker/Kubernetes/Namespace
   sandbox detection; drives the pty child's OOM-score bump on Linux and the agent driver's
   `IS_SANDBOX` / harness config), `mcp_static_config`. Tests unchanged (4,392 / 4,393).
   Follow-ups: the internal channel bins (`dev`/`local`/`preview`/`stable`) and
   `crates/warp_channel_config`, which shell out to Warp's internal `warp-channel-config`
   generator; `agent_sdk/driver/cache_setup` + `crates/build_cache` (runs only on Warp-hosted
   Namespace instances with source repos from a cloud environment); `DockerSandbox` isolation
   variant (only via a server-set env var); orphaned `app/src/sharing/qr_code_tests.rs` (no module).
5. ~~Remote-only privacy toggles~~ — **4gt done (2026-09-28).** −627 lines in 25 files. Both
   toggles' widgets were already gone; what was left controlled nothing (no reader of either value
   drove local behavior — the only readers were the palette context flag and three
   recompute-on-change subscriptions in the slash-command sources and agent footer). Gone: the
   `WarpDrivePrivacySettings` group (`IsTelemetryEnabled`, `IsCloudConversationStorageEnabled`;
   the orphaned `TelemetryEnabled` / `CloudConversationStorageEnabled` keys and their TOML paths
   still load harmlessly) and its schema test, `PrivacySettings::is_telemetry_enabled` /
   `is_cloud_conversation_storage_enabled` / `is_telemetry_force_enabled` with their setters and
   `Update*` events, the "app analytics" palette toggle, `ToggleTelemetry` and `TELEMETRY_FLAG`, the
   Warp Drive pref sync (`maybe_sync_with_warp_drive_prefs`, its call in the syncer's `sync()`, the
   two legacy storage keys), and the telemetry-policy banner (`TelemetryBanner`, the
   `GlobalAIAnalyticsBanner` flag and cargo feature, `HideTelemetryBannerPermanently`, its
   rich-content variant, the `TelemetryBannerDismissed` setting, key orphaned). Kept, being local:
   secret redaction, the network log console, `should_collect_ai_ugc_telemetry` (moved to
   `ai/blocklist/ugc_telemetry.rs`; it picks how much block output is serialized), and the
   privacy-policy link. Tests 4,391 / 4,392 (−1, the deleted schema test). Follow-ups:
   `initialize_default_regexes_once` has had no production caller since the syncer went inert (the
   deleted pref sync was its only caller), so new installs no longer get the recommended secret
   regexes auto-added (product decision: call it at startup, or leave the manual "Add all");
   `CloudPreferencesSyncer` itself is sync-disabled in every build (only its local settings-file
   hash path runs); the privacy-policy links point at warp.dev (settings page, app menu, workspace
   action); `WorkspaceSettings.telemetry_settings` / `cloud_conversation_storage_settings` are
   now-unread server team shapes.
6. ~~CloudPreferencesSyncer and unread server team settings~~ — **4gu done (2026-09-28).** −2.9k
   lines in 39 files. The syncer was inert in every build and so was its "local" settings-file hash
   path: the hash was read at startup and only consulted, and only rewritten, inside the
   never-reached cloud initial load. Gone: `cloud_preferences_syncer.rs` (whole file), its
   `SettingsFileLastSyncedHash` private key (orphaned, harmless),
   `TomlBackedUserPreferences::file_content_hash` + 5 tests and warpui_extras' `sha2` dep,
   `CloudModel::get_all_cloud_preferences_by_storage_key`, `GenericStringObjectInput`,
   `AppExecutionMode::can_sync_preferences`, the `CloudPreferencesSettings` group
   (`IsSettingsSyncEnabled` / `account.is_settings_sync_enabled`, key orphaned), and the whole
   settings-page "not synced to your other devices" icon (`LocalOnlyIconState`,
   `render_local_only_icon`, `local_only_icon_with_tooltip`, `cloud-off.svg`, the icon param on
   ten render helpers and the tooltip-state fields of eleven pages; the `Setting` type parameter of
   `render_ai_setting_toggle` / `_label` existed only to feed it). Execution profiles: the
   logged-in-only legacy import that waited on the syncer (`migrate_settings_profiles`, the
   cloud-collection reconciliation gates, `sync_explicit_settings_collection`) is gone and
   `SettingsMigrationState` is down to `PendingLegacyImport` / `Authoritative`; local edits still
   materialize the settings collection. `WorkspaceSettings` loses `telemetry_settings`,
   `cloud_conversation_storage_settings`, `link_sharing_settings`, `is_invite_link_enabled`,
   `is_discoverable`, `addon_credits_settings` (never persisted: workspaces load with default
   settings); the persisted `TeamSettings` JSON loses the unread `telemetry_settings`,
   `cloud_conversation_storage`, `link_sharing`, `addon_credits_settings` (old cached rows still
   load: serde ignores unknown keys). Kept, being persisted shapes: `Preference` /
   `CloudPreference` objects still load from sqlite into CloudModel (session-restoration
   integration test). **Regex fix:** startup now calls `initialize_default_regexes_once` right
   after `PrivacySettings` registers — once per install via the private
   `HasInitializedDefaultSecretRegexes` flag, so regexes the user removed never come back (new
   `privacy_tests.rs`). Upstream nuance: upstream ran it after the Warp Drive initial load for
   existing users and suppressed it for brand-new accounts (`disable_default_regex_trigger` on
   `is_onboarded == false`, dead since 4gc and now deleted); with no account there is no new-user
   signal, so every fresh install now gets the recommended list (inert until secret redaction is
   turned on). Tests 4,383 default / 4,384 simplewarp (−9 legacy-import tests deleted with their
   code, +1 regex test), 0 failed. Follow-ups: the `sync_to_cloud:` marker on every setting
   (~350 declarations, the `define_setting!` / `implement_setting_for_enum!` arms,
   `Setting::sync_to_cloud` / `current_value_is_syncable` / `is_setting_syncable_on_current_platform`,
   `RespectUserSyncSetting`) and the SettingsManager half that only served the syncer
   (`equals_fns`, `is_syncable_fns`, `sync_regardless_of_users_syncing_setting`,
   `cloud_syncing_mode_for_storage_key`, `are_equal_settings`, `all_storage_keys`,
   `SettingsEvent::LocalPreferencesUpdated`, `set_value_from_cloud_sync` / `ChangeEventReason::CloudSync`);
   the rest of the logged-in-only legacy execution-profile backend (`LegacyCloudObjects`,
   personal-drive ownership, `reconcile_with_cloud_state_after_initial_load`); the whole
   workspace/team settings layer (every workspace loads with default settings; `TeamSettings` is
   written only by the gone server fetch).

7. ~~Settings cloud-sync machinery~~ — **4gv done (2026-09-28).** −1.3k lines in 62 files. The
   `sync_to_cloud:` marker is gone from all 258 declarations (scripted; `define_setting!` /
   `maybe_define_setting!` / `define_settings_group!` / `implement_setting_for_enum!` arms and the
   crate doc examples), with `SyncToCloud`, `RespectUserSyncSetting`, `Setting::sync_to_cloud` /
   `current_value_is_syncable` / `is_setting_syncable_on_current_platform` /
   `set_value_from_cloud_sync`, `ChangeEventReason::CloudSync`, `SettingsMode::should_sync_to_cloud`.
   No storage key, TOML path or serde shape changed. SettingsManager loses everything only the
   syncer read: `SettingsEvent` (event type now `()`) and its per-group subscription, `clear_fns` /
   `clear_cloud_settings_local_state`, `is_syncable_fns`, `sync_regardless_of_users_syncing_setting`,
   `cloud_syncing_mode_for_storage_key`, `are_equal_settings`, `all_storage_keys`,
   `supported_platforms_for_storage_key`, `is_private_for_storage_key`, `read_local_setting_value`,
   and `update_setting_with_storage_key`'s `from_cloud_sync` flag. `equals_fns` had one local reader
   (`validate_all_public_settings`, the startup settings-file check, which called it as
   `equals(v, v)`), so it became `validate_fns` (`Fn(&str) -> Result<()>`). Also gone: the theme
   settings' inherent `current_value_is_syncable` and `ThemeKind::is_custom_theme_reference_syncable`
   (+ `settings/theme_tests.rs`, 6 tests; the portable-path check moved into `themes/theme_tests.rs`
   as a test helper since only tests use it now), the #13228 "not synced" test, sync comments on
   ~10 declarations, and cloud_object_models' `settings` dep (`Preference::new` takes a `Platform`
   instead of a sync mode; the persisted `Preference` shape is unchanged). The settings-schema
   generator never emitted the flag; nothing regenerated. Kept, being local: `public_storage_keys`,
   `update_setting_with_storage_key` (settings-file import), `default_values` (app menu),
   `load_setting` / `reload_all_public_settings` (hot reload), `ChangeEventReason` (`LocalChange` /
   `Clear`; the field is never read, left for a separate cleanup). Tests 4,376 default / 4,377
   simplewarp (−7: the theme syncability file and the #13228 sync test), settings + settings_value +
   cloud_object_models + warpui_core 417 passed (−6 in `settings`: syncability, cloud-sync
   explicit-set, manager is-private, 3 read-local), settings doc tests 4 (−1 sync example), schema bin 1, 0 failed. Follow-ups: the unread
   `change_event_reason` field on every settings event; the rest of the legacy execution-profile
   cloud backend; the workspace/team settings layer (see 4gu).

8. ~~Legacy profile cloud backend and workspace/team settings layer~~ — **4gw done (2026-09-28).**
   −9.7k lines in 149 files. Execution profiles: the logged-in-only Warp Drive backend is gone
   (`LegacyCloudObjects`, `DefaultProfileState`, personal-drive ownership, the `CloudModel`
   subscriptions, `reconcile_with_cloud_state_after_initial_load`, `maybe_inherit_from_legacy_settings`,
   `reset`, sync ids on `AIExecutionProfileInfo`, `get_profile_id_by_sync_id`,
   `ExecutionProfileId::from_legacy_server_id`, UpdateManager's profile update/delete, the
   `FileBackedExecutionProfiles` flag + cargo feature). `ProfileSource` is now `Settings` /
   `PendingSettings` (implicit default from legacy settings until the first edit materializes the
   collection) / `Cli` (fixed, uneditable); local create/edit/delete/select unchanged. Workspace/team
   settings: `WorkspaceSettings`, `TeamSettings` and every sub-type are gone (workspaces always loaded
   with defaults; the `team_settings` sqlite table is kept, no longer read), with the dead writers
   (`ModelEvent::UpsertWorkspace(s)` / `SetCurrentWorkspace`, `save_workspace(s)`,
   `set_current_workspace`; never emitted). Every reader collapsed to its default: org autonomy /
   sandboxed-agent overrides and the "managed by your workspace" banner + "enforced by your
   organization" tooltips (permissions getters now read the profile; `get_org_execute_commands_denylist`
   gone), enterprise secret redaction (PrivacySettings fields, Enterprise tab on the privacy page),
   UGC collection, org remote-session AI policy (`FocusedTerminalInfo`, the remote-block flags and
   command regex check; `is_any_ai_enabled` and 21 other AISettings getters lost their unused ctx),
   codebase-context and agent-attribution org gates, team-provided BYO keys/endpoints
   (`ByoKeySource::TeamProvided`), `default_host_slug`, the cloud-agent computer-use org lock, and the
   Bedrock/Gemini settings widgets. Gemini Enterprise (GEAP) is deleted outright (it needed the
   workspace federation config and a Warp login): app + `crates/ai` credential modules, request-time
   refresh, error view, `RenderableAIError` variant, `FeatureFlag::GeminiEnterprise` + feature,
   `GeminiEnterpriseCredentialsEnabled` setting (key orphaned). Server-only `Workspace` / `Team` fields
   and billing types nothing read are gone (members/invites/usage history/overages/service
   agreements, `TeamVisibility`, `TeamDeleteDisabledReason`, 16 `BillingMetadata` helpers; orphaned
   `teams_page_tests.rs`). `change_event_reason` / `ChangeEventReason` removed: settings events are
   unit variants now. Kept, being persisted shapes: `BillingMetadata` / `Tier` JSON (incl.
   `UgcCollectionEnablementSetting`), the workspace sqlite tables, `TerminalPaneSnapshot.active_profile_id`
   (always written `None`), `CloudAIExecutionProfile` rows loading into CloudModel,
   `LLMModelHost::GeminiEnterprise`. Stale-cache nuance: a workspace cached by upstream Warp used to
   block AI in remote sessions (placeholder default `allow_ai_in_remote_sessions = false`); it no longer
   does. Tests 4,306 default / 4,307 simplewarp (−70, all deleted with their code: 17 GEAP, 18
   workspace host/codebase/attribution, 10 workspace billing, 6 attribution toggle, 6 org-permission,
   5 FocusedTerminalInfo, 3 GEAP refresh, 2+2 profile/MCP-secret rewrites, 1 remote-flag), `ai` +
   `settings` + `warp_features` 277 passed (−29 GEAP in `ai`), settings doc tests 4, 0 failed.
   Follow-ups: the AWS Bedrock vertical (credential refresh + AWS SDK deps, login/CLI banners, the
   Bedrock credentials error view, model-picker Bedrock icon, `aws_bedrock_*` settings), now gated off
   by a constant-false `UserWorkspaces::is_aws_bedrock_credentials_enabled`; `UserWorkspaces` itself
   (workspaces only come from a sqlite cache this client never writes — billing-tier gates, team
   windows/spaces, `TeamsChanged` only fires in tests); the terminal secret model's always-empty
   enterprise tier (`SecretLevel::Enterprise`); `agent run --profile` (always errors now) and the
   "Unsynced" ids of `agent profile list`; single-variant `ByoKeySource`.

9. ~~AWS Bedrock (Warp-server path)~~ — **4gx done (2026-09-28).** −1.7k lines in 44 files (+Cargo.lock
   −670). Verified first: `crates/local_inference` has no Bedrock provider and never reads
   `ApiKeys.aws_credentials`; the whole chain only put the user's AWS credentials on the request for
   Warp's server to call Bedrock. Gone: the app + `crates/ai` `aws_credentials` modules (SDK credential
   refresh, `AwsCredentialRefresher`, `AwsCredentialsState`, block-completion / settings / TeamsChanged
   subscriptions), `ApiKeyManager`'s credential state (`api_keys_for_request` takes only the BYO flag;
   the proto `aws_credentials` field is always `None`), the AWS SDK deps (`aws-config`,
   `aws-credential-types`, `aws-sdk-sts`, `aws-types`, ~20 transitive crates; `aws-lc-*` stay for
   rustls), the "Use AWS Bedrock?" and "AWS CLI not installed" inline banners with
   `ByoLlmAuthBannerSessionState`, the run-`aws login` terminal plumbing (`is_pending_aws_login`,
   `AIBlockEvent/Action::RunAwsLoginCommand`, `ConfigureAwsLoginCommand`, `ToggleAwsBedrockAutoLogin`),
   the credentials error view + `RenderableAIError::AwsBedrockCredentialsExpiredOrInvalid` +
   `FailedOutputPresentation` variant (an invalid-key stream end from a Bedrock provider now renders as
   the generic invalid-key error), the model-picker Bedrock icon / "Inference via Bedrock" cost row
   (`should_show_bedrock_icon_for_model`, `ModelIconFlags::is_using_bedrock`, the now-unread
   `ModelSearchItem::is_auto`), `Icon::Aws` and the orphaned `Icon::GeminiEnterpriseAgentPlatform`
   (+ both svgs), the debug "Un-dismiss AWS login banner" action, the five `aws_bedrock_*` settings
   (keys orphaned), and `UserWorkspaces::is_aws_bedrock_credentials_enabled`. Kept, being persisted
   shapes: `LLMModelHost::AwsBedrock` (model-config JSON), the proto `LlmProvider::AwsBedrock` /
   `ApiKeys.aws_credentials`. Tests 4,299 default / 4,300 simplewarp (−7, deleted with their code: 5
   SDK error-message mapping, 2 host-icon), `ai` + `warp_core` 260 passed, 0 failed. Follow-ups:
   `UserWorkspaces` itself; `SecretLevel::Enterprise`; `agent run --profile`; single-variant
   `ByoKeySource` (see 4gw).

10. ~~`UserWorkspaces`, enterprise secret tier, `ByoKeySource`, CLI profiles~~ — **4gy done
   (2026-09-28).** −2.7k lines in 135 files. `UserWorkspaces` and the `Workspace` / `Team` /
   `BillingMetadata` / `Tier` model types are gone; the workspace sqlite cache is no longer read
   (tables and migrations untouched; the unused diesel row structs `Team`, `NewTeam`,
   `TeamMemberRow`, `NewTeamMember`, `Workspace`, `NewWorkspace`, `TeamSetting`,
   `NewTeamSettings`, `WorkspaceTeam`, `NewWorkspaceTeam` deleted). Every gate collapsed to
   no-workspace: the billing-tier toggles (prompt/code suggestions, Next Command, git-ops AI,
   voice → `cfg!(voice_input)`, AI autonomy → `is_agent_mode_autonomy_allowed` deleted), member
   BYO key/endpoint policy (`CustomInferenceVisibility`, the "managed by your organization"
   section/tooltip, `custom_llm_info_for_id_if_enabled`; `api_keys_for_request` /
   `custom_model_providers_for_request` lost their always-true flag), paid-plan and enterprise
   checks, the Uber `aifx` CLI-agent special case (`CLIAgent::detect` lost its ctx). Team
   windows: the title-bar team switcher, `OpenNewWindowForTeam` / `ShowTeamSwitcherMenu`,
   `NewWorkspaceSource::TeamSwitched`, `WindowSnapshot.team_uid` (the `windows.team_uid` column
   is written `NULL`), the Team workflows tab, team MCP sharing (share button/events,
   `share_templatable_mcp_server*`, admin check in `is_authorized_editor` → `is_author`; unshare
   stays, a stale team object can still move to Personal). `personal_drive`, `Space::owner` and
   `From<Owner> for Space` replace the owner/space helpers; `Space::name` and
   `CloudObject::space` / `location` / `is_in_space` / `can_move_to_space` and the CloudModel
   space queries lost their unused ctx. `is_codebase_context_enabled` moved to `settings::code`
   (slash commands now listen to `CodeSettings` directly). `SecretLevel` is deleted outright (it
   was never serialized): regex level metadata, per-match level rescan in the grid,
   level-priority merging, the enterprise tooltip / env-var messages;
   `set_user_and_enterprise_secret_regexes` → `set_user_secret_regexes`. `ByoKeySource` →
   `BYO_KEY_INFERENCE_LABEL` + a bool. CLI profiles (local fix): CLI launches snapshot the local
   profiles (stored collection, or the implicit default), `AIExecutionProfilesModel::local_profiles`
   exposes them, `ExecutionProfilesConfig::resolve` takes an exact ID or a unique case-insensitive
   name (`ProfileLookupError::{NotFound, AmbiguousName}`); `agent profile list` prints the local
   IDs (`default`, `profile-…`) and `agent run --profile <id|name>` selects that profile for the
   run's terminal (help text updated). Tests 4,292 default / 4,293 simplewarp (−7 net: −14 deleted
   with their code — 7 UserWorkspaces, 3 Uber aifx, 2 team window, 1 window team_uid round-trip,
   1 workspace prompt alert; +7 new — 5 profile resolution, 2 CLI local-profile model),
   `ai` + `persistence` + `warp_cli` 305 passed (−3 in `ai`: BYO-disabled flag cases), 0 failed.
   Follow-ups: `Space::Team` / `Owner::Team` / `WorkflowSource::Team` readers (only stale team
   objects reach them now); `UserProfiles` (server-fetched user display names); the dead pub
   CloudModel query `trashed_cloud_object_types_in_location_with_descendants` and other
   zero-caller space queries.

11. ~~Team-object residue, `UserProfiles`, dead CloudModel queries~~ — **4gz done (2026-09-28).**
   −1.9k lines in 64 files. Stale team objects (only from an upstream cache) now load as the
   user's own: `Owner::Team` stays (persisted `TEAM` rows), but `From<Owner> for Space` maps every
   owner to `Personal`, and `Space::Team` / `Space::Shared` are gone (`Space` was never serialized;
   `Shared` was never constructed), so `Space` is single-variant. Gone with them: the Team→Personal
   move guard, `can_move_to_space` (trait + folder impl), `WorkflowSource::Team` /
   `NotebookLocation::Team` / the `team_uid` fields on `WorkflowSource::Notebook` and
   `BlockInfo::EmbeddedWorkflow` (verified never serialized: runtime events/actions only; every
   cloud workflow source is `PersonalCloud`), `From<Owner> for Option<ServerId>`, the "Duplicate"
   shared-space gates (always shown), access levels / `ContentEditability` and the `sharing`
   module (always full/editable; a stale team object is now editable, not `RequiresLogin`
   read-only), the Team MCP surface (`is_server_template_shared` / `_installation_shared`,
   "Shared by …" / "Shared from team" chips and the shared list section, the "Remove from team"
   unshare button + dialog variants + manager methods, the "only team admins" banner, the
   delete-shared dialog; `Author::OtherUser`, unknown publisher reads "another user"), and the
   now-unproduced move path (`UpdateManager::move_object_to_location`,
   `ObjectOperation::MoveToFolder` / `MoveToDrive` — not serialized — and their toast/notebook
   arms, `CloudModelEvent::ObjectMoved`, `UpdateSource`, `ActiveNotebookDataEvent::MovedToSpace`).
   `UserProfiles` is deleted outright (it only held server-fetched profiles of other users, from a
   sqlite cache nothing writes): `UserProfileWithUID` (+ `session-sharing-protocol` dep of
   cloud_object_models), the `UpsertUserProfiles` / `ClearUserProfiles` events, the diesel
   `UserProfile` row (the `user_profiles` table is kept, no longer read), `semantic_creator`,
   the test-only AI-block creator avatar helpers; readers collapse to no name ("Edited 3 days
   ago", "Other user is editing", `Editor.email` gone). Zero-caller pub CloudModel methods deleted
   (verified incl. tests): `trashed_cloud_object_types_in_location_with_descendants` + helper,
   `can_move_object_to_location`, `object_location`, `cloud_objects_mut`,
   `delete_object(_and_descendants)` + internal, `check_if_object_is_in_cloudmodel`,
   `update_notebook_current_editor` (with the then-unemitted `NotebookEditorChangedFromServer` →
   `ModeChangedFromServer` chain), `update_object_metadata_last_updated_ts`, `overwrite_workflow` /
   `_env_var_collection` / `_workflow_enum`, `get_all_exportable_object_ids`,
   `get_workflow_enum_mut`, `get_notebook_mut`, `get_workflow_mut`,
   `get_all_active_and_inactive_{workflows,workflows_mut,notebooks}`, `active_notebooks_in_space`,
   `current_revision`, `(in)directly_trashed_cloud_objects_in_…`, `trashed_cloud_objects_in_space`,
   `all_cloud_objects_in_space`, `num_{active,trashed}_cloud_objects_per_space`,
   `update_object_location`; plus `CloudViewModel::object_space`, the env-var / notebook
   `space` / `owner` / `access_level` getters, `Banner::new_without_close`, and the orphaned
   `sharing/qr_code_tests.rs`. Kept: the notebook trash banner's "Copy to Personal" (now shows only
   when the notebook is gone from CloudModel, as before for personal ones); test-only
   `active_object_uids` / `add_object`; `update_environment_last_task_run_timestamps` (test-only
   caller, cloud-environment residue). Tests 4,289 default / 4,290 simplewarp (−3, deleted with
   their code: details-bar editor name, 2 AI-block creator avatar), `cloud_objects` +
   `cloud_object_models` + `persistence` + `cloud_object_persistence` 53 passed, 0 failed.
   Follow-ups: single-variant `Space` (thread through ~30 signatures, `CloudObjectLocation::Space`,
   `ExportId`); `WorkflowSource` / `WorkflowSelectionSource` / `BlockInfo` serde derives and
   plumbing (telemetry residue, never read for behavior); cloud-environment last-task timestamps;
   notebook baton/editor-state (`current_editor_uid`, `EditorState::OtherUser*`) now only
   reachable from stale metadata.

12. ~~Notebook baton, cloud-env timestamps, single-variant `Space`, workflow telemetry plumbing~~ —
   **4ha done (2026-09-28).** −0.7k lines in 54 files. Baton: `CloudViewModel` is deleted outright
   (its only reader was the baton; its folder-timestamp cache was never filled), with `Editor` /
   `EditorState` / `object_current_editor` / the 15-minute idle rule, `ActiveNotebookData::current_editor`,
   `set_current_editor`, and the create-time `current_editor_uid` write (always `None`: no user).
   `grab_edit_access(_or_display_access_dialog)` / `give_up_edit_access_and_start_viewing` →
   `start_editing` / `request_edit_mode` / `switch_to_view`; open-in-view (4bg) unchanged. The details
   bar label now follows the local mode ("Viewing" / "Editing"; it used to read "Viewing" always in
   production, "Other user is editing" for stale upstream metadata, which also blocked the edit toggle
   unless the recorded revision and metadata were over 15 minutes old). `current_editor_uid` stays in `CloudObjectMetadata`
   (sqlite `current_editor` column round-trips). Cloud-env timestamps: `last_task_run_ts` (never
   persisted, always `None`), `update_environment_last_task_run_timestamps`,
   `CloudModelEvent::EnvironmentLastTaskRunTimestampsUpdated`; the environment catalog sorts by name
   only (unchanged order). `Space` (never serialized) is gone with `CloudObjectLocation` (never
   serialized), `CloudObject::space` / `is_in_space` / `location`, `Space::owner` (→ `personal_drive`),
   the MCP-template `space` params, `ExportId`'s space half; `PERSONAL_SPACE_NAME` keeps the
   "Personal" breadcrumb root and bulk-export subdirectory; CloudModel `*_in_space(space)` queries →
   `active_cloud_objects` / `active_non_welcome_*` / `active_cloud_objects_directly_in_folder(Option)`
   (duplicate naming); `ActiveNotebookData::space` → `exists`. Telemetry residue (verified by
   compiling without the derives: nothing serializes them): serde off `WorkflowSource`,
   `NotebookLocation`, `SelectionMode`; `WorkflowSelectionSource` deleted with its plumbing through
   workspace/pane/input actions (stored, never read); notebook `BlockInfo` / `ActionEntrypoint` and
   the never-handled `EditorViewEvent::CopiedBlock` deleted (`copy`/`cut` lost their entrypoint).
   Tests 4,288 default / 4,289 simplewarp (−1, deleted with its code: env-timestamp recency order; the
   two baton tests re-pinned to open-in-view and stale-editor-does-not-block-editing),
   `cloud_objects` + `cloud_object_models` + `cloud_object_persistence` 30 passed, 0 failed.
   Follow-ups: `personal_drive()` is `None` in every production build (no user is ever set), so each
   creation gated on it is a silent no-op — "save as workflow" / temporary-workflow panes, restoring
   new notebook/env-var/workflow panes, the MCP template cloud object (the local installation still
   happens) — needs a local owner uid (product-visible, verify in the app first); the remaining
   ignored `EditorViewEvent` telemetry variants (`OpenedBlockInsertionMenu`, `OpenedFindBar`,
   `NavigatedCommands`, `ChangedSelectionMode`) and the `notebooks::telemetry` module name;
   `workflow_enums_with_owner`'s unused `AppContext` param.

13. ~~`personal_drive()` always `None`~~ — **4hb done (2026-09-28).** Regression confirmed: no
   production path calls `AuthState::set_user`, so `user_id()` and with it `personal_drive()` were
   always `None`. Broken since `c1a6c3136` in simplewarp (`local_only` returned before any user
   was adopted) and since 4gd (`656f60ab0`, the persisted-user read went) in default builds; unit
   tests hid it (`AuthStateProvider::new_for_test` sets a user). Silent no-ops: "save as workflow"
   / temporary / prompt workflow panes, restoring new notebook / env-var / workflow panes (restore
   errored), `CreatePersonalEnvVarCollection`, notebook "Copy to Personal", the suggested agent-mode
   workflow dialog, **adding AI rules** (Rules page and suggested-rule dialog), and the MCP template
   object. Fix: `personal_drive() -> Owner` is `Owner::User { LOCAL_USER_UID = "local_user" }`,
   no context, decoupled from `AuthState` (unchanged, so nothing flips to "logged in"; audited
   `user_id()` readers: MCP `is_author` compares `creator_uid` (always `None`) to `user_id()`, the
   agent-conversation owner filter, the sqlite load's default owner). Every `None` branch and the
   rules views' `owner` field are gone; the three pane `restore`s are infallible. Stale rows (other
   uids, `TEAM`) load and read as personal as before. Plans-as-notebooks stays off, now explicitly:
   a plan counts as saved only once its notebook has a server id, which no local object gets, so a
   local owner would have left plans "Saving" forever and held child-agent launch for the 30 s
   publication timeout; `save_to_notebook` only links an existing notebook (the Plans-folder
   creation, `UpdateManager::create_folder`, `get_server_conversation_id` are deleted). Also gone:
   unused `new_logged_out_for_test`. Tests 4,290 default / 4,291 simplewarp (+2: saving a new env-var
   collection creates it in the personal drive, `CreatePersonalEnvVarCollection` opens a pane; the
   plan-publication test re-pinned to "plans stay NotSaved"), `warp_server_auth` 1 passed, 0 failed.
   Follow-ups: verify in the app (save-as-workflow, add a rule, new env-var collection, restart
   restores them); the plan-publication wait / pending queue / `autosync_plans_to_warp_drive`
   plumbing is now dead (server-backed only).

14. ~~Plan publication, notebook telemetry residue, `share.rs`~~ — **4hc done (2026-09-28).** −1.5k
   lines in 39 files. Plan publication (server-backed only: a plan counted as saved once its notebook
   had a server id): `plan_publication.rs` (30 s wait), `publish_documents_for_conversation`, the
   pending-document queue, the server-backing reconciliation and CloudModel subscription,
   `AIDocumentSaveStatus` + `DocumentSaveStatusUpdated` and the plan header's save/"Saving"/"saved as a
   notebook" icons and `SaveToNotebook` action, `save_to_notebook`, `hydrate_saved_plan` (read_documents
   now reads the shared document model only), `update_plan_notebook_uid`. `run_agents` dispatches
   children synchronously inside `execute` (`PendingRunAgents::Publishing` and its `cancel_execution`
   gone; `pending` is a set). `autosync_plans_to_warp_drive`: setter, editor toggle, settings-page line,
   create-time default and the create_documents autosync call gone; the key stays in
   `AIExecutionProfile` / `ExecutionProfileFile` (orphaned, round-trips). Kept, local: the dirty-plan
   `pending_document_id` context, opening a plan from an existing plan notebook
   (`create_document_from_notebook`, edits mirrored via `UpdateManager`), the artifact "open plan"
   button for persisted `notebook_uid`s. Notebook telemetry: `EditorViewEvent::OpenedBlockInsertionMenu`
   / `OpenedFindBar` / `NavigatedCommands` / `ChangedSelectionMode`, `RichTextEditorModelEvent::
   SwitchedSelectionMode`, the `notebooks::telemetry` module (its `SelectionMode` only fed that event);
   `clear_command_selections` / `select_at` return `()`. `workflow_enums_with_owner` /
   `load_workflow_enums_with_owner` lost the unused ctx / take `&AppContext`. `warp_cli::share` was
   session-sharing residue (`--share` on `agent run`, always failed "not available"): deleted with
   `should_share`, `ShareSessionError`, `AgentDriverError::ShareSessionFailed`,
   `wait_for_session_shared`, `SetupStep::SharedSessionEstablishment`, `add_share_requests`; `--share`
   is now an unknown argument. Tests 4,287 default / 4,288 simplewarp (−3, deleted with their code:
   pending-queue refresh, cancel-during-publication, share-session error class; re-pinned: run_agents
   dispatches a local child with no wait, read_documents reads a plan owned by another conversation),
   `warp_cli` + `cloud_object_models` 91 passed (−13 share parser tests), 0 failed.
   Follow-ups: plan-notebook residue now reachable only from stale rows (`AIDocument::sync_id`,
   `create_document_from_notebook`, `Artifact::Plan.notebook_uid` open button, notebook
   `AttachPlanAsContext`); `ai_document_model.rs`'s file-wide `#![allow(warnings)]` hides dead
   methods (`delete_document`, `is_document_visible*`, `get_content`); `--idle-on-fail` /
   `--idle-on-complete` docs still describe the shared session.

15. ~~Warp-hosted agent sandbox residue~~ — **4hd done (2026-09-28).** −3.4k lines in 22 files.
   `agent_sdk/driver/cache_setup` + `crates/build_cache` (spacectl cache mounts; ran only when
   `detect()` said Namespace *and* Warp's infra set `WARP_BUILD_CACHE_ROOT`) with
   `SetupStep::CacheSetup`. `IsolationPlatformType::DockerSandbox` and the server-set
   `WARP_ISOLATION_PLATFORM` override (and the enum's unused `Serialize` / `serde` dep).
   `WARP_SANDBOX_DEADLINE` pre-kill timer (server-injected; the SIGTERM → finalize-then-exit path
   stays, it is generic). The dispatch-set Factory definition clone (`WARP_FACTORY_REPO_CLONE_URL` /
   `_DIR`) and `AgentDriverOptions::additional_source_repos` (server-supplied, always empty);
   `merge_repos_deduped` → `dedupe_repos` over the environment's repos. `--idle-on-fail` /
   `OZ_IDLE_ON_FAIL` (kept a failed run's shared session attachable; headless and unshared, it only
   delayed exit): flag, `linger_after_failure`, `arm_debug_window`; a terminal error now always exits
   at once, `--idle-on-fail` is an unknown argument. Kept, local: `detect()` for Docker /
   Kubernetes / Namespace (all read as `is_some()`: Linux OOM-score bump, `IS_SANDBOX`, harness
   skill-dir publishing), `prepare_environment` (repo clone + setup commands; also the local Docker
   sandbox), `--idle-on-complete` (docs reworded; a follow-up can still resume a completed run).
   Tests 4,279 default / 4,280 simplewarp (−8, deleted with their code: 3 cache-setup, 4 factory
   clone, 1 idle-on-fail window; merge tests re-pinned to `dedupe_repos`), `warp_cli` 55 passed
   (−6 idle-on-fail parser tests), 0 failed. Follow-ups: the driver's global-skill pipeline
   (`AuthState::global_skills()` is always empty: no user is ever set; `resolve_global_skills`,
   `clone_global_skill_repos`, `load_global_skills`, `ai/skills/global_skills.rs`);
   `AgentDriverOptions::task_id` / `parent_run_id` (always `None` from the CLI); `--environment`
   only resolves `ServerId`s, which no local environment has; `--snapshot-upload-timeout` and the
   rest of the snapshot args (check where snapshots go); recording finalization/upload.

16. ~~Agent-driver server residue~~ — **4he done (2026-09-28).** −3.5k lines in 40 files. Global
   skills (server-assigned per user; no user is ever set): `resolve_global_skills`,
   `clone_global_skill_repos`, `load_global_skills`, `ai/skills/global_skills.rs`,
   `User::global_skills`, `AuthState::global_skills()`. `AgentDriverOptions::task_id` /
   `parent_run_id` (only server-dispatched runs set them; local children launch through
   `local_harness_launch`, which keeps `task_env_vars` with its own ids), the driver's
   `parent_agent_id` stamping, `TerminalDriverOptions::task_id`. `--environment`: environments exist
   only as server-synced objects (no local creation path; the orchestration picker is for cloud
   runs), so the flag, `warp_cli::environment`, `resolve_environment`, the driver's environment prep,
   file-based MCP discovery/readiness wait, environment-skill loading, `EnvironmentNotFound` /
   `EnvironmentSetupFailed`, `SkillManager::is_cloud_environment` are gone; a config file's
   `environment_id` still parses (`deny_unknown_fields`) and is ignored with a warning;
   `prepare_environment` now dedupes its repos itself. Snapshot args (`--no-snapshot`,
   `--snapshot-*-timeout`): parsed, never read — deleted. Recording: publishing was an upload to
   server artifacts and the local adapter never offers the tools, so `RecordingController`,
   `recording_finalize`, the start/stop executors, the shell-command/use-computer action-group hooks,
   the driver's teardown finalization and its SIGTERM handler (existed only to let finalization run;
   SIGTERM now terminates with the default disposition), `FeatureFlag::VideoRecording`; persisted
   `StartRecording` / `StopRecording` actions stay and fail at once with "not available". Tests
   4,246 default / 4,247 simplewarp (−33, deleted with their code: 11 global-skills, 16 recording
   controller, 3 recording finalize, 2 env/global skill loading, 1 cloud-environment skill scope),
   `warp_cli` + `warp_server_auth` + `warp_features` 56 passed (snapshot parser test re-pinned to
   "removed server flags are unknown", −1 user test), 0 failed. Follow-ups: the docker sandbox pane
   looks up a hardcoded `ServerId` environment that cannot exist locally, so its env init always
   fails — with it `prepare_environment`, `register_cloned_repo(CloudEnvironmentPrep)` and the
   `CloudEnvMcpScanComplete` chain (now `#[allow(dead_code)]`, test-only) are dead; conversation
   `recording_spans_by_action_id` (reads only stale results); `computer_use`'s recorder / pointer
   sink; `UseComputer` / `RequestComputerUse` are also never offered by the local adapter.

17. ~~Docker sandbox pane and cloud-environment residue~~ — **4hf done (2026-09-28).** −8.3k lines in
   53 files. Docker sandbox pane **kept**: it runs a local `sbx run` container (Docker's sandbox
   CLI resolved from the user's PATH), no Warp service involved; only its post-bootstrap env init
   was server-bound (looked up hardcoded `ServerId` "SVhg783GBFQHk1OfdPfFU9" in the synced
   environment store, always "environment not found"). Deleted: `initialize_docker_sandbox_environment`
   and its workspace new-tab hook, `AvailableShell` / `ShellStarter::is_docker_sandbox`; with it
   `agent_sdk/driver/environment.rs` (`prepare_environment`, clone/checkout/setup-command helpers,
   `register_cloned_repo`), `SetupStep::EnvironmentRepoClone` / `EnvironmentSetupCommands`,
   `TerminalDriver::execute_silent_command` / `cd` / `cd_silent` / `active_session_shell_type`
   (`create_from_existing_view` is now test-only), `RepoDetectionSource::CloudEnvironmentPrep`,
   the `CloudEnvMcpScanComplete` chain (watcher countdown, manager wait-set and
   `CloudEnvMcpScanServer`; `maybe_autostart_file_based_servers` returns `()`);
   `FileMCPWatcherEvent` variants renamed `Parsed` / `Removed` / `Failed` (clippy
   `enum_variant_names`). Recording UI residue: conversation `recording_spans_by_action_id` and
   `RecordingSpanInfo/Status`, the "Recording active / Captured in recording" footer, the StopRecording
   "Open recording" button and `AIBlockAction::OpenRecordingArtifact` (always toasted a failure);
   persisted Start/StopRecording actions and results still load and render their cards.
   `computer_use`: `Recorder` / `create_recorder` and the mac/linux ffmpeg recorders, mock recorder,
   `post_process_recording`, overlay burn-in (`overlay.rs`), thumbnail, video-duration probe,
   `PointerSink` / `PointerSession` and `Options::pointer_sink`, `Action::is_no_op`,
   `main_display_dimensions`, deps `thiserror` and mac/linux `nix`/`tokio`/`uuid` extras;
   `RecordingCompletionStatus` stays (persisted results). Actor / control code untouched.
   `read_skills_from_files` (test-only) inlined into its tests. CLI `about` rewritten for the local
   agent CLI; docs.warp.dev line dropped from help. Tests 4,220 default / 4,221 simplewarp (−26,
   deleted with their code: 18 environment prep, 5 recording span, 2 cloud-env MCP scan, 1 recorded
   use-computer decoration); `computer_use` −78 on macOS (overlay, pointer session, recorder,
   metadata, thumbnail; plus 7 Linux-only recorder tests), `warp_cli` + `repo_metadata` pass, 0
   failed. Linux x11 pointer-sink removal is not compile-checked here (no Linux target). Follow-ups:
   cloud-environment object types (`CloudAmbientAgentEnvironment`, catalog, orchestration picker,
   context chip) only ever hold server-synced rows; the docker sandbox view's non-`local_tty`
   mock branch is dead (function is `local_tty`-only); `ai::artifacts` screenshot/file
   download buttons; remaining recording tool-call conversion.

18. ~~Cloud-environment objects, artifact downloads, recording conversion, `local_fs` / `local_tty`~~ —
   **4hg done (2026-09-28), two commits.** (a) −2.4k lines in 49 files. Environments: only
   server-synced rows ever existed, so `CloudEnvironmentCatalog`, `AmbientAgentEnvironment` (+
   StringModel/JsonModel, `GithubRepo`, `BaseImage`, providers/secrets config), the orchestration
   environment picker / `environment_snapshot` / default resolution / persistence / "select an
   environment" hint, the never-constructed `ChipMenuType::Environments` menu + sidecar (and the
   copyable-field options only it used), private setting `last_selected_environment_id`, unread
   `FeatureFlag::CloudEnvironments` / `cloud_environments` feature. Stale `CLOUDENVIRONMENT` rows are
   skipped at load (as `CLOUDAGENTCONFIG`). `SourceRepo` / `CodeForge` → `cloud_object_models::
   source_repo` (scheduled agents read them). Remote runs still carry `environment_id` on the wire;
   the UI never sets it. Artifact row: screenshot / file buttons (failed lightbox / failed download
   toast from server artifact storage) gone with `open_screenshot_lightbox`,
   `download_file_artifact`, `file_button_label`; persisted `Screenshot` / `File` artifacts still load
   and still match the list filters. Recording: `StartRecording { summary }`, `StopRecording
   { recording_id }` (capture config fed only the deleted recorder), window-target parsing,
   `InvalidRecordingWindowId`, success/discard to-API conversions (never produced locally); orphaned
   never-compiled `ai/agent/action/convert_tests.rs`. (b) −2.9k lines in 205 files: `local_fs` /
   `local_tty` were set unconditionally by build.rs in app and in ai / lsp / node_runtime /
   persistence / repo_metadata / warp_core (every bin, every config), so the features, the six
   crate build.rs files, every `not(...)` stub / `cfg_attr` / `cfg!` and the four `dummy_*` modules
   are gone and the enabled code is unconditional (incl. the docker sandbox view's mock-terminal
   branch). Scripted (attribute / `cfg_if!` / `cfg!` rewrite), reviewed by compile + diff; one
   clippy `const_is_empty` fix in the WSL error path. Tests 4,211 default / 4,212 simplewarp (−9,
   deleted with their code: 2 catalog, 1 env snapshot, 1 env pre-fill, 2 `set_environment_id`, 3
   `file_button_label`), `cloud_object_models` −20 (environment serde; 1 `SourceRepo` test kept),
   ai / lsp / repo_metadata / node_runtime / persistence / warp_core / warp_features 448 passed, 0
   failed. Linux x11 check of `computer_use` (4hf) still not possible: `x86_64-unknown-linux-gnu`
   std installs, but `freetype-sys` / `yeslogic-fontconfig-sys` (via warpui_core → font-kit) need a
   Linux C sysroot. Follow-ups: 76 bare `{ … }` blocks left where `#[cfg(feature = "local_fs")] {`
   stood (not flattened blindly: scope ends drop guards/locks); the whole Cloud/Remote orchestration
   mode (host / runner / auth-secret pickers, `RunAgentsExecutionMode::Remote`; remote children
   already fail with "not supported in this build"); `ScheduledAmbientAgent` (server-scheduled);
   `UploadFileArtifact` tool-call conversion; conversation-list Screenshot / File artifact filters.

19. ~~Cloud/Remote orchestration mode, scheduled agents, upload-artifact conversion~~ — **4hh done
   (2026-09-28).** −5.3k lines in 59 files. Orchestration is local-only: the Local/Cloud toggle,
   host / runner / API-key pickers (`host_picker.rs`, runner + auth-secret snapshots, auto-open
   create-key stubs), `OrchestrationConfigState::execution_mode` / `auth_secret_selection` /
   `AuthSecretSelection`, the host/secret providers, `CloudAgentSettings` (five private keys, none
   read elsewhere), `HarnessAvailabilityModel` auth-secret state and its event,
   `RunAgentsRequest::harness_auth_secret_name`, `StartAgentExecutionMode::Remote` (dispatch already
   failed "not supported in this build"), the unused `matches_active_config`, and the test-only
   `ensure_remote_child_conversation`; every `is_local` catalog branch collapsed to local.
   Persisted shapes: `OrchestrationConfig` drops its execution mode (a stale Remote config reads as
   local; `to_proto` writes Local); `RunAgentsExecutionMode::Remote` stays as a field-less marker
   from stale tool calls — the confirmation card edits and accepts it as a local run, while an
   unattended dispatch (autonomous / always-allow / approved plan) fails with "Remote child agents
   are not supported in this build."; `RunAgentsLaunchedExecutionMode::Remote` (serde) kept.
   `ScheduledAmbientAgent` + its Cloud aliases and app `StringModel` glue gone, stale
   `SCHEDULEDAMBIENTAGENT` rows skipped at load; `SourceRepo` kept (`AgentConfigSnapshot` still
   reads it); `scheduled_ambient_agent.rs` → `agent_config_snapshot.rs`. `UploadFileArtifact`
   (server-offered upload to server storage): `AIAgentActionType::UploadArtifact`,
   `UploadArtifactResult`, the card, CLI text/JSON output, redaction, executor stub; persisted calls
   and results now have no client representation (proto-level YAML export untouched).
   `ArtifactFilter::Screenshot` / `File` are left: persisted artifacts still load, and the filter
   is only ever `All` (the loaded snapshot deserializes with `.ok()`). Clippy: boxed
   `terminal::view::Event::OpenCodeReviewPaneAndScrollToComment::comment` (`large_enum_variant`
   after the Remote payloads shrank the runner-up). Tests 4,152 default / 4,153 simplewarp (−59,
   deleted with their code: 11 host picker, 13 card remote/runner/toggle, 7 remote start-mode, 7
   host/api-key/cloud snapshot, 5 edit-state toggle/secret, 3+2 validation/config toggles, 5
   run_agents auth-secret (+1 persisted-Remote refusal), 3+1+1 upload artifact, 1 runner flag, 1
   `ensure_remote_child_conversation`), `ai` −14 (remote match / round-trip),
   ai + cloud_object_models + cloud_objects 204 passed, 0 failed. Follow-ups: stale remote-child
   rows (`is_remote_child` readers: pill-bar cloud badge, restoration skip, cloud-cancel candidate;
   test-only `mark_conversation_as_remote_child`, `start_new_child_conversation`'s always-false
   `is_remote`); `ArtifactFilter` itself (never set but `All`); `RunAgentsAgentRunConfig::
   agent_identity_uid` (wire field, locally always rejected); the `from_restore` flag on
   `OrchestrationConfigUpdated` (its only UI consumer, the create-key auto-open, is gone).

20. ~~Stale remote-child, plan-notebook and `ai_document_model` residue, `ArtifactFilter`~~ — **4hi
   done (2026-09-28).** −0.4k lines in 39 files. Remote children: `AIConversation::is_remote_child` /
   `mark_as_remote_child` and every reader (pill-bar cloud badge, restoration skip, cloud-cancel
   candidate, local-child icon check, agent-view placeholder check, swap-failure log), the
   unified-stack persist guard, test-only `mark_conversation_as_remote_child`,
   `start_new_child_conversation`'s `is_remote`, `AgentConversationData::is_remote_child` (stale
   `"is_remote_child":true` rows still parse; the key is ignored). A stale remote-child row now
   restores as an ordinary child (hidden pane with its local transcript, status derived from its
   tasks) — pinned by `restored_stale_remote_child_loads_as_local_child` and the persistence
   stale-row test. `RunAgentsAgentRunConfig::agent_identity_uid` gone (proto field untouched, so
   persisted calls parse; a stale call naming an identity now runs as the caller instead of being
   rejected). `OrchestrationConfigUpdated::from_restore` gone. Plan notebooks **kept**: a persisted
   `Artifact::Plan.notebook_uid` opens its notebook from local sqlite (`open_warp_drive_object_in_new_pane`),
   the notebook's "Attach plan as context" inserts `<plan:id>`, and `create_document_from_notebook`
   + `AIDocument::sync_id` mirror edits back via `UpdateManager::update_notebook_data` — all local;
   `DriveObjectType::Notebook { is_ai_document }` (plan icon) is reachable from those panes. Deleted:
   the workspace duplicate-toast check for synced plans (`UpdateManager` never emits
   `ObjectOperationComplete` for `Update`) and `PaneGroup::contains_ai_document`.
   `ai_document_model.rs`: file-wide `#![allow(warnings)]` removed; `delete_document`,
   `is_document_visible_by_conversation`, `is_document_visible`, `update_title` deleted,
   `get_content` test-only, `StreamingDocumentsCleared` / `DocumentVisibilityChanged` lost their
   unread payloads, collapsible ifs / `or_default` fixed, three `too_many_arguments` allows (repo
   idiom). `ArtifactFilter`, `AgentManagementFilters::artifact`, `artifacts_match_filter` gone (only
   `All` was ever set; a stale `"artifact"` key is ignored). Tests 4,149 default / 4,150 simplewarp
   (−3, deleted with their code: mark-remote-child persist, identity-uid rejection, file artifact
   filter; re-pinned: stale remote child loads as a local child, nested-parent lazy restore uses a
   local mid-level child), persistence + ai + warp_features 218 passed, 0 failed.
   Follow-ups: `OrchestrationUnifiedStack` (dogfood-only) now gates only the agent-view
   `is_existing_child_placeholder` check (shared-session viewers); `is_viewing_shared_session`
   readers (session sharing is gone); the other `AgentManagementFilters` fields (`creator`,
   `environment`, `source`) and the always-`None` `agent_management_filters` snapshot write.

21. ~~Shared-session viewer residue, `OrchestrationUnifiedStack`, agent-management filters~~ — **4hj
   done (2026-09-28).** −2.0k lines in 71 files. Viewer: `AIConversation::is_viewing_shared_session`
   (+ setter, `new`/`start_new_conversation` arg, history-model setter; never set true) and every
   reader collapsed to the local path (navigation hiding, persist skip, follow-up/cancel/auto-resume
   gates, viewer start-time/TTFT derivation, subtask exchange re-pointing, search-subagent temp-dir
   cleanup, AI-document "follow latest version", agent-footer transcript check, fork-from-good-state,
   pane cloud-cancel candidate + the two toast-only `cancel_task_*` stubs); the viewer input
   reconstruction chain (`should_convert_input_messages` on `Task::new_subtask` / `add_messages` /
   `upsert_message` / `update_exchange_from_messages`, `user_inputs_from_messages`); the executor's
   always-false `is_shared_session_viewer` + `NotExecutedReason::WaitingOnSharer`;
   `AgentViewEntryOrigin::SharedSessionSelection`; command-palette / `@`-menu viewer flags (setters
   had no callers); queued-prompts `can_send_prompt` (always true) and its read-only tooltip;
   `PromptType::Static` (never built; `PromptType` is now a struct over `CurrentPrompt`); the
   profile selector's `is_viewer`; `apply_external_input_*` + `SessionSharingApply`; the unused
   `session_sharing` cargo feature. Ctrl-C harness cancel (REMOTE-2597) was scoped to the viewer
   input path only (`write_viewer_bytes_to_pty`, no other caller): the grace-window state machine in
   `CLIAgentSessionsModel`, `CLIAgentSessionStatus::Cancelled`, `write_user_bytes_to_pty`'s bool and
   the `CtrlCCancelsThirdPartyHarness` flag are gone. `OrchestrationUnifiedStack` gated nothing else:
   removed. Filters: `AgentManagementFilters` had no setter anywhere (its view is gone) — every
   `get_entries` caller passed the default — so the whole struct goes (owner/creator/source/
   environment/status/created-on/harness, `OwnerFilter` and the other filter enums, `matches_*`,
   the entry's filter-only `creator` / `source` / `environment_id` display fields, `AgentSource`);
   `get_entries` takes no filter. `WindowSnapshot::agent_management_filters` /
   `PersistedAgentManagementFilters` gone: the sqlite column stays, written `NULL`, never read.
   `EntrypointType::SharedSession` kept (serde). Tests 4,127 default / 4,128 simplewarp (−22,
   deleted with their code: 18 Ctrl-C state machine, 2 viewer Ctrl-C, harness is-filtering, filter
   serde; can-send test rewritten to its non-empty-input half), warp_features 1 passed, 0 failed.
   Follow-ups: the input CRDT sync plumbing (`latest_buffer_operations`,
   `DeferredRemoteOperations`, `process_remote_edits`, `EditorEvent::UpdatePeers` /
   `Event::EditorUpdated`); `session_sharing_protocol` dep (5 type uses); `is_dummy_cloud_mode_session`;
   `AIContextMenu::is_in_ambient_agent` (setter has no callers); the entry's other cloud-only
   fields (`executor`, `run_time`, `session_status`, `has_ambient_run`, `PrincipalType`).

22. ~~Input CRDT peer sync, `session_sharing_protocol`, dummy cloud-mode session, entry cloud fields~~
   — **4hk done (2026-09-28).** −1.0k lines in 35 files. Input peer sync: `Input::
   latest_buffer_operations` (+ integration test `test_latest_buffer_operations` and its assertion),
   `DeferredRemoteOperations` (its `latest_block_id` survives as `Input::buffer_block_id`, still
   gating buffer reinit on user-command completion and re-keyed after bootstrap),
   `process_remote_edits` / `refresh_deferred_remote_operations`, `input::Event::EditorUpdated`;
   editor `Event::UpdatePeers` / `EditorModelEvent::UpdatePeers`, `apply_remote_operations` (view +
   model), the collaborative/non-collaborative buffer-event split, the viewer "display-only
   ephemeral" (`show_display_only_empty_buffer`, `exit_ephemeral_loading_state`, no callers), the
   peer registry (`register/unregister_remote_peer`, `set_remote_peer_selection_data`,
   `Buffer::registered_peers`, `Peer`, `PeerSelectionData`) and remote-cursor rendering (avatars,
   `RemoteDrawableSelectionData`, `DrawableSelection::replica_id`, `CursorData::replica_id`, the
   remote beam width, `cursor_avatar_*`); `Avatar::with_status_element`. **Kept:** the `Buffer` CRDT
   itself (op generation, lamport/undo history, buffer-level `UpdatePeers` emission, now ignored by
   `EditorModel`); the remote-op intake (`Buffer::apply_ops` + helpers, `DeferredOperations`,
   `RemoteSelection::observed`) is `#[cfg(test)]` because 25 buffer tests (convergence, undo,
   selection merging) drive it. `session_sharing_protocol` dependency gone (workspace, app,
   `cloud_object_persistence` dev-dep, about.toml note): the entry's `SessionId` field and three
   uncalled `From`/`TryFrom` impls (`InputMode`, `ServerConversationToken`) deleted.
   `TerminalModel::is_dummy_cloud_mode_session` (always false; `new_internal` folded into `new`)
   and its three readers; `AIContextMenu::is_in_ambient_agent` + setter + the
   `get_categories_for_mode` param. `AgentConversationEntry`: `ambient_agent_task_id`,
   `session_id`, `executor`, `run_time`, `session_status`, `has_ambient_run`, `is_cloud_agent_run`,
   `PrincipalType`, `AgentConversationPrincipal`, `SessionStatus` (all always `None`/false for local
   entries). `WITH_LOCAL_SESSION_SHARING_SERVER` / `SERVER_ROOT_URL` / `WS_SERVER_URL` rerun lines in
   `app/build.rs` and the feature-to-env shim in `script/run`. Tests 4,127 default / 4,128
   simplewarp (unchanged; the removed integration test is not in the lib suite).
   Follow-ups: the `Buffer` CRDT exchange (buffer-level `UpdatePeers`, test-only `apply_ops`, remote
   selections map) could go with its 25 tests; `ActiveAgentViewsModel` ambient sessions
   (registered from transcript-viewer / details-panel task ids); `EditOrigin::RemoteEdit` (only
   the test-only intake emits it).

23. ~~Warp-internal channel bins, `warp_channel_config`, Warp-infra CI~~ — **4hl done (2026-09-28).**
   −4.6k lines in 55 files. The `dev` / `warp` (local.rs) / `preview` / `stable` bins loaded their
   `ChannelConfig` from `warp-channel-config`, a generator `cargo install`ed from the private
   `warpdotdev/warp-channel-config` repo over SSH (or embedded by `app/build.rs` from it in
   `release_bundle` builds); without it they panicked at startup. Gone with them: the
   `warp_channel_config` crate, `app/build.rs` `generate_channel_config_if_needed`,
   `script/install_channel_config` (+ calls in `script/run`, `install_cargo_build_deps`, the
   `prepare_environment` CI action and its SSH-key step/input), their `[[bin]]` /
   `[package.metadata.bundle.bin.*]` entries, the `preview_channel` feature (its only cfg reader,
   the UDI default in `settings/input.rs`, folds to the non-preview branch), `LOCAL_FLAGS` + its
   warp_features test (`LocalClaudeCodexChildHarnesses` stays: still enabled by its cargo feature).
   `cargo run` already defaulted to `warp-oss` (`default-run`); `script/run` / `script/macos/run`
   now always build `warp-oss` (`WarpOss.app`, `~/Library/Logs/warp-oss.log`); the dead `--host-id`
   flag (`WARP_CLOUD_MODE_DEFAULT_HOST`, no reader) is gone. Bundle scripts
   (`script/{macos,linux}/bundle`, `windows/bundle.ps1`) default to and only accept `--channel oss`;
   `script/linux/bundle` also had a syntax error left by 4bp (dangling `elif`), fixed. CI: deleted the
   Warp-infra-only workflows (`create_release` GCS/notarization/channel-config releases,
   `cut_new_release*` / `delete_release` (channel-versions repo), `changelog_draft`,
   `feature_flag_cleanup`, `update-*-local`, `docubot_reply_to_comment`, `close_stale_fix_prs`,
   `warp_cleanup_fix_prs` (Oz agent / Warp API key), `repo-sync`, `sync-pr-checks`,
   `populate_build_cache`, `publish-agent-dev-image` + `script/push-dev-image` + `docker/agent-dev`
   (warp-internal-dev image), `notify-docs-settings-changed`), their actions (`get_channel_config`,
   `docubot`, `bundle_arch_package`), `release_configurations.json` + README,
   `script/create_release_tag_and_branch`. `ci.yml` kept (fmt/clippy/tests/release-check) minus
   the gcloud SSH-test auth (SSH tests always excluded now), trunk.io uploads, repo-sync marker
   check and the agent-mode-evals job. `.vscode` launch/tasks point at `warp-oss`. **Kept:** the
   `Channel::{Stable,Preview,Dev,Local}` variants (paths / URL schemes / ports / icon names still
   match on them; nothing constructs them now), `app/channels/*` icons (`bundle_simplewarp` uses
   `stable`'s), `check_approvals` / `label_external_contributors` / `stale_requested_changes_prs`
   (plain GitHub bots). Tests 4,127 default / 4,128 simplewarp (unchanged), warp_features +
   warp_core 43 passed, 0 failed.
   Follow-ups: collapse `Channel` to `Oss` / `Integration` (paths, `http_server` ports,
   completer channel list, `preview_config_migration`, `is_dogfood`, app-icon names); the stale TUI /
   CLI artifact paths and Warp notarization/GCP-secret signing in the bundle scripts and
   installers; `ci.yml` is still gated on `repository_owner == 'warpdotdev'` and Warp's
   Namespace / large runners, so it never runs on this fork.

24. ~~SSH remote-server daemon residue (4gr follow-ups)~~ — **4hm done (2026-09-28).** −12.0k lines in
   138 files (−7.3k of it upstream remote-server design docs under `specs/`). The daemon was the only
   source of `LocalOrRemotePath::Remote` / `RemotePath` / `HostId` values (`HostId::new` had no
   caller; `pwd_as_local_or_remote` already returned `None` for remote sessions), so gone: the
   `Remote` variant, `warp_util::{remote_path, host_id}` and every `Remote` arm across file tree,
   left/right panel, working directories, code review, git dialog, global search, file search,
   notebooks, skills (`SkillPathOrigin::Remote`), project rules (remote global rules), and the
   editor (`open_remote_buffer`, `is_remote_disconnected` + banner + toast,
   `ImmediateSaveError::RemoteDisconnected`, `FileSaveError::RemoteError`). `LocalOrRemotePath`
   stays a single-variant enum: its `{"Local": …}` JSON is persisted in code-pane sources and
   skill references. `repo_metadata`: `RemoteRepoMetadataModel`, the incremental-update emitter
   (`IncrementalUpdateReady`, `emit_incremental_updates`, `new_with_incremental_updates`) and the
   symlink-target watches it alone enabled, the client-side apply path
   (`apply_repo_metadata_update`, `from_file_tree_entry`), `DetectedRepositories` remote roots;
   `RepositoryIdentifier` is now a newtype over `StandardizedPath`. File-tree remote roots
   (`remote_host_id`, `set_remote_root_directories`, remote-item action gating). Flattened:
   `DiffStateModel`, `GitRepoStatusModel`, `GitHubRepoModel` (the wrapper enums go; the former
   `Local*` models take their names, files `local.rs` → `model.rs`); `DiffStateModelMap`,
   `PaneGroupRepositoryRoots::insert`, `register_remote_repo` / `register_terminal_for_repo`, the
   git dialog's remote Changes-box refresh and `DiffMetadataAgainstBase::files`.
   `BootstrapSessionType` merged into `SessionType` (identical, not serde). The tmux deprecation
   banner (pointed users at the SSH extension) and its `UseSshTmuxWrapper` /
   `SshTmuxDeprecationNoticePending` settings: the orphaned `warpify.ssh.use_ssh_tmux_wrapper` /
   `ssh_tmux_deprecation_notice_pending` keys still load harmlessly. `ci.yml`
   `EXCLUDE_REMOTE_SERVER_TESTS_FILTER`; daemon wording in comments. `NotebookLocation::RemoteFile`
   (never built). `SettingsViewEvent::OpenCustomRouterEditor` now boxes its router (clippy
   `large_enum_variant` once `PaneEvent` shrank). **Kept** (plain SSH): `RemoteCommandExecutor`,
   `SessionType::WarpifiedRemote`, `CodingPanelEnablementState::RemoteSession`, the plain-SSH
   `RemoteFileOperationsUnsupported` / "Remote codebase search is not enabled" fallbacks, warpify
   and SSH hooks. Tests 4,114 default / 4,115 simplewarp (−13, deleted with their code: 11 remote
   skill/path, 1 wrapper variant, 1 roots `insert`); repo_metadata + ai + warp_util 404 (−24, all
   remote model / remote path / symlink-target / client-apply tests).
   Follow-ups: flatten `LocalOrRemotePath` itself (serde shape; `to_local_path()` is now always
   `Some`); the `RepoMetadataModel` wrapper forwards to one sub-model; the `ExitShell` DCS hook
   (emitted by the SSH bootstrap scripts to tear down the daemon's `remote-server-proxy`, a no-op
   in the client now); `SessionType`'s `Local` / `WarpifiedRemote` naming.

25. ~~Collapse `Channel`; TUI / Warp-signing residue in bundle scripts (4hl follow-ups)~~ — **4hn done
   (2026-09-28).** −2.5k lines in 78 files (most of it the dev/local/preview icon sets). `Channel` is
   now `Oss` / `Integration` (the only two any bin constructs; not serde — `Display` only feeds a
   tracing tag). Every match lost its `Stable` / `Preview` / `Dev` / `Local` arms; the Oss /
   Integration values are unchanged (`.warp-oss` / `.warp-integration` dirs, `WarpOss` GUI app id,
   `warposs` scheme, port 9282, `warp-oss` CLI name, `warp_2` default icon — `paths_tests` pins the
   dirs). `is_dogfood()` (false for both) is gone and its readers folded to the false branch: the
   `safe_*` log macros emit only `safe:` (the `full:` args are still type-checked by a hidden
   `__discard_full_log_args!` so their captures stay used); the dev-only "Fork from here" menu items
   + `ForkAIConversationFromExactExchange`; `CopyAIDebuggingLink` / `CopyConversationId` (the
   non-dogfood "Copy debugging ID" stays); `load_agent_mode_conversation` (downloaded a Warp-server
   `/debug/maa/` link) + `LoadAgentModeConversation` + its binding; the AI error card's Send
   Feedback button is now unconditional; `should_suppress_during_recovery`; the sandboxed-CLI
   computer-use default; `CloudAgentComputerUseEnabled` default `false` (key kept);
   `Experiment::can_use_user_override` (→ `allow_user_overrides_in_stable`). `enable_debug_features`
   is `cfg!(debug_assertions)`. Runtime flag set unchanged: `features::enabled_features()` never read
   the channel, and `DOGFOOD_FLAGS` is only read by the schema/default-settings generators' string
   `--channel` switch. `preview_config_migration` (Preview-only `~/.warp` → `~/.warp-preview`
   symlinker; its unit tests, integration-testing helper, integration test and `specs/QUALITY-408`)
   deleted. Local-channel checks (dock icon reset, appearance-page hint, default-terminal) always
   pass now. The completer registers the `warp_cli` signature for `warp-oss` only (was `oz` /
   `oz-preview` / `oz-dev`, Warp's installed CLI names). `app/channels/{dev,local,preview}` deleted
   (`stable` kept for `bundle_simplewarp`, `oss` for the bundle scripts / Cargo bundle metadata).
   Scripts: `--artifact tui` gone from `script/{macos,linux}/bundle` and `windows/bundle.ps1`
   (+ `tui-installer.iss`, `test_tui_installer.ps1`, `REQUIRE_SIGNATURES`); `script/macos/bundle`'s
   Warp Developer ID keychain / codesign / notarytool / staple path and `--read-passwords-from-env`
   gone (`--selfsign` ad-hoc/Apple Development signing kept; default was already unsigned);
   `windows/build_inno_sign_tool_command.ps1` (Azure Trusted Signing) and
   `linux/sign_arch_packages` (orphans of the deleted release workflow); gcloud install/auth for the
   SSH integration tests in the bootstraps + `install_test_deps` (and `--skip-gcloud-auth`); the
   `release-tui*` / `dev-remote` cargo profiles. `bash -n` clean on every edited script. **Kept:**
   `DockTilePlugin`'s own dev/preview/local icon fallback (ObjC, bundle-id keyed); the
   `windows-installer.iss` channel switch; `prepare_bundled_resources` channel-gated skills; the
   generators' `stable/preview/dev` strings; `ci.yml` (user decision: rewriting it to run on the fork
   would start consuming Actions minutes). Tests 4,108 default / 4,109 simplewarp (−6, the deleted
   migration tests), warp_core + http_server + cloud_object_models + warp_features 53,
   warp_completer 174 / 123 (v2), 0 failed.
   Follow-ups: `ForkFromExchange::fork_from_exact_exchange` is now always `false` from the menus
   (check the rewind caller before folding); the `safe_*` macros' `full:` arms could be dropped at
   the ~90 call sites; `ChannelState::url_scheme()` is `warposs` in simplewarp while its plist
   registers `simplewarp` (pre-existing mismatch: deep links / MCP OAuth redirect use `warposs`);
   `windows-installer.iss` / DockTilePlugin channel branches; `app_services/linux` D-Bus default
   `dev.warp.WarpLocal`.

26. ~~URL-scheme fix; 4gq / 4hk / 4hn dead hooks~~ — **4ho done (2026-09-28).** **Fix:**
   `ChannelState::url_scheme()` was keyed on `Channel` and returned `warposs` for the simplewarp
   bin, whose Info.plist (embedded in `bin/simplewarp.rs` and written by `script/bundle_simplewarp`)
   registers `simplewarp`: deep links (`simplewarp://…`) were rejected by `validate_custom_uri`, and
   the MCP OAuth `redirect_uri` (`warposs://mcp/oauth2callback`) was never routed back to the app, so
   MCP OAuth sign-in could not complete. The scheme is now `ChannelConfig::url_scheme`, set by each
   bin (`simplewarp` / `warposs` / `warpintegration`); `uri_tests` pins each bin's scheme against its
   plist(s). `ChannelConfig` lost its unused serde derives. **Cleanup:** `SystemInfo` is now just the
   Windows process-table lookup (the 5 s poll, `MemoryUsageHigh`, the excessive-memory check, the
   `memory_footprint` module on all four platforms, app's `mach2` dep and the
   `Win32_System_ProcessStatus` feature); `crash_recovery::Event` (neither variant observed);
   warpui's `on_gpu_driver_selected` / `on_gpu_device_info_reported` hook and the `GPUDeviceInfo`
   types behind it (mac Metal + wgpu + winit); the ambient-session half of `ActiveAgentViewsModel`
   (`ConversationOrTaskId` folded to `AIConversationId`) and everything that could only feed it —
   `PaneGroup::new_for_conversation_transcript_viewer` / `create_conversation_viewer` (no callers),
   `ViewingAmbientConversation`, `TerminalModel::ambient_agent_task_id` / `is_cloud_agent_conversation`,
   `TerminalView::is_cloud_agent_session` and its cloud-icon/tab-indicator branches
   (`Indicator::AmbientAgent`), `OpenConversationTranscriptViewer` (never dispatched), the icon
   helpers' always-false `is_ambient` input; `EditOrigin::RemoteEdit` (test-only intake now emits
   `SystemEdit`); `ForkFromExchange::fork_from_exact_exchange` (rewind uses `fork_conversation`, not
   this path; the reconciliation test now stops at a user query instead). Non-mac edits
   (winit window, `crash_recovery` on_frame_drawn in `lib.rs`, Windows `memory_footprint` removal)
   were not compile-checked. `app_services/linux`'s `dev.warp.WarpLocal` proxy default left as is:
   not live (the only proxy build sets `.destination()` / `.path()`). Tests 4,105 default / 4,106
   simplewarp (−4 deleted with their code, +1 scheme pin), warp_core + warpui + warpui_core 390, 0
   failed. Follow-ups: `ConversationTranscriptViewerStatus::ViewingLocalConversation` is never
   constructed (only `Loading` is set); `IconWithStatusVariant`'s `is_ambient` still has
   `vertical_tabs` `true` producers (summary CLI rows) worth checking; `Channel::cli_command_name()`
   says `warp-oss` for the simplewarp bin too; the `app_services/linux` D-Bus default.

27. ~~Stale text, small follow-ups, final survey~~ — **4hp done (2026-09-29).** Code −0.9k net lines in
   101 files; −18.2k lines of upstream specs. **Follow-ups:** the transcript-viewer status was
   live (it is the read-only placeholder while a local conversation loads), so only the
   never-built `ViewingLocalConversation` went and the single-variant enum became a bool
   (`set_loading_conversation_transcript`). `IconWithStatusVariant` / `SummaryPaneKind` lost
   `is_ambient` (every producer passed `false`; the vertical-tabs `true` arm matched nothing, so
   no bug, just a dead cloud-lobe path) together with the cloud-lobe renderer,
   `OZ_AMBIENT_BACKGROUND_COLOR`, the collage shift and `StatusColorStyle`. CLI name: the
   simplewarp bin installs/symlinks as `simplewarp` (was `warp-oss`): `ChannelConfig::cli_command_name`
   is set per bin like `url_scheme`, read via `ChannelState::cli_command_name()` by the CLI install,
   completions (`warp_cli` + completer registration) and help text. The `safe_*` log macros are gone:
   every call site is now the `log::*` of its old `safe:` message (behavior unchanged; the `full:`
   args never logged), variables that fed only `full:` are `_`-ignored, `InlineDiffViewEvent::FailedToSave`
   and `Export::handle_failure` lost the error they only logged; `safe_anyhow!` / `safe_eprintln!`
   had no callers. DockTilePlugin: only `warp_2` fallback (dev/local/preview icons deleted);
   `windows-installer.iss`: oss/integration only, `IsNotStable` (unused) gone; `app_services/linux`
   D-Bus proxy default is now `dev.warp.WarpOss` (still overridden by the only proxy build).
   **Stale text:** Sentry-era comments in 15 files reworded; the privacy-policy settings widget,
   app-menu item, `ViewPrivacyPolicy` action/binding and `PRIVACY_POLICY_URL` removed (no README
   privacy section to point at); the `logging-and-error-reporting` skill (and `review-pr-local`'s
   log bullet) rewritten for local-only logging; AGENTS.md "both front-ends" and the feature-flag
   how-to (`DOGFOOD_FLAGS` / `PREVIEW_FLAGS` are enabled by no bin); a SimpleWarp note atop README and
   FAQ (upstream text kept); 100 upstream specs for fully removed features deleted (TUI, cloud mode,
   local-to-cloud handoff, session sharing / remote control, Sentry bridge, server-only `oz run` /
   named agents / factory files, credits, orchestration host picker / create-API-key, server
   forking); the empty `server::graphql` module. **Survey:** see "Remaining queue" at the top of
   Next. Not compile-checked here: the Windows/Linux-only edits (installer, D-Bus default, Windows
   env / msys2 / wsl log sites), DockTilePlugin (`clang -fsyntax-only` clean). Tests 4,103 default /
   4,104 simplewarp (−2: the ambient circle-colour and ambient-Claude summary tests), warp_core +
   completer + warp_cli + editor + ai + repo_metadata + vim + warp_errors + watcher 1,152,
   completer v2 123, 0 failed.

28. ~~R1: cloud-agent OTLP tracing export~~ — **4hq done (2026-09-29).** −1.2k lines in 15 files.
   **Evidence:** export was armed only when `WARP_CLOUD_AGENT_OTLP_ENDPOINT` *and* a currently valid
   `WARP_CLOUD_AGENT_OTLP_TOKEN` + `_TOKEN_EXPIRES_AT` (RFC3339 UTC, bearer auth, token scrubbed from
   env) were set at dispatch; no setting, CLI flag or doc exposes it, the exporter only shipped spans
   carrying the `tags.cloud_agent` routing marker under a fixed `warp-cloud-agent` service name, so
   it was Warp's server-dispatch plumbing, not a user-facing OTLP export. Deleted `app/src/tracing{.rs,/}`
   (native exporter, shutdown-aware tracer/span registry, cloud-agent auth + its 4 tests), the
   `X-Warp-Traceparent` trace-link header in `http_client` (only valid under that subscriber; its 3
   tests, so `http_client` now has none, and its `tracing`/OTel deps), every `tags.cloud_agent = true`
   span field (15 sites), and the `opentelemetry`, `opentelemetry-http`, `opentelemetry-otlp`,
   `opentelemetry_sdk`, `tracing-opentelemetry` and `tracing-subscriber` deps (−135 Cargo.lock lines).
   **Kept:** behavior without the env vars: `run_internal` still installs the global `tracing`
   `NoSubscriber` (so `tracing` never writes log lines), local logging via `warp_logging` is
   untouched, and the `tracing` spans/`instrument` attributes stay (inert, cheap). Tests 4,099 default /
   4,100 simplewarp (−4), 0 failed.

29. ~~R2 + R3: cloud handoff and cloud-agent context residue~~ — **4hr done (2026-09-29).** −0.43k net
   lines in 29 files. **R2:** the never-dispatched `OpenLocalToCloudHandoffPane` /
   `AutoHandoffActiveAgentToCloud` actions + `AutoCloudHandoffTrigger`, `AUTO_CLOUD_HANDOFF_PROMPT`,
   `LocalToCloudHandoffIntent`, the no-op `start_local_to_cloud_handoff{,_from_source}` /
   `record_automatic_handoff_failed`, `Workspace::terminal_view` (its only caller),
   `ai/blocklist/handoff` (`PendingCloudLaunch`, `HandoffLaunchAttachments`) and the
   `ambient_agents::task::AttachmentInput` it alone used; `InputTypeAutoDetectionSource::CloudHandoffEnter/Exit`
   (in-memory only, never constructed). **R3:** `AgentViewEntryOrigin::ThirdPartyCloudAgent` and
   the likewise never-constructed `Tui`; `ActiveConversationContext::is_cloud`,
   `BlockList::is_cloud_conversation_context` and the `is_cloud` params of
   `enter/set_active_conversation_context`; `is_in_cloud_context` (its 4 callers now read
   `is_conversation_transcript_viewer()` directly, which was the only live half);
   `Availability::NOT_CLOUD_AGENT` (OR'd into every GUI availability, 12 command uses);
   `ROOT_CLOUD_MODE_PANE_KEY` (never set); `app/src/ai/cloud_agent_config` and
   `cloud_object_models::cloud_agent_config` (`AgentConfig` / `CloudAgentConfig`, no user);
   `CloudAgentComputerUseWidget`, `ToggleCloudAgentComputerUse` and
   `is_cloud_agent_computer_use_enabled`. **Kept:** `AgentToolbarItemKind::HandoffToCloud`,
   `CancellationReason::AutomaticCloudHandoff`, the `DidAddHandoffChipToToolbar` and
   `cloud_agent_computer_use_enabled` setting keys, `JsonObjectType::CloudAgentConfig`, and
   `DefaultSessionMode::CloudAgent` — a persisted value already degraded to Terminal; now pinned by
   `persisted_cloud_agent_default_session_mode_loads_and_falls_back_to_terminal`. No behavior
   change. **Follow-ups:** `terminal/input/slash_commands/mod_tests.rs` is an orphan (no `mod`
   includes it; its NOT_CLOUD_AGENT test was deleted anyway); `slash_commands/cloud_mode_v2_view.rs`
   (1.2k lines, `CloudModeV2SlashCommandView`) has no importer outside its re-export — check next.
   Tests 4,100 default / 4,101 simplewarp (+1), cloud_object_models 10, 0 failed.

30. ~~R4: always-logged-out `AuthState` walls~~ — **4hs done (2026-09-29).** −1.3k net lines in 96
   files. `crates/warp_server_auth` and `app/src/auth` are gone (`AuthState`, `AuthStateProvider`,
   `AuthManager` + its login-gated toast, `User` / `Credentials` / `AnonymousUserType` /
   `PersonalObjectLimits` / `PrincipalType`), as are `cloud_object/object_limits.rs`,
   `WorkspaceAction::blocked_for_anonymous_user`, the `IsAnonymousUser` context flag (no binding read
   it) and the dead `SkipFirebaseAnonymousUser` flag + cargo feature. **Moved:** `UserUid` (persisted
   owner shape) to `cloud_objects::user_uid`; the experiment-bucketing id to `experiments`
   (same private pref key `ExperimentId`, now created on first bucket lookup, cached per process).
   **Collapsed to the logged-out behavior (no change):** Get Started tab / agent-onboarding
   auto-trigger on new windows (never fired: required a non-anonymous, not-onboarded user) —
   `should_trigger_get_started_onboarding`, `check_and_trigger_onboarding`,
   `trigger_agent_onboarding`, `should_show_agent_onboarding` deleted (the `AddGetStartedTab`
   action stays); the tab-bar avatar is always the gear icon (no tooltip; `AvatarContent::Image` had
   no other producer); AI-block and queued-prompt avatars are always "User" (the
   `user_display_name` / `profile_image_path` plumbing is gone); custom-router name placeholder is
   "My custom router"; header code-toolbelt tooltip and the zero-state block no longer consult
   `is_onboarded`; the notebook / workflow / env-var object limits (only feature-gated anonymous
   Warp users had limits) and their untrash checks; sqlite load passes no default owner (a `USER`
   permissions row needs its `subject_id`, which the 2024-10 migration guarantees); unused
   `_auth_state` params (diff application, PTY recorder, shell starter); `persistence::initialize`
   lost its context. MCP `is_author` is now "the template has no `creator_uid`" (what
   `creator_uid == user_id()` with no user always meant), no context. **Behavior changes (local
   features that the logged-out wall blocked):** (1) the first-frame callback now always runs: it
   records `FIRST_FRAME_DRAWN`, detects a low-power GPU (`GPUState`, which shows the
   integrated-GPU preference in Settings) and refreshes the graphics-backend dropdown, and on
   Linux/Windows feeds the crash-recovery watchdog `on_frame_drawn` — all were logged-in-only;
   (2) `TerminalAction::OnboardingFlow` no longer returns early for an anonymous user, so the
   debug-build "[Debug] Onboarding Callout" bindings open the local agent-onboarding callout (no
   production dispatcher of `StartAgentOnboardingTutorial` / `pending_onboarding_intention` exists).
   **Kept:** `DisableReason::NeedsWarpAccount` — not an auth reader but the client-side marker that
   hides Warp's server-side `auto` router placeholders and makes `fallback_llm_info` pick the user's
   endpoint; removing it means replacing the default model lists (Phase 3b). `LOCAL_USER_UID` /
   `personal_drive()` unchanged. Tests unchanged: 4,100 default / 4,101 simplewarp, 0 failed (the 39
   test files that installed `AuthStateProvider::new_for_test` / `AuthManager::new_for_test` now run
   with no user, as production does; none needed re-pinning); cloud_objects +
   cloud_object_persistence + cloud_object_models 10, 0 failed. **Follow-ups:** the local
   agent-onboarding tutorial / session-config onboarding intention has no production entry point
   (`StartAgentOnboardingTutorial` is never dispatched, `pending_onboarding_intention` is never set):
   product decision to wire it up or delete it; `get_shell_starter_internal`'s unused
   `_background_executor`; `avatar_color` in `render_user_avatar` is always `None`.

31. ~~R5 + R6: Warp server protocol residue~~ — **4ht done (2026-09-29).** −1.3k net lines in 32
   files. **Headers:** `http_client::Client` added Warp's client metadata to every request it
   built (`include_warp_http_headers` always returned `true`): `X-Warp-Client-ID` (`warp-app` /
   `warp-cli`), `X-Warp-Client-Version` (when an app version is set), `X-Warp-OS-Category`,
   `X-Warp-OS-Name`, `X-Warp-OS-Version`, `X-Warp-OS-Linux-Kernel-Version` (Linux), plus
   `WARP_EXTRA_HTTP_HEADERS` pairs on integration builds. That client carries the LSP server
   downloads (GitHub releases), the Node/npm install (`node_runtime`) and the OAuth2 adapter, so
   those third parties received it. `local_inference` (BYOK provider calls and model listing) and
   `mcp` build their own `reqwest::Client`, so provider and MCP traffic never carried the headers
   and needs nothing. All of it is gone: the `headers` module, the injection, the env var, the
   now-unused `ExecutionMode::client_id` / `current_client_id` global, and
   `warp_core::operating_system_info` (its only reader; `sysinfo` dropped from `warp_core`).
   No User-Agent was ever set by `http_client`, so none is added; requests go out with reqwest's
   defaults. **R6:** `server_api.rs`'s `X-Warp-Error-Code` / `OUT_OF_CREDITS` 429 mapping, the
   `From<http_client::ResponseError> for AIApiError` it lived in (no caller: the agent path
   converts `local_inference::Error`), `AIApiError::QuotaLimit` / `ServerOverloaded` and
   `RenderableAIError::ServerOverloaded` ("Warp is currently overloaded"), and with them
   `http_client::ResponseError` + the `Response::error_for_status*` methods (no other caller); `warp_errors`'
   `staging.warp.dev` 403 case. A provider 429 is unchanged: `ProviderStatus` becomes
   `AIApiError::ErrorStatus(429, body)`, recoverable (auto-resume), rendered with the provider's
   body; generic 429/5xx "not actionable" handling in `warp_errors` stays. **R5:** the
   `channel_versions` crate (release manifest, overrides, `apply_overrides` / `version_compare`
   bins) is deleted; `TargetOS` moved to `warp_core::platform` as a plain `Copy` enum
   (`MacOS` / `Linux` / `Windows`; the never-constructed `Web` / `Unknown` and the serde/clap
   derives went, it was never persisted); the `channel_versions_test.json` ignore lines too.
   **Kept:** `RenderableAIError::QuotaLimit` (provider-worded copy; still produced by the
   `stream_finished::Reason::QuotaLimit` arm, which `local_inference` never emits); the Linux
   secure-storage key seed string that mentions `channel_versions.json` (changing it would break
   stored keys). No behavior change for the user beyond the headers. Tests 4,100 default / 4,101
   simplewarp, http_client + warp_core + warp_terminal + warp_errors + ai + lsp + node_runtime 366,
   local_inference 97,
   0 failed. **Follow-ups:** `RenderableAIError::QuotaLimit`'s `user_display_message` is always
   `None` now (could map a provider 429 to it instead of the generic error, a product call).

32. ~~R7: Warp release/test infra in scripts; orphaned source files~~ — **4hu done (2026-09-29).**
   −3.5k lines in 48 files. **Packaging:** a locally built Linux package no longer points the
   machine at Warp's repositories: the `.deb` postinst/postrm repo templates
   (`debian/common/postinst.repo.template` wrote Warp's signing key and a
   `releases.warp.dev/linux/deb` apt source, `postrm.repo.template` purged them) and the `.rpm`
   `%post` Warp signing key + yum/zypper `releases.warp.dev/linux/rpm` repo setup (app and cli
   specs) are gone, as are `bundle_rpm`'s GitHub-only `rpm --import releases.warp.dev/…/warp.asc` +
   `rpmsign` step, `REPO_NAME` and `bundle_deb`'s stable repo-name check. The app `%post` keeps
   `update-desktop-database`; the cli spec has no `%post`. **Tests:** the SSH integration tests
   (`crates/integration/src/test/ssh.rs`, 6 tests) and the 2 remote-subshell tests tunnelled into
   Warp's GCP VM (`gcloud compute start-iap-tunnel … warp-ssh-integration-testing`), so they, the
   step helpers (`setup_gcloud_sdk`, `enter_ssh_command`, `enter_remote_subshell_command`,
   `wait_for_password_prompt`, `enter_ssh_password`), `integration_testing/subshell/util.rs`, their
   registrations, `ci.yml`'s `EXCLUDE_SSH_TESTS_FILTER` + ssh-agent step, and the gcloud CLI in
   `docker/linux-dev` (+ README mount) are gone; the local-subshell tests stay. The stale
   `.agents/specs/APP-4957` (deleted `docker/agent-dev` image) too. **Code:**
   `slash_commands/cloud_mode_v2_view.rs` (`CloudModeV2SlashCommandView`, 1.2k lines, never
   constructed). `get_shell_starter_internal` lost its unused executor (and a no-op `if let`);
   `render_user_avatar` / `query::Props` / `render_query` lost the always-`None` `avatar_color`.
   **Orphaned `.rs` files** (a script walked every crate's `mod` / `#[path]` / `cfg_attr(path)`
   graph from its lib/main/bin/test/bench roots): re-wired `slash_commands/mod_tests.rs` (+3 tests;
   the unused `BASELINE_AVAILABILITY` dropped; its `#[cfg(windows)]` WSL test is not compile-checked
   here) and `warp_completer/src/completer/tests.rs` (+2; `display` is `SmolStr` now). Deleted as
   testing deleted or rewritten code: `agent_sdk/driver/snapshot_tests.rs` (`build_repo_patch`),
   `agent_sdk/runner_tests.rs` and `warp_cli/src/runner_tests.rs` (cloud runners),
   `settings_view/platform_page_tests.rs` (API-key page), `inline_action/malformed_line_heuristics_tests.rs`
   (its module is gone), `writeable_pty/pty_controller_tests.rs` (pre-rewrite
   `PtyController` API; covered by the command-bytes/lifecycle tests). Deleted as dead or duplicate
   sources: `action_model/execute/get_files.rs` (`GetFilesRequestType` gone),
   `block/model/debug_model_impl.rs` (debug-link conversation model), `ai/voice/transcribe.rs`
   (warp-server Transcribe; its `api` module was gone), `usage/mod.rs` (its only module gone),
   `app/src/app_id_tests.rs` and `app/src/util/meta.rs` (byte-identical copies of `warp_core` /
   `warp_completer` files), and the empty `conversation_navigation/legacy.rs`,
   `metadata_project_rules_tests.rs`, `persisted_workspace_tests.rs`, `warp_core/src/errors.rs`.
   Remaining script hits are false positives (Linux `cfg_attr(path)` modules, `test_data` fixtures).
   **Kept:** `/opt/warpdotdev/<package>` and the `warp-terminal` / `oz` package names — the local
   `script/linux/bundle` deb/rpm/arch/AppImage builds install there (branding, not infra); Warp
   `Homepage` / `Maintainer` / `License` metadata in the templates and the Windows installer URL;
   `ci.yml` (user decision); `script/resolve_common_skills` (GitHub raw `warpdotdev/common-skills`,
   used by bootstrap/run). `bash -n` clean on every edited script and package template. Tests 4,103
   default / 4,104 simplewarp (+3), warp_completer 176 / 125 (v2) (+2 each), warp_cli + warp_core 104,
   0 failed. **Follow-ups:** `apply_edits`' unused `_ai_identifiers` / `_background_executor` /
   `_passive_diff`; rebrand the Linux package names/paths if the fork ever ships packages.

33. ~~Local Claude/Codex child-harness messaging~~ — **4hv done (2026-09-29).** −0.5k net lines in 17
   files. **History:** the child prompt told local Claude children to run
   `"$OZ_CLI" run message send|list|read|mark-delivered`; those subcommands (`warp_cli::task::MessageCommand`)
   went in 4cg (`0e2a88bd8`, conversation sync) because every one of them called Warp's public API
   (`post_public_api("agent/messages")`, `list/read/mark_message_delivered`, `…_for_task`), already
   `local_only_error()` stubs by then. `OZ_MESSAGE_LISTENER_*` (+ legacy `OZ_PARENT_*`) told the
   `oz-harness-support` Claude plugin (the `warpdotdev/claude-code-warp` "parent-message delivery
   bridge" + cloud skills) whether Warp or the plugin ran the listener that watched that same server
   mailbox; the lead-side receive path (message hydrator, orchestration event streamer) went in 4cg
   too. No local channel ever existed, so nothing to restore. **Removed:** the messaging block of the
   child prompt (now: work alone, end with a summary; pinned by a test), the child's `OZ_RUN_ID` /
   `OZ_PARENT_RUN_ID` / `OZ_CLI` / `OZ_HARNESS` / listener env (its env is now just the model var;
   pinned), the `oz-harness-support` install for local children (the notification plugin stays),
   `task_env_vars` + listener helpers/consts and their 5 tests, the `parent_run_id` threading
   (`StartAgentRequest`, `StartAgentExecutor::dispatch`, run_agents, terminal pane, launch), and the
   `OZ_RUN_ID_ENV` / `OZ_PARENT_RUN_ID_ENV` / `OZ_HARNESS_ENV` consts. The `agent run` driver still
   exports `OZ_CLI` (`oz_cli_env_var`) because it still requires the platform plugin.
   **Leftover:** `apply_edits` / `ApplyDiffModel::apply_diffs` lost the unused
   `ai_identifiers` / `background_executor` / `passive_diff`, `RequestFileEditsExecutor` its
   discard-only `generate_ai_identifiers`, `PreprocessActionInput` its never-read `conversation_id`.
   **Findings:** `LocalClaudeCodexChildHarnesses` is enabled by no bin (only its cargo feature, in
   neither `default` nor `simplewarp`; runtime-toggleable from the debug menu in dev builds) and
   only gates Codex; local Claude children are product-enabled, but the whole child launch is fed only
   by `RunAgents` tool calls, which the local adapter does not offer (`local_inference::tools::SUPPORTED`
   has 7 tools), so it is unreachable in simplewarp today. `SendMessageToAgentExecutor` (lead to
   child) always returns the local-only error. Tests 4,099 default / 4,100 simplewarp (−4), warp_cli
   55, 0 failed. **Follow-ups:** product decision on orchestration (offer `run_agents` in the local
   adapter, or delete the RunAgents / StartAgent / SendMessageToAgent vertical); the `agent run
   --harness claude` driver still requires the `oz-harness-support` plugin whose skills call the
   deleted `harness-support` CLI.

34. ~~`agent run` third-party harness's Warp platform plugin~~ — **4hw done (2026-09-29).**
   **Findings:** `warpdotdev/claude-code-warp` is a Claude Code plugin marketplace on GitHub with two
   plugins. `oz-harness-support` (the "platform plugin") is Oz-cloud-only: parent-message delivery
   hooks and the `oz-child-agent-orchestration` / `oz-finish-task` / `oz-notify-user` /
   `oz-report-pr` / `oz-upload-file` / `factory-files` skills, all calling `$OZ_CLI` subcommands or
   Warp's server. The driver installed it with `claude plugin marketplace add
   warpdotdev/claude-code-warp` + `claude plugin install` (network), and for Claude
   (`requires_verified_platform_plugin() == true`) failed setup with `HarnessSetupFailed` if install
   or the ≥1.1.2 version check failed, so `agent run --harness claude` hard-errored offline or
   without the plugin. Codex/Gemini had no plugin manager and never needed it. The run itself is
   local: the runner types `claude --session-id … < prompt` into the driver's terminal; completion
   comes from the command's exit code, plus `/exit` sent when the session status turns
   Success/Failed/Blocked. That status comes from the other plugin, `warp@claude-code-warp` (the
   notification plugin): its hooks write OSC 777 `warp://cli-agent` payloads to the local terminal and
   never talk to Warp. **Removed:** the platform-plugin install/update/verify path in the driver
   (`setup_platform_plugin`, `verify_required_platform_plugin`, `required_platform_plugin_error`),
   `ThirdPartyHarness::requires_verified_platform_plugin`, the trait's four platform-plugin methods,
   Claude's `PLATFORM_PLUGIN_KEY` / `MINIMUM_PLATFORM_PLUGIN_VERSION` and helpers, the two
   `SetupStep::…PlatformPlugin*` spans, the `OZ_CLI` export (`oz_cli_env_var`, `warp_cli::OZ_CLI_ENV`;
   only the platform plugin read it), and 7 tests. Plugin setup no longer returns an error. Also
   removed: `SendMessageToAgentExecutor` (the action is handled inline with the same error result, and
   `SendMessageToAgent` is no longer in `get_supported_tools`; the action/result shapes stay, so
   restored conversations still load). **Kept:** the notification-plugin install/update (the local
   completion signal; the install still clones the public GitHub marketplace once, which is the only
   non-provider network use left in `agent run --harness claude`) and Codex's
   `--dangerously-bypass-hook-trust` (it now only lets the user's own hooks run unattended).
   −0.5k lines in 15 source files. Tests 4,092 default / 4,093 simplewarp (−7), warp_cli 55, 0
   failed. Not run: a real `claude`/`codex` CLI.

35. ~~Onboarding tutorial~~ — **4hx done (2026-09-29, user decision: delete).** −3.0k lines of Rust
   (`crates/onboarding` 1.8k, −1.2k in 21 app/core sources), 56 images and 6 SVGs. Nothing dispatched
   `StartAgentOnboardingTutorial` or set `pending_onboarding_intention`; the only way in was the
   debug "[Debug] Onboarding Callout" bindings. **Removed:** `crates/onboarding` (callout view/model,
   components, examples) and its workspace/app deps; `workspace/view/onboarding.rs`
   (`OnboardingTutorial`, `start_agent_onboarding_tutorial`, `dispatch_tutorial_*`),
   `WorkspaceAction::StartAgentOnboardingTutorial`, `pending_onboarding_intention`; the session-config
   "Access your tab configs here." chip (it was only armed by a pending onboarding intention:
   `DismissSessionConfigTabConfigChip` + its escape/enter bindings, the
   `SESSION_CONFIG_TAB_CONFIG_CHIP_OPEN` context flag, the chip mouse state, its tutorial queue);
   `TerminalAction::OnboardingFlow`, `OnboardingVersion` / `AgentOnboardingVersion` /
   `OnboardingIntention`, the five debug bindings, the terminal view's callout field / rendering /
   focus / event handling and keybinding builder; the terminal `Event::OnboardingInitCompleted` /
   `OnboardingTutorialCompleted` / `PendingCommandCompleted` (only the tutorial subscribed; the
   pending-command queue and deferred agent-view entry stay), `pane_group::Event::OnboardingTutorialCompleted`,
   `clear_enter_agent_view_after_pending_commands`, `has_pending_command_or_awaiting_completion`;
   `AgentViewEntryOrigin::{OnboardingCallout, Onboarding}` and
   `InputTypeAutoDetectionSource::OnboardingAgentPrompt` (in-memory only); the `AgentOnboarding` flag +
   `agent_onboarding` cargo feature; `AppExecutionMode::can_show_onboarding`; the callout-bubble
   renderer and its six `CalloutTriangle*` icons/SVGs; `onboarding_theme_picker_themes` (no caller);
   56 unreferenced `async/png/onboarding` images from the old onboarding wizard and deleted launch
   modals (41 MB embedded; `custom_model_router_intro_banner.png` stays, the feature-intro modal uses
   it); the warp-oss bundle's `resources = ["assets/onboarding"]` (the directory does not exist).
   **Kept (live, same word):** the block-onboarding prompt block after "Import External Settings"
   (`ImportSettings`, `OnboardingPromptBlock`, `SettingsImportView`), the `BlockOnboarding`
   experiment layer, `AgentModeOnboardingBlockShown` / same-line-prompt onboarding-block settings,
   `RichContentMetadata::AIOnboardingBlock`, init-project's `ONBOARDING_TEXT`, Claude Code's
   `has_completed_onboarding` config write, `CloudObjectEventEntrypoint::Onboarding` (serde), the
   `AddGetStartedTab` debug binding, `set_enter_agent_view_after_pending_commands`. No setting key
   was removed. Tests unchanged: 4,092 default / 4,093 simplewarp, 0 failed (the tutorial had no
   tests). **Follow-up:** resolved by 4hz below.
36. ~~Help menu / JoinSlack / feedback / in-app docs.warp.dev links~~ — **4hy done (2026-09-29, two
   commits; user decision: drop, not repoint).** Part 1 (−566 lines in 23 files): the Help menu
   (Send Feedback / Warp Documentation / GitHub Issues / Join Slack) and `util/links.rs`; workspace
   actions `JoinSlack`/`ViewUserDocs`/`SendFeedback` + palette bindings; the `/feedback` slash
   command; the AI settings page's typeform "Let us know", ToS hyperlink and custom-inference
   "Learn more" with their highlight plumbing; agent tips' link field + "Learn more" renderer (34
   tips); docs links in the slow-bootstrap / ControlMaster / incompatible-shell / pure-prompt
   banners; the notifications discovery/error banners' Learn more/Troubleshoot buttons + action
   variants; the permission toast's troubleshoot link; the shell-terminated banner's File issue /
   More info buttons + `OpenUrl` action; the AI block debug footer's Send Feedback +
   `OpenFeedbackDocs` actions; the wayland crash-recovery banner's docs link; orphaned
   `NewSessionMenuItem::OpenLaunchConfigDocs`. Part 2 (−489 lines in 21 files): every remaining
   user-facing docs.warp.dev link — settings pages: agent-profiles (codebase-context + MCP learn-more
   links and their `HighlightedHyperlink` plumbing), appearance (the `CreateCustomThemeWidget`
   docs-link widget deleted; blur + alt-screen-padding info buttons dropped),
   knowledge (Rules learn-more + its `HyperlinkClick` action), features (8 sites — notifications,
   session restoration ×2, sticky command header, Wayland global-hotkey, mouse reporting, text
   selection, YAML workflows — plus `FeaturesPageAction::OpenUrl` and per-widget mouse states),
   warpify title, external-editor markdown-viewer info button + its `OpenUrl` action, MCP list page
   (2 links); terminal side: the open-in-Warp banner's Learn more button + action + URL consts, the
   block-onboarding prompt's learn-more link + `HyperlinkClick`, the warpify success block's
   learn-more link + `OpenUrl` (cascade: `WarpificationSource` enum deleted, the
   `SessionBootstrappedEvent` destructure drops `session_type`), the launch-config save modal's
   "Link to Documentation", the default-shell-fallback banner's "Learn more", the workflows
   empty-state "creating your own workflow" link (sentence kept as plain text); plain-text URLs:
   agent-SDK slow-bootstrap CLI warning, warpui wgpu Nvidia-adapter log, the launch-config YAML
   template comment. Also fixed part-1 residue the clippy gate caught (`render_agent_tip` in
   status_bar.rs: `single_match` + a `let _tip_description` dummy; part 1 had only run cargo
   check). **Kept, deliberately:** `AIAgentCitation::WarpDocumentation` (multi-agent wire shape;
   opens the public docs site like a WebPage citation — the only docs.warp.dev string left in
   non-test code), `secure_storage/linux.rs`'s `releases.warp.dev` literal (a KDF salt, not a
   link), the `warpui_core` platform doc-comment example, and all test fixtures (`warp.dev/about`,
   `app.warp.dev` as sample URLs). Tests unchanged: 4,092 default / 4,093 simplewarp, 0 failed;
   clippy trio, format, both cargo checks clean.

37. ~~`session_config_rendering` accent-background residue (4hx follow-up)~~ — **4hz done
   (2026-09-29).** −407 lines. The four `*_with_background` variants took `bg: Option<ColorU>`;
   the `Some` path rendered the Phenomenon-palette accent-tinted onboarding callout, and every
   caller passed `None`. **Folded:** `render_session_type_pills_with_background` /
   `render_directory_picker_with_background` / `render_worktree_checkbox_with_background` /
   `render_autogenerate_worktree_branch_name_checkbox_with_background` into their plain wrappers
   (`bg` param, `on_accent_bg` branches and `session_type_item_color`'s `on_accent_bg` arm
   dropped). **Deleted:** `app/src/view_components/callout_bubble.rs` (89 lines — eight
   Phenomenon-palette helpers plus `callout_label_color` / `callout_checkbox`, whose only consumer
   was the dead accent branches) and its `mod` decl, and `warp_core`'s `ui/theme/phenomenon.rs`
   (157 lines — the `PhenomenonStyle` palette, used by nothing else; `pub` in a lib crate so
   dead-code lint never flagged it). The kept block-onboarding prompt block lives elsewhere and
   does not use either file. Tests unchanged: 4,092 default / 4,093 simplewarp, 0 failed; clippy
   trio, format, both cargo checks clean.

38. ~~Multi-agent orchestration vertical (user decision: delete)~~ — **4ia done (2026-09-29,
   commit 1a248d999).** The local adapter never offers `run_agents`, so the whole child-agent
   vertical was unreachable. −18.9k lines in 123 files: `RunAgentsExecutor` /
   `StartAgentExecutor` + tests, local Oz/harness child launch (`child_agent_launch.rs`,
   `pane_group/child_agent/` hidden panes + restoration, `local_harness_launch.rs`,
   `local_harness_setup.rs`), all orchestration UI (pill bar + model, run_agents confirmation
   card, orchestration_controls, topology, conversation links, avatar module, block renderer,
   `document/orchestration_config_block.rs`, usage per-child rollup), the `/orchestrate` slash
   command (+ `SlashCommandKind::Orchestrate`; `UserQueryMode::Orchestrate` stays for wire
   compat), the `CommonCommandGates` machinery that only gated /orchestrate,
   `orchestration_enabled` request plumbing (`supports_orchestration_v2` now literal false;
   RunAgents/WaitForEvents no longer advertised), RunAgents permission settings UI + dead model
   methods, and the `LocalClaudeCodexChildHarnesses` / `MultiLevelOrchestration` flags
   (RUNTIME_FEATURE_FLAGS now empty). 181 tests deleted. **Kept:** crates/ai action/result
   serde shapes + convert arms (old transcripts load), `RunAgentsPermission` enum + TOML field,
   `agent run` CLI driver + shared harness/plugin-manager code (the notification plugin still
   installs for `agent run --harness claude`), `AmbientAgentTaskId`, `Harness::Oz`,
   `is_child_agent_conversation` + conversation_loader child semantics for old DBs,
   context-window usage view. **Residue (later rounds):** crates/ai orchestration variants are
   wire-only; `OrchestrationMessageDisplayMode` setting + block.rs collapsible helpers are
   restore-path only. Tests 3,911 default / 3,912 simplewarp, 0 failed; warp_cli 55; clippy
   trio, format, both checks clean. The dispatch subagent died mid-round on a provider quota
   error; the resume (parent session) verified the uncommitted partial tree as untrusted
   input, completed the remaining scope, and repaired two botched edits (claude_tests.rs
   dangling fragment; agent_icon_tests match arm referencing a deleted variant).

39. ~~Provider 429 → QuotaLimit (user decision: map)~~ — **4ib done (2026-09-29, commit
   280d1f95e).** `From<&Arc<AIApiError>>` grew an `ErrorStatus(429, body)` arm producing
   `RenderableAIError::QuotaLimit` with a best-effort display message extracted from the
   provider body (`error.message` → `message` → `detail`; generic "Quota limit reached." copy
   when nothing matches). Non-429 statuses unchanged; `agent run`'s error classification
   improves with it. +2 tests: 3,913 default / 3,914 simplewarp, 0 failed.

40. ~~Oz → SimpleWarp rebrand (user decision)~~ — **4ic done (2026-09-29, commits a191035c3 +
   48a51d04f).** The symlink already installs as the channel's `cli_command_name`
   (simplewarp), so this was copy: clap help `name`/`display_name`, Install/Uninstall CLI
   palette entries + toasts (fns renamed `install_cli`/`uninstall_cli`,
   `WorkspaceAction::{Install,Uninstall}Cli`; binding names were already neutral
   `workspace:{,un}install_cli`), codex modal, init slash-command description, appearance
   setting copy, execution-profile allow/deny descriptions, "Command from agent" workflow
   title, the CLI log subdirectory `oz/` → `simplewarp/` (old logs stay put; the native_tests
   legacy-oz decoy stays), deleted the legacy invoked-as-"oz" CLI-mode check in run(), and
   reworded comments naming the Oz CLI / the deleted Oz secrets store. **Kept:** the
   `Harness::Oz` wire name + `convert_run_agents_harness` "oz" string, legacy settings-page
   slug mappings ("Oz", "OzCloudAPIKeys"), the worktree-name word list, `AgentHarness`-era
   internal logs. Tests unchanged.

41. ~~Enable-candidate flags (user decision: enable all)~~ — **4id done (2026-09-29, commit
   7b27ff169).** Added `editable_markdown_mermaid`, `ime_marked_text`, `iterm_images`,
   `local_computer_use` and `local_docker_sandbox` to the `simplewarp` feature set;
   LocalDockerSandbox got a new cargo feature + features.rs mapping (it was
   dogfood-list-only). Pinned by a `simplewarp_enables_the_local_feature_set` test next to
   the jupyter one. Tests 3,913 default / 3,915 simplewarp, 0 failed; clippy trio, format,
   both checks clean. Status table's enable-candidate list is now EMPTY; Phase 3b remains
   the only open phase.

42. ~~3b: built-in model list~~ — **done (2026-09-29, commit 06743c4f8).** The compiled-in
   default catalog held only Warp's server-side routers (`auto`, `auto (responsive)`,
   `cli-agent-auto`, `computer-use-agent-auto`), disabled client-side with
   `DisableReason::NeedsWarpAccount` because no local build can reach them. All four feature
   lists now share one first-party trio — `claude-sonnet-4-5` (Anthropic; the default),
   `gpt-5.4` (OpenAI; the repo's own Codex migration target), `gemini-2.5-pro` (Google) —
   `disable_reason: None`, vision on, `provider` tagged so BYOK detection works;
   `local_inference` routes by slug shape onto the user's key for that provider. With no key
   the entries show in the picker and a request fails with the actionable `NoApiKey` message
   ("Add one in Settings > AI") instead of the old dead `auto` chip + `NoModelConfigured`.
   To keep the fallback honest, `is_usable_llm` is now key-aware for first-party models
   (same precedent as BYOK-aware `RequiresUpgrade`): a built-in without its provider's key is
   skipped by `fallback_llm_info`/`usable_default_llm_info` and cleared by
   `reconcile_disabled_model_preferences` on key events, so one provider's key resolves the
   default to that provider's model instead of a model the key can't reach. **Deleted:**
   `DisableReason::NeedsWarpAccount` (variant, tooltip arm, `should_clear_preference` arm,
   the three picker filter arms — no producer left) and `default_computer_use_llms` (the
   `get_computer_use_available` OnceLock fallback now uses the shared catalog). **Kept:**
   `is_warp_router` in `local_inference::config` (a persisted `base_model: "auto"` selection
   still resolves and gets the clear `NoModelConfigured` error), `is_auto_target` (router
   YAML validation), and the rest of `DisableReason` (no local producer, but the enum is
   wire-shaped and its tests exercise the machinery). **Known wart:** with a provider key
   set, the live `/models` catalog can show alongside a same-slug built-in entry (ids differ
   when the provider returns dated snapshots). Tests unchanged: 3,913 default / 3,915
   simplewarp, 0 failed; clippy trio (the workspace run matches HEAD's 6-error red
   baseline), format, both checks clean.

Known non-targets (do not queue without a new user decision): persisted shapes (MoveToDrive,
PersonalCloud, `autosync_plans_to_warp_drive`,
`AIAgentCitation::WarpDriveObject`, `OpenWorkflowModalWithCloudWorkflow` action name,
`Icon::Warp`, the app_state WarpDrive tombstone), the
enable-candidate flags above, and the `AgentHarness` flag (live by design).

## Risks

- **The agent loop lives on the client.** The system prompt, tool schemas, and loop control the
  server used to own are now in `local_inference`; quality can differ from Warp.
- **Model configuration.** The model list used to come from the server. The local build now
  ships a small compiled-in first-party trio (round 42) plus whatever the user's keys reach
  live; compiled-in slugs can age out — the failure is a 404 naming the model, and the live
  list remains correct.
- **Persisted shapes.** Deleting a serde variant or setting key can break loading old configs.

## Verification per round

- `./script/format` idempotent.
- The three clippy runs in `script/presubmit` (workspace minus completer, `-p warp`,
  `-p warp_completer`) with `-D warnings`. The `--all-features` run in AGENTS.md is known broken
  and not used.
- `cargo check` of the default and `simplewarp` builds with zero warnings.
- `cargo nextest run -p warp --lib` in both configs; compare against the previous round's counts.

## Git: this repository is a fork

`wynn5a/simplewarp` is a fork of `warpdotdev/warp`. A bare `gh pr create` targets the **parent**.

```sh
gh repo set-default wynn5a/simplewarp
gh pr create --repo wynn5a/simplewarp --base master --head <branch> ...
```

Always pass `--repo`. Push to `origin` only; never add a remote pointing at `warpdotdev/warp`.
Push as `wynn5a`, then switch `gh` back to `fuwenming-pacvue`.

## Build commands

```sh
cargo run --no-default-features --features simplewarp --bin simplewarp   # the app
cargo test -p local_inference                                            # the AI adapter
cargo check -p warp --bin warp-oss                                       # no regression
script/bundle_simplewarp                                                 # release .app
```

Toolchain: `protoc` (`brew install protobuf`), full Xcode (Command Line Tools lack `metal`), and
the Metal Toolchain (`xcodebuild -downloadComponent MetalToolchain`). `xcode-select -p` already
points at `/Applications/Xcode.app`.

### Disk

- `[profile.dev] incremental = false`: the incremental cache for the ~700k-line `warp` crate was
  most of a 23 GB `target/`. Use `CARGO_PROFILE_DEV_INCREMENTAL=true` for a run of repeated edits.
- macOS purges `target/` by itself (`CACHEDIR.TAG`). `couldn't create a temp dir … rmetaXXXX` is
  that, not corruption — re-run.
- Each feature set keeps its own artifacts: run default-feature checks together and the
  `simplewarp` check last.

## How to test the AI

### Without the app, against a real provider

```sh
export LOCAL_INFERENCE_BASE_URL=https://example.com/v1
export LOCAL_INFERENCE_API_KEY=sk-...
export LOCAL_INFERENCE_MODEL=some-model
export LOCAL_INFERENCE_SCHEMA=anthropic        # optional; OpenAI Chat Completions is the default
cargo test -p local_inference --test live_provider -- --ignored --nocapture
```

The app keeps its keys in the login keychain (service `dev.simplewarp.SimpleWarp`, account
`AiApiKeys`, one JSON blob). To reuse the app's endpoint:

```sh
eval "$(security find-generic-password -s dev.simplewarp.SimpleWarp -a AiApiKeys -w \
  | python3 -c '
import sys, json, shlex
e = json.loads(sys.stdin.read())["custom_endpoints"][0]
print("export LOCAL_INFERENCE_BASE_URL=" + shlex.quote(e["url"]))
print("export LOCAL_INFERENCE_API_KEY=" + shlex.quote(e["api_key"]))
print("export LOCAL_INFERENCE_MODEL=" + shlex.quote(e["models"][0]["alias"]))
')"
```

### In the app

1. Start the app, open Settings > AI.
2. Paste a provider key, or add a custom endpoint (base URL plus model slug) — e.g. Ollama at
   `http://localhost:11434/v1`.
3. Ask the agent something. Watch `~/Library/Logs/simplewarp.log`, and check with
   `lsof -nP -iTCP -a -p $(pgrep -f simplewarp)` that the only connection is to the provider.
