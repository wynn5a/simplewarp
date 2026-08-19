# SimpleWarp — Plan

## Goal

Make **SimpleWarp**: a fast terminal with bring-your-own-key (BYOK) AI.
No cloud, no login, no subscription, no Warp Drive.

Target experience:

- The app works offline. The only network traffic goes to the AI provider that the user
  configures.
- No login screen and no anonymous Firebase user.
- No Warp Drive, shared sessions, cloud mode, ambient agents, remote server, or billing UI.
- AI keys stay on the machine. The app calls the provider direct.
- Keep: terminal emulation, tabs, panes, settings, themes, shell management, completions,
  command palette, and the GUI front-end.

## Decisions

| Topic | Decision |
| --- | --- |
| AI | Local adapter. The app calls the provider direct. |
| Code removal | Gate, then hide, then delete. |
| TUI (`crates/warp_tui`) | Delete. It is not part of the GUI app. |
| Name | SimpleWarp. Bin `simplewarp`, id `dev.simplewarp.SimpleWarp`, scheme `simplewarp`. |

## Reconnaissance

| Metric | Value |
| --- | --- |
| Rust LOC | ~1.68M |
| Workspace crates | 78 |
| `app/src` modules | 127 |
| Files that use server/firebase/graphql crates | 147 |
| Cargo features in the `default` set | 199 |
| Toolchain | Rust 1.92.0 |

### Critical finding — BYOK is not local today

`crates/warp_multi_agent_client/src/lib.rs:127` builds the AI endpoint from
`ChannelState::server_root_url()`. Every agent request goes to `{warp_server}/ai/multi-agent`.
`app/src/ai/agent/api.rs:405` puts the user API keys **inside that request**
(`warp_multi_agent_api::request::settings::ApiKeys`). Warp's server does the model call.

Result: if we remove the cloud, the AI stops to work. A local adapter is necessary.

`app/src/ai/agent_sdk/` (Claude Code, Codex, and Gemini harness) does not help. It uses Warp
cloud runners (`app/src/ai/agent_sdk/runner.rs`, `api_key.rs` use GraphQL and `ServerApiProvider`).

### Good news — the protocol is small

The server tells the client what to do with a small event stream:

```
ResponseEvent = Init | ClientActions | Finished
ClientAction  = CreateTask | AddMessagesToTask | AppendToMessageContent
              | UpdateTaskMessage | BeginTransaction | CommitTransaction | ...
Message       = UserQuery | AgentOutput | ToolCall | ToolCallResult | AgentReasoning | ...
```

**The client already runs all the tools locally** (`RunShellCommand`, `ReadFiles`,
`ApplyFileDiffs`, `Grep`, `CallMCPTool`, and more). The server only decides which tool to call.
So a local adapter must do 3 things: build a provider request from the conversation, stream the
reply, and emit the same events.

Proto source: `github.com/warpdotdev/warp-proto-apis` rev `b0886a9`.

### Scaffolding that we can use

| Requirement | Existing mechanism |
| --- | --- |
| No telemetry, crash reporting, or autoupdate | `app/src/bin/oss.rs` sets these configs to `None` |
| No login | `skip_login` cargo feature, `SkipFirebaseAnonymousUser` flag |
| Custom AI UI | `solo_user_byok`, `api_key_management`, `custom_model_routers` flags |
| Feature gating | `FeatureFlag` enum in `crates/warp_features`, mapped in `app/src/features.rs` |

Warning: `skip_login` makes authenticated requests `bail!`
(`crates/warp_server_client/src/auth/session.rs:98`). It hides nothing by itself.

### Key files

- `crates/warp_features/src/lib.rs` — the `FeatureFlag` enum.
- `app/src/features.rs` — maps cargo features to flags.
- `app/Cargo.toml` — `[features]`; the `default` set turns on the cloud surface.
- `app/src/bin/oss.rs` — the model for the new binary.
- `app/src/root_view.rs:1925-1964` — the startup gate. `ForceLogin` and the pre-login
  onboarding path run **before** the `SkipFirebaseAnonymousUser` check.
- `app/src/auth/`, `app/src/billing/`, `app/src/drive/`, `app/src/cloud_object/`,
  `app/src/remote_server/`, `app/src/workspaces/` — the cloud surface.

## Phases

### Phase 0 — Baseline — DONE

