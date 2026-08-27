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

1c. The TUI **surface metadata** — **DONE**.

   The front-end was gone but the *concept* of a TUI surface was still woven through settings and
   command declarations. Unlike steps 1 and 1b, almost none of this was behind a `cfg`, so the
   compiler could not find the dead paths — each had to be read.

   | Surface marker | What happened |
   | --- | --- |
   | `SettingsMode::Tui` + `SettingSurfaces::TUI` | Both collapse to GUI. `SettingSurfaces::ALL` now means the GUI alone, so the ~200 settings that declare `ALL` needed no edit. |
   | `SlashCommandSurfaces` | 17 TUI-only commands and their `SlashCommandKind` variants deleted; the 16 `GuiAndTui` declarations became `GuiOnly`, leaving `GuiOnly` as the only variant. |
   | The TUI-only dispatch arm | `slash_commands/mod.rs` held one arm for all 17 kinds whose body was `debug_assert!(false, "Attempted to execute TUI-only slash command in the GUI")`. Gone with them. |
   | `BundledSkillActivation::TuiOnly` | Removed, with the `resources/bundled/skills/tui-migrate-setup` asset and the `tui_settings_file_path` / `tui_mcp_config_file_path` template variables only that skill used. |
   | `ExecutionMode::Tui` / `is_tui()` | Removed. `is_tui()` had exactly one caller — the activation above. |
   | The two TUI-only settings | `TuiUsageDisplayMode` and the `TuiStatusline` config (185 lines in `settings/ai.rs`) were the only `SettingSurfaces::TUI` declarations. |
   | `warp_core::paths::tui_*` | `tui_state_dir`, `tui_config_local_dir`, `tui_mcp_config_file_path`, and the macOS `.warp_cli*` directory name. |
   | MCP behaviour keyed on `settings_mode() == Tui` | Three flags that are now constant-false, removed rather than pinned to `false`. |

   **Two of those MCP flags were load-bearing machinery, not one-line checks.**
   `FileBasedMCPManager` carried a whole deferral path — `defer_global_warp_autostart` plus
   `global_warp_servers_activated` across six sites — so the TUI could scan its global MCP config
   before login without starting servers. Its only non-test activation was the TUI login flow, and
   `activate_global_warp_servers` was already `#[cfg(test)]`, so the entire mechanism went. In
   `templatable_manager/native.rs` the OAuth callback mode was `Loopback` for the TUI and
   `CustomScheme` otherwise; only the custom-scheme branch survives.

   Acceptance:

   - [x] `cargo nextest run -p warp --lib`: **6404 pass, 0 fail** (31 fewer than 1b — the deleted
         TUI command, skill, statusline, and MCP-deferral tests).
   - [x] `cargo nextest run` on `warp_core` (46), `settings` (71), `local_inference` (83): all pass.
   - [x] `cargo check --all-targets` across the workspace, and clippy on `warp`, `warp_core`,
         `settings`, `warpui_core`: clean. `./script/format --check`: clean.
   - [x] `cargo check --no-default-features --features simplewarp --bin simplewarp`: clean.
   - [ ] Not re-run in the app. Settings, slash commands, and MCP startup all changed here, so
         this is the step most worth exercising by hand.

   Two single-variant types are left behind: `SettingsMode::Gui` and
   `SlashCommandSurfaces::GuiOnly`. Collapsing them would touch every settings and command
   declaration for no behaviour change, so they stay until there is a reason to move them.
