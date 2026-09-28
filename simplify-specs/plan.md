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
| 3b — Built-in model list, MCP tool support | OPEN |
| 4 — Delete the dead cloud code and the TUI | Nearly done — see below |

Enabled in the simplewarp build: `jupyter_notebook_rendering` (2026-09-15, pinned by a
`features::tests` test). Remaining enable-candidates (product decisions, not deletion work):
EditableMarkdownMermaid, ImeMarkedText, ITermImages.

## Phase 4 progress

Workspace is down to 68 crates. Gone, in rough order:

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

Known non-targets (do not queue without a new user decision): persisted shapes (MoveToDrive,
PersonalCloud, `autosync_plans_to_warp_drive`,
`AIAgentCitation::WarpDriveObject`, `OpenWorkflowModalWithCloudWorkflow` action name,
`Icon::Warp`, the app_state WarpDrive tombstone), `crates/onboarding`'s live local tutorial, the
enable-candidate flags above, and the `AgentHarness` flag (live by design).

## Risks

- **The agent loop lives on the client.** The system prompt, tool schemas, and loop control the
  server used to own are now in `local_inference`; quality can differ from Warp.
- **Model configuration.** The model list used to come from the server; the local build needs its
  own (Phase 3b).
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