`cargo check -p warp --bin warp-oss` is green. Three tools were necessary that the bootstrap
script does not install:

| Tool | How |
| --- | --- |
| `protoc` | `brew install protobuf` |
| Xcode | The App Store. Command Line Tools alone have no `metal`, which `crates/warpui/build.rs:113` runs. |
| Metal Toolchain | `xcodebuild -downloadComponent MetalToolchain` (839 MB). Xcode 26 ships it separately. |

`xcode-select` still points at Command Line Tools, so every build command carries
`DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer`. `sudo xcode-select -s
/Applications/Xcode-beta.app` would make that permanent.

### Phase 1 — The `simplewarp` binary — DONE

1. A `simplewarp` feature set in `app/Cargo.toml`: the `default` 199 features, less 57 cloud,
   sharing, ambient, hand-off, and billing features, plus `local_only` and `local_inference`.
2. `app/src/bin/simplewarp.rs`, modeled on `oss.rs`. Telemetry, crash reporting, and autoupdate
   are `None`. App id `dev.simplewarp.SimpleWarp`, URL scheme `simplewarp`.
3. `Channel::Oss` is reused. Branding comes from `AppId` and the Info.plist, so a seventh
   `Channel` variant would only add 42 match arms for nothing. Rename it in Phase 4.
4. `skip_firebase_anonymous_user` is on. `account_first_onboarding`, `agent_onboarding`, and
   `open_warp_new_settings_modes` are off, so `root_view.rs:1934-1958` goes to the terminal.

**`local_only` instead of `skip_login`.** `skip_login` installs a stand-in test user
(`crates/warp_server_auth/src/auth_state.rs`), so `is_logged_in()` returns true and every cloud
call site starts work that can only fail. The first run logged 6 startup errors for that reason.
The new `local_only` feature makes the build genuinely logged out: no test user, no persisted
user from secure storage (so a machine that has run Warp before is not silently reconnected),
and no token. Every `is_logged_in()` guard then works, and the startup errors went from 6 to 0.

**The BYOK gate had to move.** `is_byo_api_key_enabled` and `is_custom_inference_enabled`
(`app/src/workspaces/user_workspaces.rs`) both return false for a logged-out user. In this build
a user key is the only path to a model, so under `local_inference` both return true.

Acceptance — all verified on a real run:

- The app starts direct into a terminal (`Starting shell /bin/zsh`).
- No login or onboarding modal.
- `lsof -iTCP` on the running process shows no outbound connection at all.
- 0 errors in the startup log.
- `cargo check -p warp --bin warp-oss` is still green, so the normal build is not affected.

### Phase 2 — Hide the cloud UI — STARTED

Done so far, all gated on `features::warp_account_available()`, which is false when the
`local_only` feature is on. That predicate is deliberately not "the user is logged out": in a
normal build logged out means "you could sign in", here it means "there is no such thing".

| Removed | Where |
| --- | --- |
| The title-bar **Sign up** button | `app/src/workspace/view.rs` |
| The **Sign up** item in the user menu | `app/src/workspace/view.rs` |
| The **Login for AI** inline banner | `app/src/terminal/view.rs` |
| The sign-up prompt that replaced the AI toggle | `app/src/settings_view/warp_agent_page.rs` |
| The settings pages with nothing local on them | `app/src/settings_view/mod.rs` |

`SettingsSection::needs_warp_account()` names the dropped pages: Account, Billing and usage,
Referrals, Shared blocks, Teams, Warp Drive, and the Cloud platform umbrella. An umbrella whose
subpages have all gone is dropped with them.

**Dropping a page from the sidebar is not enough.** `Account` is the enum `#[default]`, and a
settings pane restored from SQLite carries whatever page was open last, so settings still opened
on a page that was no longer in the sidebar. `SettingsSection::available()` maps such a page onto
`WarpAgent`, and it is applied at `set_and_refresh_current_page_internal` — the one function
every page change funnels through, including session restore. Two unit tests in
`settings_view/mod_tests.rs` pin both directions of that mapping.

**The command palette needed no filtered `Vec`.** `EditableBinding::with_enabled` already exists
for exactly this, and its own docs say a disabled binding "is hidden completely". The six
cloud pages in `add_open_setting_pages_as_editable_binding` — Account, Shared Blocks, Teams,
Billing and usage, Referrals, and Environments — now carry
`.with_enabled(|| features::warp_account_available())`, which matches how the file already gates
on `FeatureFlag::AgentMode`. The predicate is re-read at runtime, so nothing is cached wrongly.