2. **The cloud modules. Take them in coupling order, not the order written here.** Measured
   external references *into* each module:

   | Module | Size | References in from outside |
   | --- | --- | --- |
   | `app/src/billing/` | 3 files, 492 lines | **1** — DONE |
   | `app/src/remote_server/` | 24 files, 10.3k lines | 29, across 22 files |
   | `app/src/drive/` | 48 files, 23k lines | 184, across 110 files |
   | `app/src/auth/` | 17 files, 6.9k lines | 272, across 185 files |
   | `app/src/cloud_object/` | 12 files, 7k lines | **437, across 227 files** |

   `cloud_object` is not a deletion, it is a refactor of the persistence and sync layer; it must
   go last. `remote_server` looks small by reference count but is woven through the terminal, AI
   file access, and code review (`read_files`, `apply_diff_model`, `diff_state/remote`,
   `global_buffer_model`), each of which branches on local-vs-remote.

   **`billing` is DONE.** Its one external reference was the "shared object creation denied" modal
   in `workspace/view.rs`, but the events that opened it were raised in `drive/`, so eleven emit
   sites and two event variants had to go with it. Every site reads
   `if !has_capacity(..) { emit(modal); return; }`; only the `emit` was removed, so an object over
   the team's limit is still not created — what is lost is the modal explaining why.

   **A scripted removal is not safe here, and the compiler is not a sufficient check.** The script
   that stripped the emit calls walked backwards to the nearest line starting with `ctx`, which in
   `drive/panel.rs` swallowed the body of an unrelated match arm and two arms after it. That one
   surfaced as an unclosed delimiter, but a removal that still compiles would not have. Every
   scripted edit in this phase needs its diff read line by line — reading the `drive/index.rs`
   diff is what confirmed all seven `return;` statements survived.

   **`drive` turned out to be the same shape as `cloud_object` — a refactor, not a deletion —
   just at smaller scale.** Four commits landed cleanly on the assumption that `drive/` was pure
   dead UI (every entry point gated on `WarpDriveSettings::is_warp_drive_enabled`, which can never
   be true with no account): `bbc9ccff` redirected 17 files' `CloudObjectTypeAndId` import off
   the `crate::drive` barrel onto its real home, `cloud_objects::drive`; `0bbf18f6` deleted the
   drive-sharing onboarding block; `494c2a37` deleted the Warp Drive settings page; `cfd45d52`
   deleted the command-palette Warp Drive search subtree and, in one legitimate cascade, the
   `/prompts` inline terminal menu (they shared a `DataSource`) — 5999/5999 tests passing, down
   from 6015 by exactly the removed tests' own tests.

   That assumption broke on the rest. A traced survey of `sharing/`, `folders/`, `items/`, and
   `mod.rs`'s standalone types found three things load-bearing for code that survives this phase:
   `sharing/dialog/`'s `SharingDialog` is live UI, reachable for `ShareableObject::AIConversation`
   sharing (not just Drive objects) from `pane_group/`, `terminal/`, `workflows/`, `env_vars/`,
   `notebooks/`, and `ai/ai_document_view.rs` — it only looks Drive-specific because of its file
   path; `items/{workflow,notebook,folder,env_var_collection,ai_fact,ai_fact_collection,
   mcp_server,mcp_server_collection}.rs`'s `impl WarpDriveItem` blocks are half load-bearing,
   since `ai/facts/view/rule.rs` and `ai/blocklist/block/view_impl.rs` call `icon()`/
   `display_name()`/`sync_status_icon()` on them to render sync-status icons and citation chips
   (only `click_action`/`preview`/`secondary_icon` are Drive-row-only); and `folders/mod.rs` plus
   `mod.rs`'s `DriveObjectType`/`DriveSortOrder`/`OpenWarpDriveObjectSettings`/`Args` are pure data
   used by `cloud_object/breadcrumbs.rs`, `workspace/view.rs`, `pane_group/`, `workflow_pane.rs`,
   `notebook_pane.rs`, and URI parsing.

   A fourth finding is deferred rather than acted on: the `warp://drive/...` deep-link handler
   (`extract_server_id_and_object_type_from_warp_drive_link` → `root_view.rs`'s
   `open_warp_drive_object[_in_existing_window]`) is reachable code with no account gate, but every
   path it opens is a guaranteed dead end — it resolves a pane by `SyncId::ServerId`, which per
   3e/3f below can never succeed. The natural fix mirrors 3e/3f exactly (return "can't open, no
   account" immediately instead of routing to a doomed pane-open), but that touches `root_view.rs`
   and URI parsing on its own, so it is left as an open item rather than bundled in.

   The remaining `drive/` work is split into what deletes outright (the panel, its index, the
   `WarpDriveRow` rendering, the four Drive-only dialogs, the workflow-creation modal, import,
   export, and the `workspace/view.rs`/`left_panel.rs` `DrivePanel` wiring those views are wrapped
   in) versus what has to move out to `cloud_object/` (or a new home outside `drive/`, for the
   sharing dialog) before the directory can come out. Full detail is in the working plan file for
   this session, not reproduced here since it is not yet executed.

   **The "move out" half (Track B) is now done, in four commits, before any of the Track A
   deletions above it.** `6e3b9606` relocated `sharing/dialog/` (the live `SharingDialog`) and
   `sharing/mod.rs`'s `ShareableObject`/`ContentEditability`/`SubjectExt`/`UserKindExt`/
   `TeamKindExt` to a new `app/src/sharing/`, since the dialog serves `AIConversation` sharing,
   not just Drive objects. `44222607` moved `folders/mod.rs`'s `CloudFolder`/`CloudFolderModel`/
   `FolderId` re-exports and `CloudModelType` impl into `cloud_object/folders.rs`. `bf9bbe4e`
   moved the `WarpDriveItem` trait and `WarpDriveItemId` into `cloud_object/warp_drive_item.rs` —
   this is the one that had cloud_object/mod.rs importing back from `drive::items`, the reverse
   dependency this whole track exists to unwind. `3b84e4d1` moved `DriveObjectType`/
   `DriveSortOrder`/`OpenWarpDriveObjectArgs`/`OpenWarpDriveObjectSettings` out of `drive/mod.rs`
   into `cloud_object/drive_object_type.rs`. Every external and internal caller's import path was
   updated to the real new location (no compatibility re-export left behind in `drive/`), verified
   by a full `cargo check`/clippy/nextest/format pass after each commit — 5999/5999 tests still
   passing throughout. `items/{workflow,notebook,folder,env_var_collection,ai_fact,
   ai_fact_collection,mcp_server,mcp_server_collection,space}.rs` were left in place exactly as
   scoped: their `impl WarpDriveItem` blocks now import the trait from its new
   `cloud_object::warp_drive_item` home, but the files themselves wait on Track A's clippy sweep
   to prune their now-half-dead `click_action`/`preview`/`secondary_icon` methods before moving.
   Track A (the panel/index/dialogs/workflow-modal/import/export deletions) and Track C (the
   `warp://drive/...` deep-link handler) remain open, as scoped.

   **A fifth agent round traced Track A's six sub-steps before touching any of them, and found the
   whole deletion is blocked on Track C, plus a second reachable surface Track C's own note never
   mentioned.** The plan's sub-steps 1-4 (left_panel.rs's `ToolPanelView::WarpDrive` tab plumbing,
   `open_or_toggle_warp_drive`, the keyboard-nav `is_warp_drive_open` fallbacks, and the
   `update_warp_drive_view` forwarder) all bottom out, one hop or two down, in the same handful of
   `workspace/view.rs` methods that hold the live `ViewHandle<DrivePanel>`: `update_warp_drive_view`,
   `view_in_warp_drive`, `view_in_and_focus_warp_drive`, `open_object_sharing_settings`,
   `move_to_drive_space`, `has_warp_drive_initialized_sections`. Those six are not private to the
   left-panel tab. Two more things call into them:

   - **Track C's own deep-link handler** (`root_view.rs`'s `open_warp_drive_object_in_existing_window`,
     4 sites) is the *only* caller of `WorkspaceView::has_warp_drive_initialized_sections`, and one of
     three callers of `view_in_and_focus_warp_drive`. Since Track C is explicitly parked, this alone
     blocks deleting the field these methods close over.
   - **Not previously listed anywhere:** every open notebook/workflow/env-var-collection pane renders a
     breadcrumb trail (`workflows/workflow_view.rs:2946`, `notebooks/notebook.rs`,
     `env_vars/view/env_var_collection.rs`, via `ui_components::breadcrumb::render_breadcrumbs` and
     `cloud_object::breadcrumbs::ContainingObject`) whose click handler dispatches `ViewInWarpDrive` →
     `view_in_warp_drive`/`view_in_and_focus_warp_drive`. Unlike the panel itself, this breadcrumb is
     **not** behind `is_warp_drive_enabled` — `update_breadcrumb` populates it from
     `containing_objects_path()` for any cloud workflow/notebook/env-var collection, including ones
     created locally in `Space::Personal`, so it is live, clickable UI in this build, not just
     compiled-reachable dead code. Clicking it opens the left panel's Warp Drive tab, which then shows
     "Sign in to access Warp Drive" instead of navigating anywhere — a real (if minor) dead end, same
     shape as Track C's, that nobody has traced or fixed yet. The handler exists in six places:
     `workflows/workflow_view.rs`, `notebooks/notebook.rs`, `env_vars/view/env_var_collection.rs`, and
     the three `pane_group/pane/{workflow,notebook,env_var_collection}_pane.rs` wrappers that forward
     `ViewInWarpDrive` events up to `workspace/view.rs`, plus `drive/workflows/modal.rs`'s own copy.

   Net effect: **none of sub-steps 1-4 can land as a clean, compiling deletion without either
   touching Track C (out of scope this round) or first deciding what to do with the breadcrumb
   click surface (its own trace — e.g. does a team-owned or shared-with-me object's breadcrumb
   behave differently? — not a mechanical deletion).** No code was changed this round; forcing a
   partial cut here would either leave the field referenced from Track C (a compile error) or
   silently change breadcrumb click behavior without having verified it's actually inert everywhere.

   **The same round also re-checked three more Track A inventory items against actual callers, and
   found them misclassified the same way `sharing/`/`folders/`/`items/` were — load-bearing code
   living under `drive/` by path, not by function — while confirming a few really are panel-only:**

   | Item | Verdict | Why |
   | --- | --- | --- |
   | `drive_helpers.rs` | **Not deletable, belongs in Track B.** | Its anonymous-user object-limit checks are called from `server/cloud_objects/update_manager.rs` (real object-creation gating), `workflows/workflow_view.rs`, `env_vars/view/menus.rs`, and `notebooks/notebook.rs` — all outside the Drive panel and unrelated to `panel.rs`/`index.rs` rendering it via `render_personal_object_limit_row`. |
   | `drive/workflows/` | **Mixed, like `items/`.** | `arguments.rs`, `enum_creation_dialog.rs`, `workflow_arg_selector.rs`, `workflow_arg_type_helpers.rs` back the live workflow-argument editor (`workflows/workflow_view.rs` and its `alias_argument_selector.rs`/`argument_editor.rs`); `arguments.rs` is also used by `notebooks/editor/notebook_command.rs`. Only `modal.rs` (wired into `workspace/view.rs` as `WorkflowModal`/`WorkflowModalEvent`, itself supposedly one of Phase 2's already-hidden "Save as workflow"/"Import to Drive" sites, not independently re-verified this round) and `ai_assist.rs` look like the genuine Drive-modal-only remainder. |
   | `cloud_object_styling.rs` | **Not deletable, belongs in Track B.** | Used well outside Drive: `cloud_object/warp_drive_item.rs`, `workspace/view/vertical_tabs.rs`, `workflows/workflow_view.rs`, three `search/` files, `notebooks/editor/embedded_item.rs`. |
   | `cloud_action_confirmation_dialog.rs` | **Not Drive-panel code at all.** | Its only caller is `settings_view/teams_page.rs` (leave-team/delete-team confirmation) — `drive/index.rs` never uses it. Misfiled under `drive/` by path. Whether it's deletable depends on whether the Teams settings page itself is still reachable after Phase 2 hid it from the sidebar — a settings-page question, not a Drive-panel one. |
   | `cloud_object_naming_dialog.rs`, `empty_trash_confirmation_dialog.rs`, `items/item.rs`'s `WarpDriveRow` | **Confirmed panel-only**, no external callers found. | Still blocked on the same root cause as the rest: `index.rs` can't go until `panel.rs`/`DrivePanel` can go. |

   So the practical next step for a future round is not "start on sub-step 1" but: decide the
   breadcrumb click surface's fate, then resolve (or knowingly re-scope past) Track C's four call
   sites — only after that does any of Track A's deletion list stop being blocked.

   **A sixth agent round neutralized both of the fifth round's blockers — Track C's deep-link
   handler and the breadcrumb click surface — without touching the `DrivePanel`/`left_panel.rs`
   wiring itself, which is still Track A's job for a future round.**

   For the deep-link handler: traced whether `SyncId::ServerId` can ever resolve end-to-end. It
   can't — `CloudModel` only gains `ServerId`-keyed entries via a successful server sync
   (`upsert_from_server_object`), and every warp-server request already fails immediately (step
   3e/3f's `LOCAL_ONLY_MESSAGE`/`local_only_error()`, unconditional regardless of cargo features,
   not just in the `simplewarp` feature set). A third, independent implementation of the same fact
   already existed to confirm this against: `workspace/view.rs`'s in-app `OpenWarpDriveLink` click
   handler (for a `warp://drive/...` link clicked inside a notebook, as opposed to the OS-level
   URI) already guards with `cloud_model.get_by_uid(...).is_none()` before falling through to a
   "Resource not found or access denied" toast, and the existing-window deep-link handler's
   `Folder`/`EnvVarCollection` arms already had the identical guard — only its `Notebook`/
   `Workflow` arms, and the free-standing new-window `open_warp_drive_object`, were missing it and
   would silently open a pane/window that could never load. Hoisted the check above the `match` in
   `open_warp_drive_object_in_existing_window` so it applies to all four object types uniformly,
   and added the same check to `open_warp_drive_object` (`419746ae`).

   For the breadcrumb click surface: gave `ContainingObject`
   (`app/src/cloud_object/breadcrumbs.rs`) a `drive_viewable` flag backing its existing
   `Breadcrumb::enabled()` (previously hardcoded `true`) and a `disable_drive_link()` setter,
   called from the three UI update sites (`WorkflowView::update_breadcrumb`,
   `notebooks/notebook/details_bar.rs`'s `DetailsBar::update_breadcrumbs`,
   `env_vars/view/fixed_view_components.rs`'s `EnvVarCollectionView::update_breadcrumbs`) whenever
   `WarpDriveSettings::is_warp_drive_enabled(ctx)` is false. `Hoverable::dispatch_event` already
   returns before reaching its click handler when disabled, so this reuses the existing
   enable/disable mechanism rather than adding a new one (`d4f62eb5`). First attempt threaded the
   same check into `containing_objects_path()` itself — the shared trait method the UI and the
   plain-text `breadcrumbs()` helper both call — and it broke
   `cloud_object::model::persistence::tests::test_breadcrumbs`, whose harness never registers the
   `WarpDriveSettings` singleton; the fix belongs at the three UI call sites, not in the shared
   data lookup.

   Track A's six blocked methods are **not yet fully caller-free**, though: `drive/workflows/
   modal.rs`'s `WorkflowModal` (reachable from `terminal`/`workspace`'s "create workflow" actions —
   a live surface, not the independently-re-verified-unreachable one the fifth round's table
   assumed it might be) still calls `view_in_warp_drive`, and `ai/ai_document_view.rs`'s "Show in
   Warp Drive" pane-header menu item (shown whenever a document has a synced Drive link) still
   calls `AIDocumentEvent::ViewInWarpDrive` → `view_in_and_focus_warp_drive`. Both were out of
   scope this round. The command-palette `CommandPaletteItemAction::ViewInWarpDrive`/
   `Event::ViewInWarpDrive` path (`search/command_palette/{mixer,view}.rs`) is already dead —
   nothing constructs the action variant since `cfd45d52` deleted the Drive search subtree that
   used to — but the compiler doesn't flag it (a `pub` enum variant, matched but never built), so
   it's a leftover for the eventual clippy sweep, not a live blocker. A future round should trace
   and neutralize (or confirm dead) the workflow modal and the AI Document menu item before
   assuming these six methods are safe to delete.

   **A seventh agent round closed out both of those, and Track A's six blocked methods are now
   caller-free.** Tracing `WorkflowModal::view_in_warp_drive`'s two call sites (a breadcrumb click
   in the modal header, and `ForceClose` replaying a pending breadcrumb click) found both are
   unreachable in production, not merely dead-ended like Track C's cases: the breadcrumb header
   only renders when `self.breadcrumbs` is `Some`, which only happens when `self.workflow_id` is
   `Some` — but `workflow_id` is set to `Some` only inside `populate()`, an
   `#[allow(dead_code)]` method nothing calls outside `modal_tests.rs`; the modal's real entry
   point, `open_with_new`, always sets it to `None`. So `compute_breadcrumbs` always produces
   `None`, the breadcrumb header branch (and the `WorkflowModalAction::ViewInWarpDrive` it would
   dispatch) never runs, and `clicked_breadcrumb` (read by `ForceClose`) is always `None` too —
   the earlier "reachable from terminal/workspace's 'create workflow' actions" characterization was
   about the modal itself, which is live, not about this specific breadcrumb sub-feature inside it,
   which isn't. Deleted the whole dead thread (breadcrumbs/clicked_breadcrumb fields,
   `compute_breadcrumbs` and the `CloudModel` subscription that only existed to call it, the
   `ViewInWarpDrive` action/event variants, the breadcrumb header render branch) rather than
   gating it, since — unlike the breadcrumb surfaces Track C fixed, which rendered and dead-ended
   on click — this one never rendered at all (`86e98d5c`).

   `ai_document_view.rs`'s "Show in Warp Drive" menu item turned out to be the same shape: it (and
   its sibling "Copy link") only appears when `get_document_warp_drive_object_link` returns
   `Some`, which requires `get_document_save_status` to return `Saved`, which requires `sync_id`
   to resolve to `SyncId::ServerId`. Every place that sets a `ServerId` sync_id
   (`set_document_server_backing` via `reconcile_document_server_backing`/
   `reconcile_server_backed_notebook`, `create_document_from_notebook`,
   `hydrate_saved_plan_from_warp_drive`) requires a matching notebook already present in
   `CloudModel` with a real `ServerId` — i.e. an object actually synced from warp-server, which
   per 3e/3f can never happen in this build. `sync_id` can reach `SyncId::ClientId` locally (giving
   `AIDocumentSaveStatus::Saving`), but never `ServerId`/`Saved`, so the menu item was never
   pushed. Deleted `AIDocumentAction::ShowInWarpDrive`, its `handle_action` arm, the menu item
   construction, `AIDocumentEvent::ViewInWarpDrive`, and the `ai_document_pane.rs` handler that
   forwarded it to `pane_group::Event::ViewInWarpDrive` (`333db0be`). Left "Copy link" alone — same
   always-false gate, but it doesn't call into Warp Drive, so it's out of scope here; a future
   clippy sweep can revisit it alongside the already-known-dead command-palette
   `ViewInWarpDrive` action.

   Re-verified all six methods (`has_warp_drive_initialized_sections`,
   `view_in_and_focus_warp_drive`, `view_in_warp_drive`, `open_object_sharing_settings`,
   `move_to_drive_space`, `update_warp_drive_view`) against the whole `app/src` tree: every
   remaining textual caller is now either (a) `workspace/view.rs`'s own internal calls between the
   six methods, (b) `drive/panel.rs`'s identically-named `DrivePanel` methods (a different struct,
   itself part of Track A's deletion list), (c) `root_view.rs`'s deep-link handler (Track C,
   guarded dead by `419746ae`), or (d) the breadcrumb-emitting `WorkflowView`/`NotebookView`/
   `EnvVarCollectionView`/`pane_group` family (Track C, disabled by `d4f62eb5`). No caller remains
   outside those four already-accounted-for groups. **This is the real unblock: Track A's deletion
   (the `ToolPanelView::WarpDrive` tab plumbing, `panel.rs`/`index.rs` themselves, and everything
   else on its sub-step list) can now proceed without re-tracing any of this**, though the deep-
   link handler (c) and breadcrumb family (d) call sites will still need companion edits in the
   same commits that remove the six methods, since they're guarded-dead rather than deleted.

   **An eighth agent round found the seventh round's "real unblock" conclusion was wrong about
   one of the six methods, `update_warp_drive_view`, and by extension about `ToolPanelView::
   WarpDrive`/`panel.rs`/`index.rs` themselves — they are not dead, they back real, everyday,
   account-free features, and deleting them is a regression, not a cleanup.**

   The other five methods held up under a full compiler-driven sweep (delete, let `cargo check`
   enumerate every remaining caller, fix each one, repeat) and are gone for real:
   `has_warp_drive_initialized_sections`, `view_in_and_focus_warp_drive`,
   `open_object_sharing_settings`, `move_to_drive_space` are deleted; `view_in_warp_drive` survives
   because `set_selected_object` (kept — see below) still calls it. Four more dispatch chains that
   fed exclusively into the five deleted methods came out with them, each independently confirmed
   dead by its own evidence, not by association: the command-palette `ViewInWarpDrive` action
   (`data_sources.rs` already said in a comment that nothing produces its `ItemSummary` anymore,
   since the Drive search subtree that made them was deleted in `cfd45d52`); the breadcrumb
   "view in Warp Drive" click for Notebook/Workflow/EnvVarCollection panes (already permanently
   disabled by `d4f62eb5`, so the action it would dispatch could never fire — same shape as
   `86e98d5c`'s WorkflowModal finding, given the same treatment: deleted, not re-guarded); Notebook's
   "Move to `<team>` space" menu item (gated on `is_on_server()`, impossible with no warp-server
   connection); and the invitee-email "open Drive share dialog" flows in `open_notebook`,
   `NotebookView::load`, and `WorkflowView` (each requires a `settings.invitee_email` that only a
   real server-issued invite link could supply). `root_view.rs`'s deep-link handler and
   `workspace/view.rs`'s own `OpenWarpDriveLink` handler got their promised companion edits,
   collapsing their now-dead per-type arms into the existing "unsupported" fallback. Landed as
   `6802b408` (the sweep) and `371f6424` (the DrivePanel/DriveIndex methods orphaned by it:
   `move_object_to_team_owner`, `set_focused_item`, `has_warp_drive_initialized_sections`,
   `reset_and_open_to_main_index`, `has_initialized_sections`).

   `update_warp_drive_view` is different in kind, not degree: it is a generic forwarder
   (`left_panel_view.warp_drive_view().update(ctx, |warp_drive, ctx| update_fn(warp_drive, ctx))`)
   that **other, unrelated, genuinely-reachable features** route through to reach real
   `DrivePanel`/`DriveIndex` mutation logic that has nothing to do with the panel being visible.
   Found by tracing one specific caller that looked anomalous — `pane_group::Event::
   OpenAddPromptPane` (reachable from a terminal slash command, `/prompts` — still present after
   `cfd45d52` only removed the command-palette search subtree, not the slash command) calls
   `drive_panel.create_workflow_with_content`/`open_cloud_object_dialog` to create a new
   AI-agent-mode workflow with **no account or Drive-tab click required**. Pulling that thread
   further: `WorkspaceAction::CreatePersonalFolder`/`CreateTeamFolder`/`CreateTeamNotebook`/
   `CreateTeamEnvVarCollection` all call the same `open_cloud_object_dialog`, and — critically —
   also set `current_workspace_state.is_warp_drive_open = true` themselves, which only does
   anything because `ToolPanelView::WarpDrive`/`LeftPanelAction::WarpDrive` still exist to switch
   the left panel to it. `CloudObjectNamingDialog` (the "name your new folder" prompt) is rendered
   from *inside* `DriveIndex::render()` — it is not a standalone overlay like the app's other
   modals — so if the tab that shows `DriveIndex` can never be selected, the dialog these actions
   open is invisible and the object silently never gets named. **`CreatePersonalFolder` needs no
   team and no account**: creating a personal folder is an ordinary, always-available action in
   this build, and it was about to be silently broken.

   This was caught before landing: a first pass deleted the whole tab-plumbing family (commit not
   kept), got a clean compile and a full green test run — `cargo nextest` does not exercise "click
   the button, does a dialog appear" — and only the `create_cloud_object_dialog` render chain
   would have caught it, which nothing in the suite does. Reverted with `git checkout` before
   committing anything broken; re-landed the round with `ToolPanelView::WarpDrive`,
   `LeftPanelAction::WarpDrive`, `MouseStateHandles::warp_drive_button`, `LeftPanelView::
   warp_drive_view`/`warp_drive_view()`, `CurrentWorkspaceState::is_warp_drive_open`,
   `open_or_toggle_warp_drive`, `WorkspaceAction::ToggleWarpDrive`, and every other piece of Track
   A's originally-scoped "tab/enum plumbing" (sub-step 1) and "keyboard-nav fallbacks" (sub-step 3)
   left untouched. Two toast-driven `WorkspaceAction`s needed small fixes rather than deletion,
   since they're reachable independent of everything above: `ViewObjectInWarpDrive` (the "Plan
   synced to your Warp Drive" toast's "View" link, shown after any successful local create/update)
   now calls the surviving `view_in_warp_drive` instead of the deleted `view_in_and_focus_warp_drive`;
   `OpenObjectSharingSettings` (dispatched only by `sharing/dialog/inheritance.rs`'s "inherited from
   `<parent folder>`" link) is now a no-op, since its one caller needs ACL data synced from
   warp-server — impossible here — so it was already unreachable, but `sharing/` is otherwise live
   code and not this round's to redesign.

   **Track A's sub-steps 1 ("tab/enum plumbing") and most of 6 ("delete `panel.rs`/`index.rs`") are
   not safe to attempt, full stop, not just "not yet attempted."** `DrivePanel`/`DriveIndex` are not
   a dead UI shell wrapping a few load-bearing types the way `sharing/dialog/`, `folders/mod.rs`,
   and `items/mod.rs`'s trait were (Track B's finding) — they are also the *only implementation* of
   several real, reachable, account-free object-creation flows (new personal folder, new team
   folder/notebook/env-var-collection, new agent-mode prompt via `/prompts`), invoked through a
   left-panel tab that is permanently unselectable via normal navigation (the toolbelt button and
   `ToggleWarpDrive` binding both gate on `is_warp_drive_enabled`, always false) but is still
   force-switched-to programmatically by exactly those flows so their naming dialog has somewhere
   to render. Deleting the tab without first giving those dialogs a new, non-Drive-panel home is a
   redesign of "how does the app prompt for a new object's name," not a deletion — out of scope for
   an agent round scoped as cleanup. A future round could pursue this (extract
   `CloudObjectNamingDialog`'s render into a real standalone overlay, callable without a `DriveIndex`
   in the tree — mirroring how `SharingDialog` already stands alone after Track B), but it needs to
   be scoped and attempted as that redesign, not folded into "finish Track A."

   Sub-steps 2, 4, and 5 are now fully done (modulo the one method, `update_warp_drive_view`, that
   turned out to belong with sub-step 6 instead). Sub-step 6 is done for every file this round could
   confirm has no remaining reference to `DrivePanel`/`DriveIndex` — none, since the panel/index
   themselves are staying — so no files were deleted from `app/src/drive/` this round; the inventory
   from the fifth/sixth rounds (`items/item.rs`, the four dialogs, `drive_helpers.rs`,
   `cloud_object_styling.rs`, `drive/workflows/`, `import/`, `export/`) is unchanged and still
   pending, now clearly blocked on `panel.rs`/`index.rs` staying rather than on caller-tracing.

   **A ninth agent round did the redesign the eighth round scoped out — `CloudObjectNamingDialog`
   now has a standalone render path — and this landed cleanly (`e98f8de3`), but it does NOT unblock
   Track A the way the eighth round's write-up implied it would.**

   The established pattern for a standalone modal in this app (`WorkflowModal`, `ThemeCreatorModal`,
   `ModalViewState<T>` and its users) is: an app-level `ViewHandle<T: View>` field on `Workspace`,
   conditionally spliced into `Workspace::render()`'s own top-level `Stack` (next to `workflow_modal`
   at line ~25919) whenever it should be visible — independent of whatever tab/panel is or isn't
   selected. Reused verbatim rather than inventing anything: a new `CloudObjectNamingModal` (in
   `drive/cloud_object_naming_dialog.rs`, next to the dialog it wraps) holds a `ViewHandle<DriveIndex>`
   and does nothing but read that `DriveIndex`'s already-existing `cloud_object_naming_dialog` field
   for `is_open()`/`render()`, and forward `DriveIndexAction`s back to the real `DriveIndex::handle_action`
   for `Create`/`Rename`/`Close`. **Zero lines changed inside `CloudObjectNamingDialog` itself** — its
   render tree, its `ctx.dispatch_typed_action(DriveIndexAction::...)` calls, and its create/rename
   logic are byte-for-byte what they were; only a new, thin, always-mounted View sits between it and
   `Workspace`'s modal stack. `CreatePersonalFolder`/`CreateTeamFolder`/`CreateTeamNotebook`/
   `CreateTeamEnvVarCollection` now call the same `open_cloud_object_dialog` as before (that's still
   what actually opens the dialog's state) but no longer also set `is_warp_drive_open = true`. A new
   test (`test_create_personal_folder_shows_naming_dialog_without_opening_warp_drive_tab`) dispatches
   `CreatePersonalFolder` and asserts the standalone modal reports `is_open()` while
   `is_warp_drive_open` stays false — the exact assertion the eighth round noted nothing in the suite
   could make, and the one a bad Track A deletion would fail.

   **Re-checking `update_warp_drive_view`'s callers (the eighth round's stated precondition for
   revisiting Track A) found six more beyond the four now-fixed naming-dialog actions, all real and
   all unrelated to dialog visibility:** `set_selected_object` (called from `open_notebook` and
   friends — i.e. every time `CreatePersonalNotebook`, `CreatePersonalWorkflow`, or any existing-object
   open happens, reachable with no account), `reset_focused_index_in_warp_drive` and the two
   `is_warp_drive_open`/`set_focused_index` helpers, `pane_group::Event::OpenAddPromptPane` (the
   `/prompts` slash command, still calling `drive_panel.create_workflow_with_content`/
   `open_cloud_object_dialog` directly), and `WorkspaceAction::UndoTrash` (a toast "Undo" button after
   trashing any object). None of these force `is_warp_drive_open = true`, so none of them were part of
   the bug this round fixed — but all of them call real `DrivePanel`/`DriveIndex` methods that do real
   work (selection bookkeeping, workflow/folder creation, trash restoration), regardless of whether the
   tab is ever visually selected. **The eighth round's "nothing legitimate needs to force-open it
   anymore" was conflating two different things: force-opening the tab for dialog visibility (now
   fixed) and `DrivePanel`/`DriveIndex` existing at all as the implementation of these flows (never
   only a visibility question).** `panel.rs`/`index.rs` are exactly as load-bearing after this round
   as before it — Track A's sub-steps 1 and 6 remain correctly blocked, and no attempt was made to
   delete the tab/panel plumbing or sweep the pending `app/src/drive/` file inventory this round.

   Acceptance: `cargo nextest run -p warp --lib` — 6000 pass, 0 fail, 4 skipped (consistent with the
   established baseline). `cargo clippy -p warp --lib --all-targets` — no new warnings; every existing
   warning is in an unrelated file. `cargo check --all-targets` (workspace and `-p integration`),
   `--no-default-features --features simplewarp --bin simplewarp` — all clean. `./script/format
   --check` clean. Not re-run in the app — same testing-constraint note as every prior round in this
   section.

