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

Workspace is down to 69 crates. Gone, in rough order:

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

1. **4gp — the wasm/web target** (user decision 2026-09-28: only the desktop app is kept).
   ~1,300 `target_family = "wasm"` sites in ~270 files, plus the `serve-wasm` and
   `warp_web_event_bus` crates. Takes the rest of the URL readers with it: `font_fallback.rs`,
   `wasm_nux_dialog.rs`, `wasm_view.rs`, `web_intent_parser.rs`, the workspace
   local-network-access toast, `WorkspaceAction::OpenLinkOnDesktop`, and the pane
   `shareable_link` browser-URL machinery.
2. **4gq — Sentry crash reporting**: the `crash_reporting` / `cocoa_sentry` / `heap_usage_tracking`
   cargo features (in neither set), ~1,200 lines in `app/src/crash_reporting/`, 46 cfg sites in
   17 files, the minidump server, the privacy-page crash-reports toggle, and `CrashReporting` /
   `CocoaSentry` flags.
3. **Decision needed — SSH remote server** (`SshRemoteServer`, a release flag, ~16k lines in
   `app/src/remote_server/` + `crates/remote_server/`). Its install downloads a Warp CLI binary
   from `{server_root_url}/download/cli` (`crates/remote_server/src/setup.rs`), which is
   `.invalid` here, so installing on a new host cannot work. Also carries the daemon bearer-token
   plumbing (`Credentials::Bearer`, `apply_remote_server_auth_context`).
4. After 1–3: the channel config fields themselves (`server_root_url`, `rtc_server_url`,
   `oz_root_url`, `session_sharing_server_url`) and `crates/websocket` if nothing else uses it.

Known non-targets (do not queue without a new user decision): persisted shapes (MoveToDrive,
PersonalCloud, WarpDrivePrivacySettings, `autosync_plans_to_warp_drive`,
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