**Two startup errors were reported for work that could never succeed.** Both features run on
Warp's server, not on the user's provider, so in this build they failed on every launch and
called `report_error!`:

| Was | Now |
| --- | --- |
| `Failed to fetch prompt suggestions` | `generate_prompt_suggestions` returns before the request. The static suggestions above it need only the finished block, so they still work. |
| `Failed to generate Next Command suggestion` | `generate_ai_input_suggestions_if_available` skips the call and answers with an empty suggestion. The history-based suggestion earlier in the same function reads the user's own shell history, so it still works. |

An empty next-command action is now dropped quietly in
`on_next_command_suggestion_result`. Without that it fell through to a warning about a prefix
that an empty string could never match — trading an error for a misleading warning.

**Two startup warnings are gone.**

- `cloud_preferences_syncer` had the switch already: `sync_enabled`, which the TUI uses to keep
  its config local. It is now also false with no Warp account. The check is at the call site, not
  in `SettingsMode::should_sync_to_cloud`, because that asks which *surface* is running and the
  `settings` crate knows nothing about accounts.
- The share-block modal is built once per pane group with no block in it, and draws nothing until
  one is chosen. With the entry points gated, nothing can ever choose one, so the empty draw is
  expected and no longer warns.

**Warp Drive was already hidden, by Phase 1.** `WarpDriveSettings::is_warp_drive_available` reads
`!SkipFirebaseAnonymousUser.is_enabled() || !is_anonymous_or_logged_out()`. This build turns that
flag on and is genuinely logged out, so both sides are false. All eight UI sites — the panel, the
"Save as workflow" and "Import to Drive" menu items, the block list — already ask
`is_warp_drive_enabled`. Nothing to do; this list entry was stale.

**The shared-session and cloud features are compiled out.** The `simplewarp` feature set holds 144
features and none of them is a `shared_session`, `drive`, `cloud`, `billing`, `referral`, `team`,
`ambient`, `remote_server`, or `credits` feature, so those `FeatureFlag`s are off at compile time
and the UI behind them never renders. What remained was the UI that those flags do **not** guard:

- The "Share..." block context-menu item, which showed whatever the flags said. It is now pushed
  only when an account is available, rather than listed and disabled — a disabled item still reads
  as a feature that is merely unavailable today.
- The "Warp credit fallback" toggle and its palette command. Both were gated on
  `is_byo_api_key_enabled || is_custom_inference_enabled`, and this build forces **both** true, so
  neither could stand in for an account check.

**The app's test suite did not compile, and had not since Phase 3.** `LLMPreferences` gained
`provider_llms` then, and the four literals in `app/src/ai/llms_tests.rs` were never updated. The
Xcode blocker hid it, because nobody could build the tests. Fixed; `cargo test -p warp --lib` runs
again: **6440 pass, 13 fail**.

Those 13 are not from this work. Reverting only the Phase 2 code and running the same suite gives
**14** failures — the same 13 plus one more — so the changes here cause none of them, and the
varying count shows some are flaky. They are in secret redaction, experiments, notebooks,
telemetry, terminal bootstrap and view, `util::path`, and a leak check, and they need their own
pass. Under default features every gate added here reads `warp_account_available() && …`, which is
`true && …`, so the normal build cannot change behaviour.

**Hiding a binding broke the native menu, and reading the log too early hid it.** The six gated
settings-page bindings were the whole story only in the palette. Three of them also carry a
`CustomAction`, and a native menu item takes its title from the binding with the same action, so
`default_name` could not find one and its `debug_assert!` killed the app about twenty seconds into
every launch:

```
panicked at 'action should have a name: ViewSharedBlocks'
panicked at 'action should have a name: OpenTeamSettings'
```

The first check read `simplewarp.log` sixteen seconds after launch, saw no errors, and called the
startup healthy. **Wait at least a minute and confirm the process is still alive before believing a
clean log.** The warning counts above were real, but the process they came from then died.

`ViewSharedBlocks` and `OpenTeamSettings` are the only two of the three that appear in
`app_menus.rs`; `ShowAccount` does not, so grepping the exact action names gives a finite set
rather than a guess. Both items are now gated, along with `CreateBlockPermalink`, which shares the
block-sharing surface. Gating the items beats softening `default_name`, whose fallback title is
`"<NO DESCRIPTION>"` — the assert only fires in a debug build, so a release build would have shown
that as a menu item instead of crashing.