3. `app/src/auth/`, `app/src/remote_server/`, cloud paths in `app/src/workspaces/`.

   **`remote_server` was attempted and reverted, deliberately.** Deleting the module and crate
   left 158 errors across 37 files, and most of them were not import fixes. The blocker is
   `app/src/code/global_buffer_model.rs`: the code editor's `BufferSource` is
   `Local | Remote | ServerLocal`, and **both** non-local variants exist only for the remote
   server — `Remote` is the client editing a file on another host, `ServerLocal` is this process
   acting as the daemon. Both carry a `SyncClock` that drives version tracking, LSP `didChange`
   sync, debounced edit batching, and background diff parsing, across 31 sites in one 2.5k-line
   file. `crates/warp_files` has the same `Local | Remote` split in `FileBackend`.

   That is a redesign of the editor's version tracking, not a deletion — and the test file that
   covers it, `code/buffer_location_tests.rs`, is itself remote-buffer-specific, so the change
   would delete its own safety net. The earlier lesson applies with full force here: a wrong
   removal still compiles. It needs an attended pass.

   The same is true, more so, of `auth` (272 references) and `cloud_object` (437). Removing `auth`
   is not deleting a directory; it is deciding what the app means when there is no user at all,
   at every `is_logged_in()` call site. `cloud_object` is the persistence and sync layer that
   Warp Drive objects — workflows, notebooks, prompts, env-var collections, AI facts — are built
   on, so it goes only with those object types.
