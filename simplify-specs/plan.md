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