**Two more surfaces turned up only by running the app**, both from the user testing the history
panel:

- The history panel showed `Sign in to access Agent conversations`. That account check is about
  Warp's own cloud history, which a logged-out user has none of, but this build keeps every
  conversation in the local database — so the wall stood in front of data already on the machine.
  Dropped, mirroring `AISettings::is_any_ai_enabled`.
- The Warp Drive toolbelt icon was still there. The claim above that Drive was already hidden holds
  for its eight menu and panel sites, but **not** for the toolbelt, which built the icon from the
  raw `enable_warp_drive` preference — default true — instead of `is_warp_drive_enabled`. Drive is
  cloud-only with no local store, so the icon is dropped when no account is possible.

Still to hide: cloud mode, ambient agents, and the remote server UI, none of which was reachable
in a startup log or a settings page, so each needs a look in the running app.

Acceptance:

- [x] No cloud settings page, and no palette command for one.
- [x] No login or billing UI.
- [x] 0 errors and no cloud warnings at startup: 17 warnings fell to 6.
- [x] The app survives launch. Confirmed past 105 seconds with 0 panics and 0 errors.
- [x] No dead buttons in the settings pages, the palette, the block context menu, the agent page,
      the native menus, the history panel, or the toolbelt. **User-tested 2026-08-19.**
- [ ] The rest of the UI is unchecked, and running the app is the only way to check it. Two of the
      defects above were invisible to `cargo check` and to the startup log.

Of the 6 startup warnings, one is SQLite recovering its WAL after the app was killed rather than
quit. The other five were one conversation each, saying `missing an initial query`; the cause was
in the local adapter and is fixed in Phase 3. Conversations made since carry their question.

### Phase 3 — Local AI adapter — DONE and VERIFIED IN THE APP

New crate `crates/local_inference` (83 unit tests plus 5 live tests pass, clippy clean):

| Module | What it does |
| --- | --- |
| `config.rs` | Picks the endpoint from `Settings`. A custom endpoint wins; otherwise the model slug decides (`claude*` → Anthropic, `*/*` → OpenRouter, `gemini*` → Google, else OpenAI). |
| `convert.rs` | Flattens the proto conversation into a neutral `Turn` list, and renders tool results to text. Caps a result at 32 kB and keeps the tail. |
| `tools.rs` | JSON schemas for `run_shell_command`, `read_files`, `apply_file_diffs`, `grep`, `file_glob`, and the two-way map to the proto `ToolCall`. Honours `Settings::supported_tools`. |
| `provider/anthropic.rs` | Anthropic Messages: body and SSE. |
| `provider/openai.rs` | OpenAI Chat Completions: body and SSE. Also covers OpenRouter, Google, Ollama, LM Studio, and vLLM. |
| `prompt.rs` | The system prompt that Warp's server used to own. |
| `emit.rs` | Deltas → `ResponseEvent`s. Streams text with `AppendToMessageContent`; holds a tool call back until its JSON arguments are whole, and drops a call that is broken or invented. |
| `stream.rs` | `generate_local_output`, the drop-in for `generate_multi_agent_output`. |

Wired at `app/src/ai/agent/api/impl.rs:141` behind the `local_inference` cargo feature, which
`simplewarp` turns on. **Not compiled yet — see the Xcode blocker.**

**The model list.** `app/src/ai/llms.rs` fetches the catalog from the server. With no server it
falls back to `ModelsByFeature::default()`, whose only entry is `auto` — Warp's own server-side
router, which names no real model. It was also the default, so a fresh user's first request
failed with "No API key is set for model `auto`".

Fixed by reusing machinery that was already there rather than adding a parallel path:

- A new `DisableReason::NeedsWarpAccount` marks the built-in `auto` entries in a `local_only`
  build. The picker already drops disabled models, and `fallback_llm_info` already falls back to
  the user's first custom endpoint when the default is unusable — so a configured endpoint
  becomes the effective default on its own.
- `local_inference` returns `NoModelConfigured` for a router id instead of a missing-key error
  that names a model the user never picked. A custom endpoint registered under the key `auto`
  still wins; there is a test for that.