3b. **`app/src/server/experiments/` — DONE.** Server-assigned A/B experiment arms, fetched with
   the user's GraphQL profile and cached in SQLite. With no server the model was always empty, so
   every experiment already read as off.

   The chain was longer than the module: GraphQL user response → `UserProperties` →
   `AuthManager` → `ServerApiProvider::handle_experiments_fetched` → the model → a SQLite table,
   plus the workspace-metadata response carrying its own copy.

   **One consumer needed care, and it is the pattern to watch for.**
   `runner_controls_enabled` was `FeatureFlag::CloudAgentRunners.is_enabled() && experiment_arm`.
   Deleting only the experiment half would have left the flag alone in the `&&` and flipped the
   cloud runner controls **on** wherever the flag is set — `cloud_agent_runners` is in the
   `default` feature set. The behaviour-preserving answer is `false`, since the arm could never
   be assigned. Its test is rewritten to pin that: the controls stay off for both flag states, so
   a later change cannot quietly re-open the gate.

   `handle_experiment_change`, which re-registered settings-sync toggle bindings, had no other
   caller and went with it.

   Acceptance: 6401 app tests pass, `cargo check --all-targets`, clippy, format, and the
   `simplewarp` binary are clean. The diesel models in `crates/persistence` and the
   `server_experiments` table are left in place; nothing writes them now.

3c. **The channel config points at hosts that cannot resolve — DONE.** Locality rested on
   every call site being guarded or deleted, while the binary still carried
   `https://app.warp.dev`, two `wss://` endpoints, `https://oz.warp.dev`, and Warp's Firebase
   API key. The startup log printed all of them. The comment above the config called it "only a
   placeholder" — the kind of claim that stops being true without anyone noticing.

   `WarpServerConfig::local_only` and `OzConfig::local_only` now use RFC 2606 `.invalid`
   hostnames. They parse, which matters because callers parse them and some `expect` the parse
   to succeed, but they can never resolve. A request that escapes a deleted guard fails in DNS
   naming SimpleWarp instead of quietly reaching Warp. Session sharing is `None`, and there is
   no Firebase key to ship.

   This is the belt to the deletions' braces: locality becomes structural rather than a
   property that has to hold at ~20 call sites.

   Acceptance: the running build made **zero outbound TCP connections in 15.5 hours** of
   uptime, with zero errors and zero panics. 6404 app tests and 46 `warp_core` tests pass;
   check, clippy, and format are clean.

3d. **The referral system — DONE.** ~2,100 lines. Referrals unlocked two bonus themes by
   inviting other people to Warp, and every part needed an account: the status model queried
   `ReferralsClient` on startup, the reward modal fired when the server confirmed an unlock,
   and the settings page showed the invite link. With no account nothing can unlock.

   Phase 2 hid the Referrals *settings page* and its palette command, but left five entry
   points: an "Invite a friend to Warp" button in the resource center, "Invite a friend" in the
   user menu, an "Earn rewards" widget on the settings main page, and an "Invite People..."
   palette binding.

   **The palette binding carried `CustomAction::ReferAFriend` — the Phase 2 crash waiting to
   happen again.** Removing a binding while `app_menus.rs` still lists an item with the same
   action leaves `default_name` with nothing to find, and its `debug_assert!` kills the app
   about twenty seconds into launch. The binding, the menu item, and the `CustomAction` variant
   have to go together. **Whenever a gated or deleted binding has a `CustomAction`, grep
   `app_menus.rs` for that action in the same change.**

   The two referral themes are filtered out of the theme chooser unconditionally, which is what
   the referral-status check already produced with no server. `ThemeKind::SentReferralReward`
   and its serde alias stay, so an existing settings file that names one still parses.

   Acceptance: 6399 app tests pass; check, clippy, format, and the `simplewarp` binary clean.

3e. **Every warp-server request fails locally — DONE.** The seam is not the 381 call sites that
   reach for a client, nor the ~20 client traits: it is the 22 transport primitives they all
   funnel through — `send_graphql_request`, the public-API post/put/delete/patch helpers, the
   three agent-event SSE streams, `server_time`, `fetch_channel_versions`, `transcribe`, the AI
   suggestion calls, and four methods in `harness_support`, `block`, and `ai` that built their
   own requests instead of going through a primitive.

   Each returns an immediate local error and never constructs a request. No path under
   `app/src/server/` reaches `http_client()` any more, except telemetry, which posts to
   Rudderstack rather than warp-server and is its own step.

   This is the counterpart to 3c. That change made traffic impossible; this one makes it
   legible — a caller gets "SimpleWarp is a local-only build" at once instead of waiting on a
   DNS failure. It applies to **every** feature set, not only `simplewarp`: this fork has no
   reason to keep a build that can talk to Warp, and cfg-gating the primitives would be work to
   undo when the cloud modules go.

   Dead transport made the layer above it provably dead, and the compiler listed it:
   `error_from_response` and its 5 tests, `ambient_agent_headers` and
   `ambient_agent_headers_for_task`, the server-time cache, `AgentTipShownAnalyticsRequest`,
   `TimeResponse`, and the at-capacity error code. **The cascade stops at `base_client`**;
   taking that out means taking `warp_server_client` with it, which is step 4.

   Acceptance: 6393 app tests pass, including the mock-server tests in `ai_tests` and
   `presigned_upload_tests`, which exercise request building and response parsing directly
   rather than through the primitives.