**Where the model list comes from now.** A key for a provider means the user wants that
provider's official API, so `crates/local_inference/src/models.rs` asks the provider itself:
`GET /models` with that key. All four (Anthropic, OpenAI, Google's OpenAI-compatible surface,
OpenRouter) answer with the same `{"data":[{"id":…}]}` envelope, so one parser serves them; only
the auth header and Google's `models/` id prefix differ.

This was chosen over a hardcoded catalog because provider slugs change often, and a stale slug
fails at request time with a 404 the user cannot act on. Asking the provider cannot go stale.

App side, in `app/src/ai/llms.rs`, mirroring how custom endpoints already work:
`provider_llms` is refetched on `ApiKeyManagerEvent::KeysUpdated` and at startup, and is chained
into the three model pickers, `model_info_for_id`, and `fallback_llm_info`. A provider that
fails contributes nothing and is logged, not reported — with several keys configured, one being
unreachable is ordinary. Both the fetch and the `LLMInfo` builder are `#[cfg(feature =
"local_inference")]`, because that dependency is optional and the normal build takes its catalog
from the server.

The provider slug is the `LLMInfo::id`, which is also `ModelConfig.base`, which is also what
`local_inference` sends — one string end to end, no mapping table.

**Verified against a real provider on 2026-08-19.** `crates/local_inference/tests/live_provider.rs`
holds three `#[ignore]`d tests that run the crate against a real endpoint, given
`LOCAL_INFERENCE_BASE_URL`, `LOCAL_INFERENCE_API_KEY`, and `LOCAL_INFERENCE_MODEL`. They stay out
of a normal `cargo test`, and no key goes in the repo. All three pass against an OpenAI-compatible
LiteLLM gateway running `deepseek-v4-pro`:

1. A plain question streams text back, inside the right event envelope.
2. A question about the machine produces a `run_shell_command` call that maps onto the proto the
   client runs.
3. A tool result goes back to the model, and the model answers from it.

The third test failed at first, and it found a real bug that only a live run could show.

**A reasoning model can demand its thinking back.** The gateway answered 400:
`The `reasoning_content` in the thinking mode must be passed back to the API`. DeepSeek in
thinking mode rejects an assistant message that carries tool calls but no `reasoning_content`.
`convert.rs` dropped every `AgentReasoning` message, on the grounds that reasoning carries no
instruction to replay, so the field was never there to send.

Probing the gateway direct fixed the shape of the fix: the field only has to be **present**. An
empty string is accepted, and a plain assistant message with no tool calls needs it. So:

- `convert.rs` now carries `AgentReasoning` into `Turn::Assistant::reasoning`.
- `openai.rs` sends `reasoning_content` on an assistant message that has tool calls — the captured
  thinking, or an empty string when the reply streamed none.
- It goes to **custom endpoints only**. The field is outside the official schema, and a
  first-party provider may reject an unknown message field. There is no key here to test
  `api.openai.com` with, so that path keeps the official schema.
- Anthropic is left alone. It carries thinking in a signed `thinking` block and validates the
  signature on replay, so a rebuilt block would be invalid, and Anthropic does not ask for one.

Still open in this phase:

1. `call_mcp_tool` is not mapped yet, so the agent cannot use MCP servers.
2. No retry and no context-window management.
3. OpenRouter answers with several hundred models and they are all listed. The picker has search,
   but the list wants a cap or a filter.
4. The app-side wiring at `app/src/ai/agent/api/impl.rs:141` ran for the first time on
   2026-08-19. A question that needed a command worked end to end in the app: the model called
   `run_shell_command` with `find . -name '*.rs' -type f | wc -l`, the client ran it, and the
   answer came back as "There are **4,053** `.rs` files". The follow-up question in the same
   conversation then failed, which is how the tool-pairing bug above was found.

**The user's question was never stored, so the model never saw it again — FIXED.** Decoding an
`agent_tasks` row showed three messages — `AgentReasoning`, `ToolCall`, `AgentOutput` — and no
`UserQuery`. The emitter never added one, because the client sends the question in
`Request::input` and Warp's server was the thing that echoed it back as a message to store.

Two consequences, one visible and one silent:

- Every conversation logged `missing an initial query` and was dropped from the history panel.
  `AgentConversationSummary` derives `initial_query` by looking for a `UserQuery` in the root task
  (`crates/persistence/src/model.rs:1097`), and there was never one to find.
- On a follow-up question the model was shown its own past replies and tool calls, but not the
  questions that prompted them. It answered with half the conversation missing, and nothing
  reported that.

`emit.rs` now adds the question to the task in the opening transaction, before the reply to it. A
request carrying tool results instead of a question — the next step of an agent loop — gets
nothing, so a turn cannot gain a second question.

It does not double up in the UI. A `UserQuery` message only becomes a rendered input when
`Task::add_messages` is told to convert input messages, which the client does for a shared-session
viewer alone; in a normal session its own copy already fills the exchange. Here the message lands
in the task's message list, which is what gets persisted and replayed.

Acceptance:

- [x] An AI conversation runs from end to end with a user key.
- [x] Tool calls map to the proto, and their results return to the model.
- [x] The same conversation runs through the app UI. **User-tested 2026-08-19.** A question needing
      a command worked, a follow-up in the same conversation worked, and asked what the first
      question had been the model answered "你刚才问的是：这个目录下有多少个 .rs 文件". The task for
      that conversation decodes as `UserQuery`, `ToolCall`, `AgentReasoning`, `ToolCall`,
      `AgentOutput`, `UserQuery` — both questions stored in order.
- [ ] The only network traffic goes to the provider host. (Checked at startup in Phase 1; not yet
      re-checked during an AI request.)

### Phase 4 — Delete the dead code

One small step for each module. Run `./script/format`, `cargo clippy`, and `cargo nextest run`
after each step.

**The test suite needs `cargo nextest`, and the bootstrap script does not install it.** Plain
`cargo test -p warp --lib` reports 13 failures. Every one of them passes when run alone, and the
failing set changes between runs, so they are cross-test interference through process-global
state, not bugs — the Phase 2 note that called them pre-existing debt was right that they are not
ours, but wrong to treat them as failures at all. `cargo nextest run` gives each test its own
process: **6453 pass, 0 fail**. Install with:

```sh
curl -LsSf https://get.nexte.st/latest/mac -o /tmp/nextest.tar.gz && tar zxf /tmp/nextest.tar.gz -C ~/.cargo/bin
```