3f. **The request-building code behind the client traits — DONE.** ~5,300 lines. With the
   primitives failing, the 165 methods in the ten `impl ... for ServerApi` blocks were building
   GraphQL operations and HTTP requests only to hand them to a function that returns an error.
   Each now returns that error directly.

   **The trait surfaces stay**, because 381 call sites are typed against them, and so do the
   types in their signatures — which is why `warp_graphql` survives this step and goes in step
   4. Deleting the bodies made a second layer provably dead: the seven public-API helpers on
   `ServerApi`, 21 request/response structs, 19 URL builders and GraphQL converters,
   `app/src/server/graphql/schema/` and `server_api/download.rs` (both left holding nothing but
   imports), and 24 tests of URL construction and response deserialisation.

   Three tooling lessons, each of which cost time:

   | Symptom | Cause |
   | --- | --- |
   | `cargo fix --lib` dropped `ServerIdAndType` from the `server::ids` re-export | No *library* code used it; seven test files did. **Verify any auto-fix pass with `--all-targets`, not `--lib`.** |
   | A `#[tracing::instrument(fields(?task_state, …))]` attribute stopped compiling | `cargo fix --broken-code` renamed the unused parameters it named. The stubbed method has nothing left to trace, so the fields went rather than the rename being reverted. |
   | A block of `E0614 cannot be dereferenced` in `ai_tests` looked like damage to the `Artifact` enum | **One unresolved name in a `use {…}` list poisons every name in it**, so `Artifact` stopped resolving and its match bindings took error types. Trimming the import list fixed all of them; `git diff` on the enum was the check that ruled out real damage. |

   Acceptance: 6369 app tests pass (24 fewer — the deleted builders' own tests); check across
   the workspace, clippy, format, and the `simplewarp` binary clean.

3g. **The resource center and the changelog — DONE.** Both were server-fed: `ChangelogModel`
   fetched release notes through `ServerApiProvider` and the resource center rendered them, so
   with no server neither could ever show anything. Gone with them: the changelog user
   preference, the `changelog` and `oz_changelog_updates` cargo features and their two feature
   flags, the App-menu items, the two bindings and two `CustomAction`s, the `/changelog` slash
   command, the `surface.resource_center.toggle` warpctrl action, and the "Latest updates"
   section of the agent-view zero state, which read the same model.

   `Tip`, `TipHint`, `TipAction`, and `TipsCompleted` moved to `app/src/tips`, beside the
   `WelcomeTipFeature` they belong with — a TODO in that file asked for exactly this move.

   **A file's name is not evidence of what reads it.** `app/src/command_palette.rs` held
   `PRIORITIZED_KEYBINDINGS`, whose doc comment says it orders the top of the command palette.
   Its only reader was the resource center's keybindings page, so the file went and the palette
   is untouched. `git grep` on the constant, not on the file name, is what settled it.

   **Two settings outlived the section they controlled**, so the Warp Agent page kept a toggle
   that changed nothing: `should_show_oz_updates_in_zero_state` and `should_expand_oz_updates`,
   with the toggle, its binding pair, and a `Show_Oz_Updates_In_Zero_State` context flag that
   was set but never read. Deleting a UI section means auditing the settings that fed it —
   the compiler is happy to keep a live toggle wired to nothing.

   Three test fixes, two of them older breakage that this step was the first to expose, because
   it was the first change since to compile those crates:

   | Test | Fix |
   | --- | --- |
   | `crates/integration` did not build at all | `SettingsSection::Referrals` went with 3d. The fixture DB still names that page, so the test now asserts the `load_pane_contents` fallback: an unknown persisted page decodes to the enum default instead of failing the restore. |
   | `warp_cli` asserted the `resource-center` completion group **exists** | It now asserts it is absent, matching how `history` and `share-to-team` are already pinned. |
   | `local_control` pinned 84 retained actions | 83, and `surface.resource_center.toggle` joins the names that must no longer deserialize. |

   Acceptance: 6364 app tests and 264 across `warp_cli`, `warp_features`, and `local_control`
   pass. `cargo check --all-targets -p warp -p integration`, clippy, format, and the
   `simplewarp` binary are clean. Not re-run in the app.

   `crates/channel_versions` still has an `oz_updates` field. It mirrors the shape of a remote
   JSON file, so an unread field there is not dead code in the same sense; it stays.

3h. **`app/src/auth/`'s "11 dead login-UI files" survey was wrong — no deletion this round.**
   A prior survey this session split the 17-file, 6.9k-line directory into ~11 files
   (`auth_view_modal.rs`, `auth_view_body.rs`, `auth_view_shared_helpers.rs`,
   `login_slide.rs`+tests, `login_error_modal.rs`, `login_failure_notification.rs`,
   `needs_sso_link_view.rs`, `paste_auth_token_modal.rs`, `auth_override_warning_modal.rs`,
   `auth_override_warning_body.rs`, `web_handoff.rs`) claimed dead because `root_view.rs`'s
   `AuthOnboardingState` startup logic deterministically lands on `Terminal(...)` in this build
   (`SkipFirebaseAnonymousUser` is on, `ForceLogin`/`AccountFirstOnboarding`/`AgentOnboarding`
   are off — verified independently, that part holds). **The survey conflated "the initial
   state is always `Terminal`" with "no other state is ever reachable."** It is not: logging
   out is a runtime *transition*, not startup logic, and it is live.

   Settings → Account unconditionally renders a `LogoutWidget` (`settings_view/main_page.rs`,
   no flag or auth-state gate) whose button dispatches `WorkspaceAction::LogOut` →
   `app:maybe_log_out` → `auth::maybe_log_out` → `auth::log_out` (both explicitly load-bearing,
   not touched) → the `"root_view:log_out"` action → `AuthOnboardingState::log_out()`, whose
   `Terminal(workspace) => { .. *self = AuthOnboardingState::Auth(..) }` arm fires from exactly
   the state this build is always in. `RootView::render()` then shows
   `ChildView::new(&self.auth_view)` — the supposedly-dead `AuthView` from `auth_view_modal.rs`.
   `is_anonymous_or_logged_out()` is unconditionally `true` in this build (credentials never
   populate), which also permanently disables the *native menu's* "Log out" item — that false
   lead is almost certainly what the original survey trusted instead of grepping the Settings
   page.

   That makes `auth_view_modal.rs`, `auth_view_body.rs`, and `auth_view_shared_helpers.rs`
   (survey step 1) live. `login_failure_notification.rs` (survey step 3) is live with them —
   `auth_view_modal.rs` calls `login_failure_notification::render` directly.
   `auth_override_warning_modal.rs`/`auth_override_warning_body.rs` (survey step 5) are
   independently live too: `workspace/view.rs` unconditionally constructs
   `auth_override_warning_modal: ViewHandle<AuthOverrideWarningModal>`, keeps it permanently in
   the render stack, and opens it on the real `AuthManagerEvent::LoginOverrideDetected` from the
   load-bearing `AuthManager` — a second, independent live wiring, not just `root_view.rs`'s own
   `ConfirmIncomingAuth` state.

   `login_slide.rs`(+tests), `needs_sso_link_view.rs`, `paste_auth_token_modal.rs`,
   `login_error_modal.rs`, and `web_handoff.rs` are referenced only from `root_view.rs`, in
   `AuthOnboardingState` variants (`Onboarding`, `LoginSlide`, `PostAuthOnboarding`,
   `NeedsSsoLink`, `WebImport`) that `AuthOnboardingState::log_out()`'s match arms do not reach
   from `Terminal`, and the one other entry point found (`debug_enter_onboarding_state`) is
   gated on `ChannelState::enable_debug_features()` *and* the confirmed-off `AgentOnboarding`
   flag — plausibly still dead. Not deleted this round regardless: `root_view.rs` is one
   ~7,000-line file with dozens of match arms across all of `AuthOnboardingState`, deleting
   these files requires editing it in the same commit either way, and the confidence gap after
   getting six of eleven files wrong on a first pass was too large to spend the remainder of
   the round re-verifying the other five to the same standard.

   **Net effect: `app/src/auth/`'s UI files are not a quick deletion.** They read as dead from
   the initial-state logic alone but are live through the logout transition, exactly the kind
   of design decision item 3's framing ("removing auth is deciding what the app means when
   there is no user at all, at every call site") already named. Folds into the existing
   `AuthStateProvider` 124-file fan-out deferral below, not a separate quick win.

   No commits made. `cargo nextest`/`clippy`/`check`/`format` not run — no code changed.

3i. **The 5 files 3h left unverified turned out to be a much bigger, single finding — the
   entire post-login onboarding flow, not 3-5 small files.** Traced every construction site of
   the 5 candidates:

   - `needs_sso_link_view.rs`'s `NeedsSsoLinkView` and `AuthOnboardingState::NeedsSsoLink` are
     **unconditionally dead in every build this fork produces**, independent of any feature
     flag. Their only entry point, `RootView::show_needs_sso_link_view`, fires exclusively from
     `handle_auth_manager_event`'s `AuthManagerEvent::AuthComplete` arm, and that event has
     exactly one emission site in the whole workspace: `auth_manager.rs:555`, inside the `Ok`
     branch of `on_user_fetched`, itself only reached via `fetch_user`'s round-trip to
     warp-server/Firebase. Step 3e's `local_only_error()` stub has no `cfg` gate — its own
     comment says so ("It applies to every feature set, not only `simplewarp`") — so that
     round-trip can never succeed anywhere in this fork. `AuthComplete` cannot fire, full stop.
   - `login_slide.rs`/`login_slide_tests.rs` (`LoginSlideView`,
     `AuthOnboardingState::LoginSlide`) and `paste_auth_token_modal.rs`
     (`PasteAuthTokenModalView`) are dead by the **same** `AuthComplete`-never-fires argument
     (via `begin_account_first_post_auth_refresh`), and *independently* dead a second way:
     their only other construction sites are inside `handle_agent_onboarding_event`, which only
     runs as a subscriber callback on `AgentOnboardingView` — a view that is itself only ever
     created by `RootView::create_agent_onboarding_view`, called only when entering
     `AuthOnboardingState::Onboarding`. Every path into `Onboarding` (`RootView::new`'s startup
     branch, `try_open_onboarding_slides`, `debug_enter_onboarding_state`) requires
     `FeatureFlag::AgentOnboarding.is_enabled()`, and `agent_onboarding` is not in the
     `simplewarp` cargo feature set (confirmed again here, matching Phase 1's original finding).
   - `login_error_modal.rs` and `web_handoff.rs` stay, as 3h already found: `LoginErrorModal` is
     still imported by `web_handoff.rs`, which is `#[cfg(target_family = "wasm")]` — a real,
     CI-checked target (`ci.yml`'s `--target wasm32-unknown-unknown` job) for a different
     product line this plan's scope never covers. Deleting it changes nothing about the
     `simplewarp` binary (already zero bytes there) and risks breaking an unrelated build this
     round has no way to verify.

   **Why this isn't a quick 3-file deletion.** `handle_agent_onboarding_event` is one ~380-line
   function matched over `AgentOnboardingEvent`, and several of its arms exist *only* to
   construct `LoginSlide`/`PostAuthOnboarding` state (`PrivacySettingsFromTerminalThemeSlideRequested`,
   `LoginFromWelcomeRequested`, the `requires_login` branch of `OnboardingCompleted`,
   `UpgradePasteTokenFromClipboardRequested`). Removing just those pieces while leaving the rest
   of the function (and `Onboarding`/`AgentOnboardingView` themselves) standing means either
   deleting match arms Rust's exhaustiveness check still requires (won't compile) or replacing
   them with placeholder no-op bodies inside a function that is *itself* already fully
   unreachable — exactly the "half-dead, deletion in disguise" shape 4j's
   `SelectionCursorRenderLocation` finding warned against, not a real fix. The honest boundary
   is: `Onboarding`, `PostAuthOnboarding`, `LoginSlide`, `NeedsSsoLink`, the account-first
   cluster (`AccountFirstLoginContext`, `AccountFirstCompletion`, `complete_account_first`,
   `resolve_account_first_post_auth`, `handle_account_first_workspaces_event`,
   `account_first_offer_experiment_arm`, `handle_login_slide_event`), `AgentOnboardingView`,
   `create_agent_onboarding_view`, `debug_enter_onboarding_state`, and the `ai/onboarding.rs`
   support module behind them (`build_onboarding_models`, `current_onboarding_auth_state`,
   `onboarding_credit_packs`, `onboarding_pricing_promotion_message`, the theme picker) all have
   to go together, in one round — a refactor the size of `drive/`'s multi-round work, not a
   follow-on to 3h.

   No commits made this round either. `cargo nextest`/`clippy`/`check`/`format` not run — no
   code changed. A future round should trace `AgentOnboardingView` and `ai/onboarding.rs` to the
   same standard before touching `root_view.rs`, the same lesson 3h paid for by getting 6 of 11
   files wrong on a first pass.

3j. **`needs_sso_link_view.rs` — DONE, the one piece of 3i that didn't need the channel
   question answered first.** Every other 3i finding (`LoginSlide`, `PostAuthOnboarding`,
   `PasteAuthTokenModal`, the account-first cluster) is entangled with `Onboarding`, whose
   reachability depends on `FeatureFlag::AgentOnboarding` — off for `simplewarp` but still in
   the `default` cargo feature set, so `warp-oss`/`stable`/`dev`/`preview` (built from the same
   shared `root_view.rs`) can still reach it. Touching those means deciding whether this fork
   still cares about those channels' behavior, which 3i correctly declined to decide
   unilaterally. `NeedsSsoLink` has no such dependency: its only entry point,
   `RootView::show_needs_sso_link_view`, fires exclusively from `handle_auth_manager_event`'s
   `AuthManagerEvent::AuthComplete` arm, and — per 3i's trace — that event's one emission site
   in the whole workspace sits behind a warp-server/Firebase round-trip that 3e's unconditional
   `local_only_error()` stub kills in **every** feature set, `default` included. So `NeedsSsoLink`
   is dead the same way regardless of channel, with no cross-binary judgment call needed.

   Deleted: `app/src/auth/needs_sso_link_view.rs` (103 lines) and its `mod` declaration;
   `AuthOnboardingState::NeedsSsoLink`, `RootView::needs_sso_link_view` and its construction,
   `RootView::show_needs_sso_link_view`, `AuthOnboardingState::show_needs_sso_link_view`,
   `AuthOnboardingState::complete_sso_link`, and every match arm across `log_out`, `focus`,
   `render`, `show_web_handoff_view` (wasm), and `handle_auth_manager_event`'s `AuthComplete` arm
   that existed only to enter or exit that state. `handle_auth_manager_event`'s
   `resumed_sso_context`/`show_needs_sso_link_view` branch is gone with it; the remaining
   `account_first_context` chain is otherwise unchanged. `pending_account_first_sso_login` is
   left in place (still declared, still reset in a few places) since it belongs to the deferred
   account-first cluster and is never set to `Some` again now — an inert but harmless leftover,
   not touched further to avoid re-opening the channel question 3i deferred.

   `login_error_modal.rs`/`web_handoff.rs` stay, as 3h and 3i both found: wasm-only, a different
   product line this plan's scope never covers.

   Acceptance: `cargo check -p warp --lib --all-targets`, `cargo clippy -p warp --lib
   --all-targets`, `cargo check --no-default-features --features simplewarp --bin simplewarp`,
   and `cargo check -p warp --bin warp-oss` all clean (confirms the deletion holds across both
   the `simplewarp` and `default` feature sets, not just the one this plan targets).
   `./script/format --check` clean. `cargo nextest run -p warp --lib`: **5999 pass, 0 fail, 4
   skipped** — exactly one fewer than the prior 6000-pass baseline, matching the one test deleted
   (`test_show_needs_sso_link_view_blocks_pre_terminal_onboarding_states`, which existed solely
   to pin `show_needs_sso_link_view`'s three-states-converge-on-`NeedsSsoLink` behavior). Not
   re-run in the app — none of this was reachable UI to begin with.

4. The crates: `firebase`, `warp_server_client`, `warp_server_auth`, `graphql`,
   `cloud_object_*`, ~~`warp_multi_agent_client`~~.

   Steps 3e and 3f name the two conditions for starting this: the dead-code cascade stops at
   `base_client`, so `warp_server_client` is the first crate to go, and `warp_graphql` cannot
   go until the client traits themselves do, because their signatures still use its types.

   **`warp_multi_agent_client` is DONE.** 240 lines, one real call site
   (`app/src/ai/agent/api/impl.rs`), and the last path that sent an agent request — with the
   user's API keys inside it — to `{warp_server}/ai/multi-agent`.

   **Deleting it forces the `local_inference` seam closed, which is the point.** Phase 3 wired
   the local adapter behind `#[cfg(feature = "local_inference")]` and left the server path in
   the `not` branch. With the crate gone that branch cannot compile, so the default build would
   have no AI path at all — the adapter has to become unconditional. The cargo feature and its
   12 `cfg` sites go with it. Same reasoning as 3e: a seam kept for a path that no longer works
   is only work to undo later.

   The compiler named most of the cascade — `convert_multi_agent_client_error`, then
   `AIApiError::from_stream_error` (37 lines) whose only caller it was, already carrying a
   `cfg_attr(…, allow(dead_code))` that admitted as much. It did **not** name the rest:

   | Orphaned | Why nothing said so |
   | --- | --- |
   | `Workspace::is_byo_api_key_enabled` | `pub` items in a lib are exempt from dead-code analysis. Each had exactly one caller — the policy branch deleted from `UserWorkspaces`. Found by grepping for the call. |
   | `BillingMetadata::is_byo_endpoint_enabled` | Same. |

   That is the 3g lesson again, from the other direction: **deleting a branch orphans what only
   that branch called, and for a `pub` item the compiler is silent.**

   Two tests, neither a mechanical fix:

   - `test_has_any_ai_remaining_false_with_grok_subscription_but_byo_disabled` asserted a
     premise that can no longer occur. `..._true_with_grok_subscription_connected` already
     covers the reachable case, so inverting it would have produced a duplicate. Deleted.
   - `test_byo_api_key_disabled_for_anonymous_firebase_user` became
     `test_anonymous_user_with_byo_key_has_ai_available`. Inverting the assertion under the old
     name would have been nonsense; the new name pins the guarantee that is the point of the
     fork — no account plus a user key means working AI.

   Acceptance: 6363 app tests and 83 `local_inference` tests pass; check across the workspace,
   clippy, format, and the `simplewarp` binary clean; `warp_multi_agent_client` gone from
   `Cargo.lock`. **Not re-run in the app, and this is the step where that matters most** — the
   AI dispatch is what changed, so a real conversation is the check.

   `byo_endpoint_policy` stays on the tier struct, written by the GraphQL conversion and read by
   nothing. It goes with `graphql`.

   **`warp_server_client` next, and it is not a clean cut.** 17 files, 3,651 lines, 23 files
   referencing it. Its `base_client` had two users and one is now gone, but the crate is four
   separate things: `iap` (839 lines of identity-token minting), `auth` (845, the session and
   token layer that `app/src/auth/` is built on), `base_client` (417), and `network_logging`
   (164, which backs the in-app network log view). The `auth` half is entangled with step 3, so
   the tractable slice is `iap` plus `base_client`, not the crate.

4b. **The Identity-Aware Proxy layer — DONE.** ~2,100 lines across six crates. IAP fronted the
   *staging* warp-server, reachable only through `WarpServerConfig::iap_config`. **No channel
   config in this fork ever set it** — `local_only` and `production` both pass `None`, as do
   both integration binaries — so `IapState` could never be constructed and every consumer was
   permanently inert.

   `base_client` turned out not to be the tractable half after all: it is still `ServerApi`'s
   `Deref` target, so it goes with `auth`. `iap` alone was the clean cut, but the seam was much
   wider than the module:

   | Removed | Where |
   | --- | --- |
   | `iap.rs` + tests | The gcloud shell-out, the WIF self-mint (STS + IAM `generateIdToken`), the on-disk token cache, `IapManager`'s refresh lifecycle. |
   | `http_client::iap` | `IapTokenProvider`, `is_iap_challenge`, the `Proxy-Authorization` builder, and the `iap_token` parameter threaded through get/post/put/patch/delete. |
   | `AuthEvent::IapChallengeReceived`, `GraphQLError::IapChallengeBlocked` | The challenge → refresh feedback loop, from the response check down to `ServerApiProvider`'s event arm. |
   | The startup gate | `authenticate_user_after_iap_access` in `lib.rs` and its twin in the CLI's `launch_command`. Both now authenticate directly. |
   | `IapCredentialsWidget` | With `MainPageAction::RefreshIapCredentials`. |
   | The websocket handshake headers | Shared-session sharer and viewer attached the proxy-auth header on connect and refreshed on a handshake challenge; all three sites drop to plain `WebSocket::connect`. |
   | `ManagedSecretsIapMinter` | The app-side bridge letting a sandboxed Oz runner self-mint. |

   Deleting the transport orphaned code the compiler could not flag, in both directions.
   `wrap_eventsource_with_iap_detection` already had no callers — 3e took its streams.
   `http_client`'s `is_warp_server_origin` existed only to scope the IAP token to Warp's origin,
   and once its one caller went it was left holding nothing but its own two tests; same for
   `connect_error_http_response` in `websocket`, whose only user was `ws_connect_is_iap_challenge`.

   **Deleting a `use` can silently move its `#[cfg]` onto the next line.** Three removed imports
   carried `#[cfg(not(target_family = "wasm"))]`, and stripping the `use` alone left the
   attribute attached to whatever followed: `server::sync_queue`,
   `warpui::assets::asset_cache::AssetSource`, and — worst — `pub mod ids;` in
   `app/src/server/mod.rs`, which would have made the whole module non-wasm. **Every one still
   compiled on macOS**, so `cargo check` proved nothing; reading the diff line by line is what
   caught them. The 2/3d lesson in a new shape: the damage a scripted removal does is not always
   to the lines it touched.

   Acceptance: 6363 app tests pass, **unchanged** — none of this had a test in `warp`. 106 pass
   across `warp_server_client`, `http_client`, `warp_graphql`, `warp_core`, and `websocket`.
   Check across the workspace, clippy, format, and the `simplewarp` binary are clean. Six now-unused
   dependencies dropped from `warp_server_client`.

   Two things this did **not** verify. The wasm target is not installed, and the three orphaned
   `cfg`s were wasm-only hazards, so that build is argued correct rather than compiled. And
   `cargo nextest run -p integration` fails 5 SSH tests: they tunnel into Warp's private GCP
   project via `gcloud compute start-iap-tunnel` — an unrelated use of the name — and time out
   after 41s waiting for a password prompt. That path is untouched and cannot work in this fork.
   This is the first time the integration crate's *tests* have been run; 3g only checked that it
   builds.

4c. **The Billing and Usage page and the credits-purchase UI — DONE.** ~13,450 lines, the largest
   step after the TUI. Phase 2 hid the page behind `needs_warp_account()`; this deletes it and
   every surface whose only purpose was to reach it. Same shape as 3g.

   | Removed | What |
   | --- | --- |
   | `settings_view/billing_and_usage*` | 18 files, 10,685 lines: the v1 and v2 pages, the dispatch view choosing between them, the billing-cycle usage tables, the usage-history model, the spending-limit modals. |
   | `SettingsSection::BillingAndUsage` | Nav item, page handle, event plumbing, the `warp://settings/billing_and_usage` route, the palette binding. |
   | `terminal/buy_credits_banner.rs` | 1,068 lines, its overlay helpers and three input render paths. |
   | `terminal/enable_auto_reload_modal.rs` | 522 lines, plus the `OpenAutoReloadModal` chain through input → terminal view → pane group → workspace view. |
   | `/usage` | The slash command and `TerminalAction::OpenBillingAndUsagePane`. |
   | `FeatureFlag::BillingAndUsagePageV2`, `ui_components/tab_selector.rs` | The flag with its cargo feature; the tab strip nothing else used. |

   **Deleting a nav target changed what the sidebar *is*, so the nav tests could not be
   mechanically fixed.** Billing and Usage was the only plain page between two collapsed
   umbrellas, and four arrow-key tests were built on that shape. The sidebar is now
   `Account → Agents → Code → Cloud platform → Teams` with the three umbrellas adjacent, so each
   test was re-pointed at the stop that now holds the position under test — `CodeIndexing`, which
   maps back to the collapsed Code umbrella — rather than renamed. Same fix for the two
   `crates/integration` navigation tests.

   **`crates/integration` is not a default workspace member, so a root
   `cargo check --all-targets` skips it silently.** It reported clean while the crate had two
   hard errors. 3g hit this too. **Check it by name.**

   Two behaviour calls, both 3b:

   - `billing_and_usage_page_v2` was in `default` but **not** in `simplewarp`, so this fork's
     build always took the `else` branch of its two `teams_page` conditions. Collapsed to that,
     not to the `default` behaviour. **A flag's default-set membership is not its value in this
     build.**
   - The prompt alert's two overage states linked to the deleted page. The link goes, the
     explanatory text stays, so the alert still says why a request was blocked without offering
     an action that cannot work.

   The compiler's cascade was larger than the deletion: the whole banner-dismissed state machine
   in `request_usage_model`, `Dropdown::with_drop_shadow` and its field, two
   `CloudActionConfirmationDialogVariant` credits variants, `TeamActionConfirmationTarget::RemoveUser`,
   and `ConversationUsageView`'s `DisplayMode`, which collapsed to one variant and turned four
   render branches into straight-line code.

   **A deleted `use` moves its `#[cfg]`; a deleted field moves its `///` the same way.** The 4b
   scan ran again here and found no orphaned attributes — but it only matched `#[`, and three doc
   comments had drifted, including `/// The display mode for this view.` landing on `timing_info`.
   Scan for both.

   **A test that names a flag is not evidence the code under test consults it.**
   `..._true_with_self_serve_auto_reload_and_billing_v2_disabled` was byte-identical to the test
   above it; `request_usage_model.rs` never read `BillingAndUsagePageV2`, so the guard that
   appeared to distinguish them did nothing.

   Acceptance: 6306 app tests pass (57 fewer), all 22 `integration` settings tests pass, check
   across the workspace *and* `-p integration`, clippy, format, and the `simplewarp` binary
   clean. Not re-run in the app — the settings sidebar and the agent input overlays both changed.

   `AIRequestUsageModel` (44 files) and `PricingInfoModel` (20) stay: they are the `auth`-class
   question of what the app means with no request quota at all, not a deletion.

4d. **Every AI request is allowed, and none are metered — DONE.** ~2,100 lines. The question 4c
   left open is answered: **all requests go through, nothing is counted against an allowance.**

   `has_any_ai_remaining` was the single gate. It asked warp-server whether the account had
   credit, then fell back to a local derivation over base quota, bonus grants, overages,
   pay-as-you-go, auto-reload and BYO keys. It returns `true`.

   That collapsed a state machine. `PromptAlertState` had eight variants — six ways to be out of
   credit plus the anonymous soft gate. Two survive: `NoConnection | NoAlert`. With no state
   offering an action, `PromptAlertAction` went and `PromptAlertEvent` became **uninhabited**,
   which the compiler then walked *upward*: both wrapper variants
   (`AgentInputFooterEvent::PromptAlert`, `UniversalDeveloperInputButtonBarEvent::PromptAlert`),
   their forwarding subscriptions, and `Input::handle_prompt_alert` are unreachable once the
   payload cannot exist. **An uninhabited event type is a strong deletion tool: it makes every
   handler along the chain provably dead.**

   Also gone: `credit_availability.rs` and the `AICreditAvailability` GraphQL round trip,
   `ServerAvailabilityState` with its refresh/reset lifecycle, the `ai_credit_availability`
   field on the workspaces-metadata response, `AIClient::get_ai_credit_availability`, and the
   prompt-suggestion banner's disabled tooltip and out-of-credits modal path.

   **A quota error can still arrive — from the provider.** `RenderableAIError::QuotaLimit` stays,
   but it used to offer a Warp subscribe CTA and, with no message, invent "your credit limit
   resets on {date}" from `next_refresh_time`. It now shows what the provider said. A 429 from an
   OpenAI-compatible endpoint is the only way it can fire.

   Two deliberate boundaries. Bonus grants survive as *data* — they gate nothing, but deleting
   the type means touching the workspace billing GraphQL conversion, which belongs with the
   `workspaces` refactor. And the onboarding credit-purchase slide keeps its handler in
   `crates/onboarding`; only the subscription that could reach it is removed.

   33 tests went with their subject and 4 replaced them. The 30 `test_has_any_ai_remaining_*`
   tests each described a way to *earn* the right to make a request; one test now pins the
   guarantee instead. `prompt_alert_tests`' seven availability-to-alert mappings became three
   tests saying no account state raises an alert and offline is the only blocker.

   **A deleted variant strands its doc comment onto the next one** — the 4c lesson again, caught
   by the same scan, twice. And `cargo fix -p warp --all-targets` *failed* here where `--lib`
   succeeded: one more reason not to trust an auto-fix pass without re-checking.

   Acceptance: 6268 app tests pass (38 fewer); check across the workspace *and* `-p integration`,
   clippy, format, and the `simplewarp` binary clean. Not re-run in the app.

4e. **Session sharing is gone — DONE.** ~41,000 lines, 61 files. The largest single deletion
   after the TUI, and the first one whose *consumers* outnumbered the feature: the module tree
   was 27k lines, the call sites another 14k.

   Removed: `terminal/shared_session/` (sharer, viewer, network, heartbeat, presence,
   permissions, selections, the share/role-change modals) and `terminal/view/shared_session/`,
   `share_block_modal`, `crates/warp_terminal/src/shared_session.rs`, `ShareableObject::Session`
   with the Warp Drive QR code, the tab and pane-header share menus, and the `WorkspaceAction`
   and `pane_group::Event` variants that reached any of them.

   **A mock is not a drop-in for the manager it replaces.** Cloud-mode panes were composed in an
   *uninitialized shared-session viewer* — that was the trick that let the composer reuse the
   terminal input with no backing session. Swapping in `MockTerminalManager` looked equivalent
   and was not: the viewer's `TerminalView` was built with `is_ambient_agent: true`, so the mock
   silently produced cloud panes with **no `AmbientAgentViewModel`**. Nothing failed to compile;
   three tests caught it at runtime. `MockTerminalManager::create_model` now takes the flag.

   **An "unused import" can be used by the test module below it.** `cargo check --lib --tests`
   reported `SerializedBlock`, `ShellName`, and `AuthStateProvider` as unused; each is reached by
   a `#[cfg(test)] mod tests` child through `use super::*`, so removing them broke a build that
   the same command had just called clean. `cargo test --no-run` is the check that sees them.
   `ShellName` is the honest case of the three: its only user is a `cfg(any(test, feature =
   "test-util"))` constructor, so the import now carries the same `cfg`.

   **A deleted dispatch branch fails at runtime, not at the compiler.** `is_viewing_shared_session`
   survives as a field on `AIConversation`, so every test that *constructed* a viewer child still
   compiled — and then materialized an ordinary pane, because the branch that read the field was
   gone. Two hidden-child tests failed this way. A leftover boolean is worse than a deleted one:
   it keeps the old shape compiling while the behaviour underneath it has changed.

   244 tests went with their subject; none needed replacing, because every one of them asserted
   on a sharer, a viewer, or a link. Eleven were kept by removing only the shared-session framing:
   a composer-selector pair now gates on `is_dummy_cloud_mode_session` alone, the DCS-hook test
   keeps the rejection half and drops the viewer half, and `unfreeze_agent_input` no longer needs
   two statuses to say the same thing once. The close-session confirmation dialog's seven tests
   all went: the dialog existed to warn before closing a *shared* pane, and it is now unreachable.

   Acceptance: 6024 app tests pass (244 fewer), 0 fail. Format, clippy (no import or error
   diagnostics), the `simplewarp` binary, and `-p integration` clean. Not re-run in the app.

   **What is deliberately left standing** is now visible as ~85 clippy `never used` warnings, and
   that list *is* the next step's work: two stubs that only log or return `false`
   (`extend_shared_session_retention`, `is_third_party_cloud_agent_viewer`), the shared-session
   scrollback loaders in `terminal/model/blocks.rs`, the close-session confirmation dialog, and
   the four `*SharedSessions*` `FeatureFlag` variants with their cargo features.

4f. **The dead code 4e left behind is cleared — DONE.** ~1,300 lines. Clippy's `never used` list
   was the whole work list; it went from 120 diagnostics to 52.

   The close-session confirmation dialog is the interesting one, because *nothing about it looked
   like session sharing*. It is a tab-close warning with a user setting
   (`should_confirm_close_session`), a features-page row, and an `OpenDialogSource` threaded
   through `close_tabs` and the local-control close handler. All of it existed for one sentence:
   "You are about to close a session that is currently being shared." With no shared pane it can
   never open, so the dialog, both settings (`should_confirm_shared_session_edit_access` had no
   reader at all), the row, the parameter, and `Workspace::close_pane` — reachable only from the
   dialog's confirm branch — all went together.

   **`is_viewing_shared_session` is NOT dead, and 4e was wrong to list it.** Three production
   writers still set it, all in cloud-transcript restore: the flag now means "this conversation
   is a passive view of a remote run", which is exactly what a restored ambient transcript is.
   Deleting it would make those transcripts locally drivable. It is live plumbing under a stale
   name — a rename, not a deletion, and not in this phase.

   **`never used` is a claim about one target, and rustc's dead-code pass is transitive.** Acting
   on the `--lib` list broke the build in two distinct ways. Four items (`restore_fired_row`,
   `IdleTimeoutSender::refresh`, `reset_unknown`,
   `restore_cloud_followup_input_after_upload_failure`) are used *only by tests*, which the lib
   target does not see. And `tear_down_active_setup_command_group` has a real caller — one that
   is itself dead, so rustc reported the pair and deleting the leaf broke the root. Both restored.
   **Delete a dead cluster from the top down, and check `grep -rl` for test users first.**

   Left for later, and not session sharing's: the warp-server residue in `server/block.rs`,
   `attachment_utils.rs`, and `generate_block_title/`. **Cleared in 4i.**

4g. **The viewer-mode ancestor SSE is gone — DONE.** ~1,200 lines. The streamer ran a second,
   parallel event path purely for a shared-session viewer watching someone else's orchestrator:
   one ancestor SSE per `parent_task_id`, a consumer refcount so several viewer panes could
   share it, a REST cold-start seed, and its own drain and reconnect timers. Its only entry
   point was a viewer pane calling `register_viewer_mode_consumer`, so after 4e nothing but its
   own tests reached it — a cluster kept alive solely by the tests written for it.

   **A two-valued mode enum is a deletion in disguise.** `FamilyDrainMode` existed to say which
   of the two paths a drain was serving. With `Observer` gone, `Primary` is the only answer, so
   the enum, the three signatures that threaded it, and the two `if mode == Primary` guards all
   collapse — and what is left reads as one plain drain rather than a configurable one.

   Acceptance: 6023 app tests pass (one fewer — the continuation-pane test went with its
   subject), 0 fail. Format, clippy (0 errors, 52 dead-code warnings), and all three binaries —
   `simplewarp`, `warp-oss`, and `-p integration` — clean. Not re-run in the app.
4h. **The agent-profiles-page usage widget is gone — DONE.** ~330 lines. `UsageWidget`
   rendered the used/limit AI-credit count and an "Upgrade" / "Compare plans" / "Contact
   support" CTA, gated on `FeatureFlag::UsageBasedPricing`. 4d already made
   `has_any_ai_remaining` always `true`, so the count had nothing left to warn about and
   the CTA had nothing to sell — this closes that gap on the one page 4c and 4d did not
   reach. `AgentProfilesPageAction::AttemptLoginGatedUpgrade`, the click target for a
   logged-out user, went with it.

   `on_page_selected`'s refresh of `AIRequestUsageModel` stays: the page's model dropdowns
   are a separate consumer of request-usage events, so the widget was not the only reader.

   Found as an uncommitted, already-written diff at the start of this session — verified
   rather than authored. **Nextest showed 8 failures unrelated to this file**
   (`settings::cloud_preferences_syncer`, six tests; `workspace::view::tests`, two Warp
   Drive/signup tests), reproduced identically on a clean stash of master with no code
   change at all, so they predate this step and are not counted against it.

   Acceptance: `cargo check --no-default-features --features simplewarp --bin simplewarp`,
   `cargo check -p warp --bin warp-oss`, `cargo check --all-targets` (simplewarp), clippy,
   and format all clean. 6007/6015 app tests pass (8 pre-existing failures, see above). Not
   re-run in the app.

   **Follow-on found by clippy after the fact:** `AIRequestUsageModel::refresh_duration_to_string`
   had exactly one caller, the widget's "This is the {weekly/monthly/biweekly} limit..."
   description, and clippy only reported it dead once that caller was gone. Deleted
   separately once found. `RequestLimitRefreshDuration` itself stays live elsewhere
   (`settings/ai_tests.rs`, `ai_assistant/mod.rs`).

4i. **The 4f-flagged warp-server residue is gone — DONE.** ~375 lines across four files.
   `generate_block_title/` and `BlockClient::{save_block, generate_shared_block_title}` —
   both stubbed with `local_only_error()` since 3f, and the request/response types they
   built were the only thing keeping the module alive. `server/block.rs`'s
   `/share_block`-embed renderer (`Block::new`, `native_prompt_for_server`,
   `embed_pixel_height`, `embed_pixel_width`, eight pixel constants) went with its one
   caller, `save_block`; `DisplaySetting` and its `GqlDisplaySetting` conversion went once
   nothing built one. `attachment_utils`'s server-side download path
   (`sanitize_filename`, `DownloadedAttachment`, `build_file_attachment_map`,
   `download_file`) had no caller left; `attachments_download_dir` and
   `MAX_ATTACHMENT_SIZE_BYTES`, the `local_inference` path's own functions, stay.

   **One orphan the compiler couldn't see, same shape as the 4 `pub`-item trap.**
   `Block::full_content_height_with_display_options` in `terminal/model/block.rs` is
   `pub`, so dead-code analysis skips it — but it had zero callers anywhere in the
   workspace. Its own doc comment named why: "used ... when sharing a block," and the
   share-block modal went in 4e. **Grep the type a deleted signature took, not just the
   signature's own crate** — `DisplaySetting` led here, not the `never used` list.

   `Block` (the struct), its `TryFrom<GqlBlock>`, and `BlockClient::{unshare_block,
   blocks_owned_by_user}` stay: `show_blocks_view.rs` still calls them, behind an account
   check Phase 2 made unreachable but did not delete.

   Acceptance: `cargo check --all-targets` (both feature sets, plus `-p integration`),
   clippy, and format clean. 6015 app tests: 6007 pass, the same 8 pre-existing failures
   as 4h, 0 new. Not re-run in the app — none of this was reachable UI to begin with.

4j. **A second clippy dead-code sweep — DONE.** `remote_server` (step 3) was looked at again
   this session and set aside a second time — it is still the `BufferSource`/`FileBackend`
   redesign, not a mechanical deletion, and stays for its own attended pass. In its place: a
   fresh `cargo clippy -p warp --lib --all-targets` (no `target/` existed; a clean build took
   2m36s) found a small new batch of dead code, almost all of it downstream of 4d, 4e, and 4g:

   | Removed | Why it was dead |
   | --- | --- |
   | `AgentManagementTelemetryEvent::{TombstoneArtifactClicked, TombstoneContinueLocally, TombstoneContinueInCloud}` | Telemetry for tombstone-view buttons nothing constructs; the sibling `DetailsPanelContinueLocally`/`SlashCommandContinueLocally` variants are still constructed elsewhere, so only these three tombstone-specific ones went. |
   | `BlocklistAIActionExecutor::terminal_model` field | Its own doc comment named the reason: "for checking session sharing state" (gone in 4e). The constructor still needs the parameter to build `ShellCommandExecutor`, which keeps its own copy — storing a second one on `self` was pointless. |
   | `OrchestrationEventStreamer::persist_cursor_local_only` | Doc comment named `FamilyDrainMode::Observer`, deleted in 4g. Zero callers anywhere, not even tests. |
   | `parent_task_id`/`run_id`/`status` fields on `ChildSpawned`/`ChildStatusChanged` | Both events are still emitted (real pill-bar broadcasts), but their sole subscriber (`controller.rs`) matches `{ .. }` and does nothing — its comment claimed `OrchestrationViewerModel` handles them, but that type no longer exists (removed with the ancestor SSE in 4g). Collapsed both to fieldless variants rather than deleting the broadcast itself, which is still real signal for a future consumer. |
   | `ShareSessionError::Internal` | Session sharing is gone (4e); no construction site anywhere, unlike `Failed`, which a test still constructs — that one stays, matching the 4f lesson about test-only users. |
   | `AIRequestUsageModel::requests_used` | Sits right below the 4d comment "SimpleWarp does not meter AI requests." Only its own tests called it; the four `assert_eq!` lines were removed, not the surrounding tests, which also cover `request_limit()`/`requests_remaining()` (both still live). |
   | `make_ambient_task_with_task_id` | Test helper, zero callers even in its own test file. |
   | `Modal::{set_header_icon, set_header_icon_color}` | Pre-existing dead API, unrelated to the cloud strip (last touched by the 2024-edition migration) — never called, so `header_icon`/`header_icon_color` are permanently `None`. |
   | `window_id` field on the AI block struct (`ai/blocklist/block.rs`) | Set at construction, never read — every real use in the file calls `ctx.window_id()` fresh instead of `self.window_id`. |
   | `SelectionCursorRenderLocation` (`Start`/`End`/`None`), `grid_renderer::render_selection_cursor`, `SELECTION_CURSOR_TOP_DIAMETER` | `git log` on the enum's file lands on **`b79e80a4` "Phase 4: delete session sharing"** — `Start`/`End` rendered a remote collaborator's selection-cursor edge; the sole call site always passes `None` now. Collapsed all the way: the enum, the now-single-behavior match arm, the parameter, and the renderer function it exclusively drove all went, not just the two dead variants — a single-variant enum plus an always-taken `_ => ()` arm is the same "deletion in disguise" shape as `FamilyDrainMode` in 4g. |

   **Explicitly left alone, both already-settled decisions from 4f:** `restore_fired_row`,
   `reset_unknown`, `restore_cloud_followup_input_after_upload_failure`,
   `tear_down_active_setup_command_group`, and `IdleTimeoutSender::refresh` reappeared in this
   clippy run under the same names — re-checked, still exactly the 4f finding (test-only
   callers, or a caller that is itself dead). A related, larger cluster around
   `tear_down_active_setup_command_group`'s real caller (`TerminalView::tear_down_cloud_mode_setup_phase`
   and six sibling methods — queued-command draining, ambient setup sync, wasm detail-panel
   checks) surfaced for the first time in this run too, once a broader warning-text pattern was
   used. Left standing, same reasoning as 4f: the caller is itself dead, so this is a
   cloud-mode/ambient-agent-lifecycle question needing its own pass, not a clippy-driven cleanup.

   Acceptance: `cargo clippy -p warp --lib --all-targets` dropped from 21+18 to 7+2 warnings
   (all of it the deferred `tear_down_cloud_mode_setup_phase` cluster and pre-existing style
   lints). `cargo check --all-targets` (workspace and `-p integration`), `--no-default-features
   --features simplewarp --bin simplewarp`, and `-p warp --bin warp-oss` all clean.
   `./script/format --check` clean. `cargo nextest run -p warp --lib`: **6015 pass, 0 fail** —
   better than 4h/4i's recorded 6007/6015 baseline; the 8 tests noted there as pre-existing
   failures did not reproduce this run. Not re-run in the app — none of this was reachable UI.

5. The `FeatureFlag` variants that are no longer in use. **29 removed: 16 in the first sweep, 2
   with step 3g, and 11 in a second sweep. More remain behind the module deletions above.**

   Of 292 variants, 16 had no `FeatureFlag::X` reference anywhere outside their own declaration:
   `WelcomeTips`, `ThinStrokes`, `WelcomeBlock`, `CloudObjects`,
   `FetchChannelVersionsFromWarpServer`, `ContextChips`, `FetchGenericStringObjects`,
   `IntegratedGPU`, `AgentPredict`, `LazySceneBuilding`, `AIBlockOverflowMenu`,
   `AIGeneratedOnboardingSuggestions`, `AIMemories`, `MarkdownImages`, `CloudModeHostSelector`,
   `PricingTransparency`.

   Three of them — `LazySceneBuilding`, `MarkdownImages`, `PricingTransparency` — were listed in
   `DOGFOOD_FLAGS`, so a dogfood build turned them **on**. That changed nothing, because no code
   ever asked whether they were enabled. A flag that is switched but never read is the most
   misleading kind of dead code: the flag list reads as a feature inventory.

   Searching for the bare variant name is not enough to prove one is dead — `WelcomeTips` and
   `CloudObjects` both look used, but the hits are an unrelated `ToggleWelcomeTips` action, a
   `WelcomeTipsViewState` type, and `CloudObjects::Listener` inside log strings. Match on
   `FeatureFlag::X`.

   **Second sweep: 11 more of the remaining 274.** `LogExpensiveFramesInSentry`,
   `DefaultWaterfallMode`, `AgentModePrimaryXML`, `AgentModePrePlanXML`, `GrepTool`,
   `FileRetrievalTools`, `ReloadStaleConversationFiles`, `RetryTruncatedCodeResponses`,
   `PRCommentsSkill`, `CodeModeChip`, `SimulateGithubUnauthed`.

   **Check the cargo feature as well as the flag.** Each of these had a cargo feature whose only
   `cfg(feature = …)` site in the workspace was the mapping in `app/src/features.rs`, so the
   feature existed only to switch a flag with no reader. Seven were in the `default` set and six
   in `simplewarp`. Where a cargo feature has other `cfg` sites it is live even when its flag is
   dead, so the two have to be counted separately.

   **Four of them read as an inventory of live AI behaviour and are not.** "Allows AI to call
   the grep tool", "Allows AI to call the file retrieval tools", and the two XML
   system-prompt flags describe what the agent does today regardless: tool availability comes
   from `get_supported_tools`, which gates on other flags. `git log -S "FeatureFlag::GrepTool"`
   dates the last change to the initial public release, so these were dead upstream rather than
   casualties of 3e and 3f. A doc comment is a claim about the past.

   `FLAG_STATES` and `USER_PREFERENCE_MAP` are sized by `cardinality::<FeatureFlag>()`, so a
   sweep leaves no count to keep in step.

   **Third sweep: 2 more of the remaining 259.** `BuildPlanAutoReloadBannerToggle` and
   `BuildPlanAutoReloadPostPurchaseModal` — no `FeatureFlag::X` reference and no cargo
   feature mapping. Both named the Build Plan auto-reload experiment; its banner and modal
   (`terminal/buy_credits_banner.rs`, `enable_auto_reload_modal.rs`) went in 4c, which is
   almost certainly why these two survived that step's own compiler-led cascade — nothing
   *read* them, so nothing failed to compile when their UI went. 257 variants remain.

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
- [ ] Phase 4: the cloud crates and the TUI are removed.
      - [x] **The TUI is fully gone** (1, 1b, 1c): the front-end, its rendering engine, and every
            surface marker — ~120k lines and the `ratatui` dependency.
      - [x] **The features that only a server could answer are gone**: billing (2), experiments
            (3b), referrals (3d), the resource center and changelog (3g) — and with 3c, 3e, and
            3f the app can no longer build, let alone send, a warp-server request. ~9k further
            lines.
      - [x] **`warp_multi_agent_client` is gone** (4), and with it the `local_inference` cargo
            feature: the local adapter is now the only AI path in every build, not one side of a
            `cfg`.
      - [x] **The Identity-Aware Proxy layer is gone** (4b): ~2,100 lines of staging-only
            token minting across six crates, plus `IapConfig` itself, which no channel config
            in this fork ever set.
      - [x] **Billing, credits, and usage are gone from the UI** (4c): ~13,450 lines — the
            settings page, the buy-credits banner, the auto-reload modal, `/usage`, and every
            route and binding that reached them.
      - [x] **The request quota gates nothing** (4d): `has_any_ai_remaining` is `true`, the
            credit-availability layer is gone, and the prompt alert is down to offline-or-nothing.
            Every AI request is allowed and none are metered.
      - [x] **Session sharing is gone** (4e, 4f, 4g): ~43,500 lines — the sharer, the viewer, their
            network layers, every modal, the drive-object and tab-menu entry points, and the
            dead code they left behind. Cloud mode survives it, but only because the composer
            pane was given back the ambient model the viewer manager used to build.
      - [x] **The agent-profiles-page usage widget is gone** (4h): ~330 lines — the last
            credit-count-and-upgrade-CTA surface that 4c/4d did not reach.
      - [x] **The 4f-flagged warp-server residue is gone** (4i): ~375 lines — the
            `/share_block` embed renderer, `generate_block_title`, and the dead half of
            `attachment_utils`.
      - [x] **A second clippy sweep is clean** (4j): a small batch of 4d/4e/4g fallout —
            orphaned tombstone telemetry, a session-sharing-only field, an observer-mode
            cursor method, unread pill-bar event fields, one `ShareSessionError` variant, an
            unmetered usage getter, and the last session-sharing selection-cursor code
            (`SelectionCursorRenderLocation` and its renderer). `remote_server` (step 3) was
            looked at again and set aside a second time — still needs its own pass.
      - [x] **`drive` (2) is mostly done.** Nine rounds: the panel's own UI (settings page,
            onboarding block, command-palette search, `/prompts` menu, dead dispatch chains) is
            deleted; the load-bearing types (`SharingDialog`, `CloudFolder`, `WarpDriveItem`,
            `DriveObjectType`) are relocated into `cloud_object`/`sharing`; the deep-link handler
            and breadcrumb click surface fail fast instead of dead-ending; and object creation
            (`CreatePersonalFolder`/`CreateTeamNotebook`/etc.) no longer force-opens the tab to
            show its naming dialog. What's left — `panel.rs`/`index.rs`, `items/item.rs`, four
            dialogs, `drive_helpers.rs`, `cloud_object_styling.rs`, `drive/workflows/`,
            `import/`, `export/` — turned out load-bearing for local-object selection tracking,
            focus, and undo-trash, not just cloud UI, so it stays with `cloud_object` (3) as a
            refactor rather than a deletion.
      - [ ] **The other cloud crates remain** (4), and so do the three modules that are refactors
            rather than deletions: `remote_server`, `auth`, and `cloud_object` (3), now including
            the remainder of `drive`. The remaining dead `FeatureFlag` variants fall out of those
            (5), together with the ~85 items 4e left dead but standing.
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

### Disk

`target/` reached 23 GB and made the machine unusable. Measured on a *partial*
build — one edit-and-check cycle over the `warp` crate, plus clippy:

| `target/debug/` | Size |
| --- | --- |
| `incremental/` | 4.6 GB |
| `deps/` | 1.3 GB, of which 871 MB is `.rmeta` |
| `build/` | 699 MB |

**The incremental cache is the whole problem, so `[profile.dev] incremental` is
now `false`.** `warp` is one ~700k-line crate: a single cycle over it leaves
~4.6 GB, each feature set keeps its own cache, and the total reached 13 GB of the
23 GB peak. Everything else together is smaller. A changed crate now recompiles
in full, which costs minutes on `warp` and almost nothing elsewhere. For a run of
repeated edits to one crate, `CARGO_PROFILE_DEV_INCREMENTAL=true` turns it back
on for that command.

Dependency debuginfo was **not** worth touching: the dev profile is already
`debug = "line-tables-only"`, so `.rlib` and `.dylib` come to 457 MB together.
`[profile.dev.package."*"] debug = false` would cost a full rebuild of ~800
crates and lose backtrace line numbers for a fraction of what incremental cost.

Two things that are not build settings:

- **macOS purges `target/` by itself**, because `target/CACHEDIR.TAG` marks it
  reclaimable. It dropped from 14 GB to 3.4 GB mid-build on 2026-08-20. The
  symptom is `couldn't create a temp dir: No such file or directory …
  target/debug/deps/rmetaXXXX`, which reads like corruption. Re-run the command;
  do not read it as a broken change.
- **Alternating feature sets doubles the artifacts.** `--features simplewarp`
  re-resolves features for the whole dependency graph and keeps a parallel set of
  `.rmeta`. It is unavoidable — checking that binary is the point of the fork —
  but it is a reason to run the default-feature checks together and the
  `simplewarp` check last, rather than interleaving them.

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