1. `crates/warp_tui` and the app-side TUI support — **DONE**.

   210 files and ~97.5k lines removed. `tui` was in neither the `default` nor the `simplewarp`
   feature set, so none of it was in the GUI build and the app suite could not regress from the
   deletion itself.

   | Removed | What |
   | --- | --- |
   | `crates/warp_tui` | The whole crate. Nothing depended on it; it depended on `warp` with the `tui` feature. |
   | `app/src/tui/`, `tui_export.rs`, `tui_onboarding_markers.rs`, `tui_test_support.rs` | The app-side TUI modules. |
   | `app/src/ai/tui_api_keys.rs` | Existed only so the GUI reloaded keys after the TUI process changed them. |
   | `app/src/server/server_api/tui_onboarding.rs` | TUI onboarding client. |
   | `app/src/settings/tui_{theme,voice,zero_state,autoupdate}.rs` | Four `SettingSurfaces::TUI` settings groups, plus their registrations. |
   | `LaunchMode::Tui`, `TuiEntryPoint`, `TuiMountFn`, `run_tui*` | ~35 sites in `lib.rs`, including every match arm. |
   | `PersistenceScope::Tui`, `PersistedDataScope::TuiFrontend` | The TUI's separate database. |
   | `script/run-tui`, `.agents/skills/tui-*` | The runner script and three skills that described deleted code. |
   | The `tui` cargo feature | Removed from `app/Cargo.toml`; ~90 `cfg` sites simplified. |

   **Two tests failed, and both were the substitutions rather than the deletion.** Three tests
   built their model with `LaunchMode::Tui` purely to reach a code path, so each needed the real
   equivalent, not a mechanical swap:

   - `ProfileSource::for_launch_mode` treats `App`/`Test` as importing legacy cloud profiles but
     `Tui` as not, so a TUI launch was authoritative for settings **immediately** while `Test`
     starts in `PendingLegacyImport`, where writes do not persist. `llms_tests` now seeds an
     explicit profile collection, which is the state the TUI reached by seeding its own.
   - `file_backed_execution_profiles_enabled` returned `true` for `Tui` *regardless of the
     rollout flag*, so a block in `profile_sources_preserve_state_across_migration_and_rollout`
     existed to assert exactly that. Its subject is gone, so the block is gone; swapping the
     launch mode would have made it a duplicate of the test above it.

   With the TUI gone, no launch mode reaches `SettingsCollection` without also importing legacy
   profiles, so `migrates_legacy_cloud_profiles` is now always `true` there. Left alone for now.

   Acceptance:

   - [x] `cargo nextest run -p warp --lib`: **6435 pass, 0 fail** (6453 before; the 18 removed
         are the TUI's own tests).
   - [x] `cargo nextest run -p local_inference`: 83 pass.
   - [x] `cargo clippy -p warp --lib --all-targets`: clean. Six pre-existing `redundant_closure`
         warnings from Phase 2 were fixed on the way.
   - [x] `cargo check --no-default-features --features simplewarp --bin simplewarp`: clean.
   - [ ] Not re-run in the app. The deletion is compile-time only, but Phase 2 and 3 both show
         that only running it proves it.

   The `#[allow(dead_code)]` attributes left behind where `#[cfg_attr(not(feature = "tui"), …)]`
   used to sit mark items the TUI alone used; they are dead now and fall to the later cloud and
   ambient-agent steps.

1b. The TUI rendering engine in `warpui_core` — **DONE**.

   61 more files. Nothing enabled `warpui_core/tui` once the app's `tui` feature was gone, so the
   whole feature was unreachable:

   | Removed | What |
   | --- | --- |
   | `src/elements/tui/` | The cell-grid element library — the `TuiElement` trait, 45 files. |
   | `src/runtime/` | The terminal runtime: renderer, event conversion, terminal probe. |
   | `src/presenter/tui*`, `core/app/tui.rs`, `core/view/tui.rs`, `core/view/context/tui.rs` | The TUI halves of the presenter, app, and view. |
   | `StoredView::Tui` | The TUI arm of the shared view registry, and ~15 match arms in `core/window.rs`. |
   | `tests/tui_integration.rs`, `examples/tui_file_viewer.rs` | With their `required-features` target sections. |
   | The `tui` feature and **the `ratatui` dependency** | `ratatui` no longer appears in `Cargo.lock` at all. |
   | `AIExecutionProfile::default_profile_for_tui` | Orphaned when the TUI seeding went in step 1. |

   `StoredView` is now a single-variant enum wrapping `Box<dyn AnyView>`. Collapsing it to a plain
   newtype would touch every use site, so it is left as is.

   Acceptance:

   - [x] `cargo nextest run -p warpui_core`: 307 pass, 0 fail.
   - [x] `cargo nextest run -p warp --lib`: 6435 pass, 0 fail — unchanged by this step.
   - [x] `cargo clippy` on both crates: clean. `./script/format --check`: clean.
   - [x] `cargo check --no-default-features --features simplewarp --bin simplewarp`: clean.

1c. The TUI **surface metadata** — NOT STARTED, and bigger than it looks.

   The front-end is gone but the *concept* of a TUI surface is still woven through settings and
   command declarations. This is one coupled cluster and must move together:

   | Surface marker | Scale |
   | --- | --- |
   | `SettingsMode::Tui` + `SettingSurfaces::TUI` | ~50 sites, including the `generate_settings_schema` binary, which emits a separate `tui` schema |
   | `SlashCommandSurfaces::TuiOnly` | ~20 TUI-only slash commands in `static_commands/commands.rs`, plus ~30 assertions in `commands_tests.rs` |
   | `BundledSkillActivation::TuiOnly` | with the `resources/bundled/skills/tui-migrate-setup` asset |
   | `ExecutionMode::Tui` / `AppExecutionMode::is_tui` | `is_tui()` has exactly one caller: `BundledSkillActivation::TuiOnly` |
   | `warp_core::paths::tui_{state_dir,config_local_dir,mcp_config_file_path}` | read by `warp_managed_paths_watcher`, `settings/mod.rs`, and the skills loader |
   | MCP behaviour keyed on `settings_mode() == Tui` | `templatable_manager/native.rs`, `file_based_manager.rs`, `file_mcp_watcher.rs` |

   Unlike steps 1 and 1b, most of this is not behind a `cfg`, so the compiler will not find the
   dead paths — each one has to be read. It is a step of its own.
2. `app/src/drive/`, `app/src/billing/`, `app/src/cloud_object/`.
3. `app/src/auth/`, `app/src/remote_server/`, cloud paths in `app/src/workspaces/`.
4. The crates: `firebase`, `warp_server_client`, `warp_server_auth`, `graphql`,
   `cloud_object_*`, `warp_multi_agent_client`.
5. The `FeatureFlag` variants that are no longer in use.

## Risks

- **Deep coupling.** The cloud crates appear in ~147 files. This is why deletion is last.
- **`skip_login` only makes errors.** Phase 2 must hide the UI, not show error toasts.
- **The agent loop moves to the client.** Phase 3 must add the system prompt, the tool
  schemas, and the loop control that the server did before. Quality can differ from Warp.
- **Model configuration.** `should_refresh_model_config` and the model list come from the
  server. The local build needs its own model list.

## Definition of done

- [x] Phase 0: baseline build is green.
- [x] Phase 1: `simplewarp` starts offline, direct to a terminal, with 0 startup errors.
- [x] Phase 2: no cloud, login, or billing UI is reachable in any surface checked so far —
      settings, palette, native menus, block menu, agent page, history panel, toolbelt.
      User-tested 2026-08-19. Cloud mode, ambient agents, and the remote server UI are
      unchecked, and only running the app can check them.
- [x] Phase 3: the `local_inference` crate is written, tested, wired in, and verified by a
      real conversation in the app.
- [ ] Phase 3b: a built-in model list, MCP tool support.
- [ ] Phase 4: the cloud crates and the TUI are removed. Steps 1 and 1b are done — the TUI
      front-end and its rendering engine are gone (~116k lines, and the `ratatui` dependency).
      Step 1c (TUI surface metadata) and the cloud crates remain.
- [x] An end-to-end AI conversation with a real key. **Done 2026-08-19** against an
      OpenAI-compatible LiteLLM gateway, by the live tests in
      `crates/local_inference/tests/live_provider.rs`. Text, a tool call, and a tool result all
      round-trip. It found and fixed the `reasoning_content` bug described in Phase 3.
- [x] The same conversation through the app UI. **User-tested 2026-08-19.** It found three
      more bugs that the crate-level tests could not: the missing user query, a launch crash
      from a hidden menu binding, and a login wall on the local conversation history.

## Git: this repository is a fork

`wynn5a/simplewarp` is a fork of `warpdotdev/warp`. In a fork, `gh pr create` defaults its base
to the **parent**, so a bare `gh pr create` opens a pull request against Warp's public upstream
repository. This work is a private derivative and is not meant to go there.

Two guards, because the first one is per-clone and a fresh clone loses it:

```sh
gh repo set-default wynn5a/simplewarp        # writes remote.origin.gh-resolved to .git/config
gh pr create --repo wynn5a/simplewarp --base master --head <branch> ...
```

Always pass `--repo` explicitly. The same care applies to any `gh` command that acts on a repo,
and to `git push`: push to `origin` only, and never add a remote pointing at `warpdotdev/warp`.

## Build commands

`DEVELOPER_DIR` is necessary until `xcode-select` is switched.

```sh
export DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer
cargo run --no-default-features --features simplewarp --bin simplewarp   # the app
cargo test -p local_inference                                            # the AI adapter
cargo check -p warp --bin warp-oss                                       # no regression
```

## How to test the AI

### Without the app, against a real provider

This needs no GUI build, so it runs even while the Xcode blocker stands. The tests are
`#[ignore]`d, so they never run by accident.

```sh
export LOCAL_INFERENCE_BASE_URL=https://example.com/v1
export LOCAL_INFERENCE_API_KEY=sk-...
export LOCAL_INFERENCE_MODEL=some-model
export LOCAL_INFERENCE_SCHEMA=anthropic        # optional; OpenAI Chat Completions is the default
cargo test -p local_inference --test live_provider -- --ignored --nocapture
```

The app keeps its keys in the login keychain, under the service `dev.simplewarp.SimpleWarp` and
the account `AiApiKeys`, as one JSON blob of provider keys and custom endpoints. To test with the
endpoint that the app already holds, read it from there instead of pasting a key:

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
2. Paste a provider key, or add a custom endpoint (base URL plus model slug). A custom endpoint
   also covers a local server such as Ollama at `http://localhost:11434/v1`.
3. Ask the agent something. Watch `~/Library/Logs/simplewarp.log`, and check with
   `lsof -nP -iTCP -a -p $(pgrep -f simplewarp)` that the only connection is to the provider.
