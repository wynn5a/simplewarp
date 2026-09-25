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
| Deletion scope (2026-09-15) | Delete only what **requires a remote service**. A feature that works fully locally stays, even when its flag is constant-false in the simplewarp build — site-count ranking of constant-false flags is not a deletion reason by itself. An attempt to fold `JupyterNotebookRendering` (purely local: local JSON parse + local render, no network) was reverted before commit for exactly this reason. Local-but-disabled features are instead *enable-in-simplewarp* candidates, a separate decision per feature. **JupyterNotebookRendering is now enabled** (2026-09-15, commit "Enable JupyterNotebookRendering in the simplewarp build"): `.ipynb` files open in the notebook viewer instead of raw JSON; a `features::tests` pinning test fails if the feature ever drops out of the set. Remaining enable-candidates: EditableMarkdownMermaid, ImeMarkedText, ITermImages. |

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

`xcode-select` already points at `/Applications/Xcode.app` (verified 2026-09-19;
`xcode-select -p` returns `/Applications/Xcode.app/Contents/Developer`), so no
`DEVELOPER_DIR` override is needed. If it ever points elsewhere, either switch it
permanently (`sudo xcode-select -s /Applications/Xcode.app`) or prefix build commands
with `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`. (`Xcode-beta.app`,
named by earlier rounds, no longer exists.)

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

3k. **The rest of 3i's onboarding cluster — DONE, on explicit direction to stop treating
   `warp-oss`/`stable`/`dev`/`preview` as in-scope.** 3i stopped short of `LoginSlide`,
   `PostAuthOnboarding`, `PasteAuthTokenModal`, and the account-first machinery because,
   unlike `NeedsSsoLink`, they route through `handle_agent_onboarding_event` — reachable only
   when `AuthOnboardingState::Onboarding` is entered, which needs `FeatureFlag::AgentOnboarding`.
   That flag is off for `simplewarp` specifically but still sits in the `default` cargo feature
   set, so `warp-oss` and the other channel binaries built from this same `root_view.rs` could
   still reach it — deleting meant making a call for those binaries too, not just this one.
   Checked what those channels actually are before making that call: `stable`/`preview`/`dev`
   point their `ChannelConfig` at Warp's real production/staging servers
   (`warp_channel_config::load_config!`) and none of the four (`stable`, `preview`, `dev`,
   `local`) are built or tested by this fork's CI (`ci.yml` has no `--bin stable`/`--bin
   dev`/`--bin preview`/`--bin local` step) — they're upstream `warpdotdev/warp` release-channel
   scaffolding this fork inherited, not a product line `wynn5a/simplewarp` ships. Given that,
   told to treat them as out of scope and delete the whole cluster.

   **What went, all from `app/src/root_view.rs` plus three files whose only caller was in it:**

   | Removed | Why it was reachable only from the dead cluster |
   | --- | --- |
   | `AuthOnboardingState::{Onboarding, PostAuthOnboarding, LoginSlide}`, and every match arm across `log_out`, `focus`, `render`, `on_focus`, `complete_auth_and_create_workspace`, `show_web_handoff_view` (wasm) that existed only to enter/exit them | `Onboarding` was the sole state `AgentOnboardingView` renders in; the other two are only reachable from it or from `AuthComplete` (dead per 3i/3j). |
   | `create_agent_onboarding_view`, `debug_enter_onboarding_state` (+ its `shift-f12` `EditableBinding` and `RootViewAction::DebugEnterOnboardingState`), `try_open_onboarding_slides`, `handle_agent_onboarding_event` (~380 lines), `handle_login_slide_event`, `onboarding_theme_kind` | The construction/dispatch machinery for the states above. |
   | `AccountFirstLoginContext`, `AccountFirstCompletion` (+ impl), `account_first_login_context`, `account_first_is_paid`, `account_first_class`, `begin_account_first_post_auth_refresh`, `account_first_offer_experiment_arm`, `resolve_account_first_post_auth`, `handle_account_first_workspaces_event` (+ its `UserWorkspaces` subscription), `complete_account_first`, `offer_variant_for_account_class`, `handle_onboarding_credit_purchase_event`, `refresh_onboarding_account_state`, `requires_post_onboarding_login`, `refresh_pending_onboarding_choices`, `mark_local_onboarding_completed` | The account-first post-auth flow, which only ever ran from `LoginSlide`/`PostAuthOnboarding`. `has_completed_local_onboarding` (the getter, not the `mark_*` setter) stays — `workspace/one_time_modal_model.rs` still calls it. |
   | `paste_auth_token_modal` field/handling, `notify_onboarding_checkout_succeeded` (+ the `url_reports_checkout_success` check in `handle_incoming_auth_url`), `pending_tutorial`/`start_pending_tutorial`, `pending_post_auth_onboarding_settings`, `pending_account_first_*`, `account_first_refresh_in_flight`, `handle_cloud_preferences_syncer_event` (+ its `CloudPreferencesSyncer` subscription) | Each had zero remaining setters/callers once the states above were gone — `pending_tutorial` in particular was only ever set inside `handle_agent_onboarding_event`, so once that's gone `start_pending_tutorial` would have been a permanent no-op left standing, the same "deletion in disguise" shape 4j's `SelectionCursorRenderLocation` finding named. |
   | `app/src/auth/login_slide.rs` (+`login_slide_tests.rs`), `app/src/auth/paste_auth_token_modal.rs`, `app/src/ai/onboarding.rs` (~1,940 lines total) | Their only callers were the deleted `root_view.rs` code; confirmed via `cargo check` (each type/function came back "never used"/"never constructed" once its one call site was gone), not by inspection alone. |
   | `AuthManager::link_sso_url` | Orphaned one hop out: its only caller was `LoginSlideView::handle_auth_manager_event`, gone with `login_slide.rs`. Found by the post-deletion clippy pass, same pattern as 4j. |
   | 6 tests in `root_view_tests.rs` that exercised the deleted functions directly | `account_first_class_uses_paid_status_then_fresh_request_limit`, `account_first_requires_login_even_without_ai_or_drive_settings`, `fallback_flow_only_requires_login_for_account_backed_settings`, `account_first_classes_route_to_paid_or_the_expected_offer`, `account_first_completion_metadata_matches_terminal_outcomes`, `refreshing_pending_onboarding_choices_replaces_stale_settings`. The three `sync_local_onboarding_to_server` tests stay — that helper is untouched. |

   **Deliberately left alone, and why:**

   - `AuthOnboardingTarget` enum + `AuthOnboardingTarget::to_workspace` — native `cargo check`
     reports them unused, but that's an artifact of checking a non-wasm target:
     `AuthOnboardingState::WebImport(AuthOnboardingTarget)` is real, `#[cfg(target_family =
     "wasm")]` code, same as `web_handoff.rs`/`login_error_modal.rs` in 3h/3i. Kept for the wasm
     build, which this round could not fully verify (see below).
   - `onboarding_credit_pack_options` (`pricing/mod.rs`), `url_reports_checkout_success` +
     `CHECKOUT_SUCCESSFUL_PARAM` (`uri/mod.rs`) — lost their only production caller but still
     have direct test coverage (`pricing_tests.rs`, `uri_tests.rs`), the same "test-only caller
     stays" call 4f/4j already made elsewhere. Both live in modules outside this round's scope.
   - `Workspace::open_vertical_tabs_panel_if_enabled`, `OnboardingTutorial::intention` — now
     genuinely zero-caller (not even a test), surfaced by the post-deletion clippy pass, but
     each lives in a different large file (`workspace/view.rs`, `workspace/view/onboarding.rs`)
     this round never otherwise touched. Left for a future clippy sweep, matching how 4j
     deferred `tear_down_cloud_mode_setup_phase`'s cluster rather than chasing every orphan
     into an unrelated file.
   - `crates/onboarding` (43 files, ~16k lines, home of `AgentOnboardingView` itself) — not
     touched at all. It's shared with a separate, likely-live "Get Started" in-terminal
     onboarding surface (`terminal/view/block_onboarding/`, the `workspace/view.rs:7681` gate
     that shows it precisely when `AgentOnboarding` is *off*) that this round never traced.
     `AgentOnboardingView` may now be fully unused within that crate — worth checking in a
     dedicated round scoped to `crates/onboarding` itself, not assumed here.

   Acceptance: `cargo check -p warp --lib --all-targets`, `cargo clippy -p warp --lib
   --all-targets`, `cargo check --no-default-features --features simplewarp --bin simplewarp`,
   and `cargo check -p warp --bin warp-oss` all clean — the last one confirms this holds under
   `default` features too, i.e. for the channels this round decided don't matter.
   `./script/format --check` clean. `cargo nextest run -p warp --lib`: **5992 pass, 0 fail, 4
   skipped** (7 fewer than 3j's 5999 — the 6 deleted `root_view_tests.rs` tests plus
   `login_slide_tests.rs`'s own test). **Not verified: the wasm32-unknown-unknown target.**
   Installed it and ran `cargo check -p warp --lib --target wasm32-unknown-unknown`; it failed
   before reaching `app` at all, inside `crates/local_inference` (`Send`-bound errors on
   `JsFuture` in `stream.rs`, unrelated to auth/onboarding and not touched by this round) — a
   pre-existing break in a dependency of `app`, not evidence either way about the wasm-gated
   code this round edited (`show_web_handoff_view`, `complete_web_import`). Not re-run in the
   app.

3l. **Logging out no longer shows a login screen — DONE.** 3h found `app/src/auth/`'s UI files
   (`auth_view_modal.rs`, `auth_view_body.rs`, `auth_view_shared_helpers.rs`,
   `login_failure_notification.rs`, `auth_override_warning_modal.rs`,
   `auth_override_warning_body.rs`) live through exactly one runtime path: Settings → Account →
   "Log out" flips `AuthOnboardingState::Terminal` to `Auth`, and `RootView::render` shows the
   full-screen `AuthView`. On explicit direction that this fork needs no login/account/quota UI
   at all, traced what that transition is actually for and killed it at the root rather than
   deleting the (still partly load-bearing) files it points at.

   **The fix is one arm of `AuthOnboardingState::log_out`.** Real login can never complete in
   this fork (3c pointed the channel config at `.invalid` hostnames, so every network call —
   including `AuthClientImpl::fetch_user`'s own HTTP client in `warp_server_client`, a separate
   code path from 3e/3f's `app/src/server/` stubs — dies at DNS resolution). So sending a
   logged-out user to a login screen was already a dead end: `Auth` was reachable, but nothing
   past it ever worked. Changed `log_out`'s `Terminal` arm to build a fresh empty `Terminal`
   workspace directly, the same as it already did on the *inside* (tear down the old workspace,
   reset `workspace_setting`) — it just no longer detours through `Auth` to get there. "Log out"
   in a local-only build now means "reset local state", nothing more.

   **That made `AuthOnboardingState::Auth` and `::ConfirmIncomingAuth` fully unreachable, and
   the compiler + a trace of every remaining construction site confirmed it, not assumption:**
   startup already always lands on `Terminal` (`SkipFirebaseAnonymousUser`, unconditional for
   this fork per 3h), logout no longer constructs `Auth`, and the one other route —
   `handle_auth_manager_event`'s `LoginOverrideDetected` arm calling
   `open_auth_override_warning_modal` to enter `ConfirmIncomingAuth` — only fires when
   `self.auth_onboarding_state` is *already* `Auth`/`ConfirmIncomingAuth`; from `Terminal` (now
   permanent) it's a no-op, matched by the existing `_ => {}`. Deleted both variants, `RootView`'s
   own `auth_view`/`auth_override_view` fields, `complete_auth_and_create_workspace` (both
   remaining callers were the arms just removed), `handle_auth_override_warning_modal_event`,
   `open_auth_override_warning_modal`, and `export_all_warp_drive_objects` (its only caller).

   **`AuthView` and `AuthOverrideWarningModal` themselves stay — they are independently live a
   second way this round does not touch.** `Workspace` owns its *own* instances
   (`require_login_modal`, `auth_override_warning_modal`, built with
   `AuthViewVariant::RequireLoginCloseable`/`AuthOverrideWarningModalVariant::WorkspaceModal`)
   and pops them whenever a user hits a cloud-gated feature — `AuthManager::attempt_login_gated_feature`,
   called from 9 sites across Drive, Teams, and the command palette, checks
   `is_anonymous_or_logged_out()`, which 3h already found is unconditionally `true` in this fork.
   So every Drive/Teams action still nags the user to log in, into a screen that (per the DNS
   argument above) can never complete. **That nag is real UI debt matching this round's request,
   but it belongs to the Drive/Teams/`cloud_object` gating cluster (item 3's `remote_server`/
   `cloud_object` remainder, already deferred, not to `auth`) — collapsing it means deciding what
   "click a cloud feature" does with no cloud, which is a `drive`-scale question, not this one.**

   **One direct follow-on taken, one left.** `AuthOverrideWarningModalVariant` lost its
   `OnboardingView` arm (RootView's own instance, deleted) and now has exactly one variant
   (`WorkspaceModal`, still used by `Workspace`) — the same "single-variant enum, deletion in
   disguise" shape 4g/4j named, so it went too: the enum, the `variant` field, and `render`'s
   match all collapsed to the one background color that's still reachable.
   `AuthViewVariant::Initial` — RootView's own `AuthView::new(AuthViewVariant::Initial, ...)`
   call, also deleted — is left standing: it is not a bare enum collapse, it's matched across
   ~10 sites in `auth_view_body.rs`/`auth_view_modal.rs` with genuinely different copy and
   layout per arm ("Welcome to Warp!" vs. "Sign up for Warp", differing bodies at lines 702 vs.
   732, 923 vs. 916), a 900-line file this round did not otherwise read closely enough to edit
   safely. `cargo check` still flags it (`variant Initial is never constructed`) — a clean
   pointer for whoever does that pass next.

   Also fixed as a direct consequence, not scope creep: `handle_incoming_auth_url`'s malformed-
   URL branch stopped writing `last_login_failure_reason` onto a `self.auth_view` that no longer
   exists (kept the `safe_error!` log, dropped the now-impossible UI surface); wasm's
   `handle_web_handoff_event` fallback for a failed `Workspace` re-import now builds a fresh
   `Terminal` workspace instead of falling back to the deleted `Auth` state, mirroring its
   sibling `Terminal` branch one match arm below.

   Acceptance: `cargo check --no-default-features --features simplewarp --bin simplewarp`,
   `cargo check -p warp --bin warp-oss`, and `cargo clippy -p warp --lib --all-targets
   --no-default-features --features simplewarp` all clean (0 errors; only the pre-existing
   `AuthViewVariant::Initial`-and-friends dead-code warnings named above, plus the unrelated
   backlog from 4j/3k). `./script/format --check` clean. `cargo nextest run -p warp --lib
   --no-default-features --features simplewarp --no-fail-fast`: 5984/5992 pass, 8 fail — the
   same 8 (`cloud_preferences_syncer`'s six, `workspace::view::tests`'s two Drive/signup tests)
   reproduce identically on a clean stash of master with zero code changed, confirmed by
   re-running the same filter against the stashed tree before restoring this round's diff. 0
   tests deleted (no test file referenced `AuthOnboardingState::Auth`/`ConfirmIncomingAuth` to
   begin with). Not re-run in the app.

3m. **`app/src/workspaces/` (the cloud team-workspace concept) surveyed, not touched —
   confirmed the same scale as `remote_server`/`cloud_object`, on explicit direction to scope
   this round down to provably-dead code only rather than attempt it in one pass.**

   **The scale first:** `grep -rl "workspaces::\|UserWorkspaces"` outside the module itself
   returns **over 100 files** — AI settings inheritance, `terminal/cli_agent.rs`, pane/session
   restoration, `settings_view` (`teams_page.rs` alone is 4,697 lines), notebooks, drive. Same
   shape as the already-deferred `remote_server`/`cloud_object`/`drive`-remainder cluster in
   item 4's checklist, not a follow-on to 3l's auth fix.

   **The one clean finding: `UserWorkspaces.workspaces` and `.current_workspace_uid` are
   provably always empty/`None` in this fork, for the same two independent reasons 3i/3l
   already established elsewhere.** Traced the constructor
   (`app/src/lib.rs:1446`, `UserWorkspaces::new(cached_workspaces, current_workspace_uid, ..)`):
   both arguments come from a local SQLite read (`persisted_workspaces`) that a fresh
   local-only install never populates, falling back to `Default::default()` (empty/`None`) when
   the read is empty. The only thing that could ever refresh them post-startup —
   `TeamClient`/`WorkspaceClient` (`app/src/server/server_api/team.rs`,
   `.../workspace.rs`) — **are already fully stubbed by 3e/3f**: `local_only_error()` appears 16
   and 5 times respectively, i.e. every team/workspace network method already returns
   immediately with no request built. So this is not new dead code to cut; 3e/3f already cut
   the layer that would matter.

   **What's left standing above that line is the `UserWorkspaces` model and its 100+ UI/logic
   consumers, and none of it is compiler-provable dead the way `NeedsSsoLink` was.** Every
   consumer checked (`team_uid: Option<ServerId>`, `is_on_uber_team`,
   `inherited_or_default_team_uid`, `admin_billing_link_for_default_team`, the Teams settings
   page, `team_tester.rs`'s data-poller trigger) already handles the "no team" case gracefully
   — it's logic that *always* takes one branch given the empty/`None` invariant above, not
   logic the compiler will ever flag as unreachable. Turning "always takes the no-team branch"
   into "deleted" needs the same site-by-site trace 3i/3k/3l already used for auth, just at 10x
   the surface area — a dedicated multi-round effort, not a pass that fits in this one.

   **Not a small cut even at the edges.** Checked `team_tester.rs` as the smallest candidate
   (38 lines, a single-method event bus for triggering workspace-metadata polling) hoping it
   would isolate cleanly like `NeedsSsoLink` did — it doesn't: `initiate_data_pollers` still
   threads into `UpdateManager`/`TeamUpdateManager`'s polling machinery and
   `auth_manager.rs`'s anonymous-user-linking flow, another 6 files, before hitting a boundary.

   No code changed this round. Next round scoped to this item should start the same way 3i did
   for auth: pick one self-contained UI surface (the Teams settings page is the closest analog
   to Settings → Account, but at 4,697 lines it is not a quick first cut — `joinable_teams`/the
   join-a-team-via-link flow may be smaller and worth checking first) and trace its construction
   sites fully before deleting.

3n. **The Drive/Teams "please log in" nag no longer shows the dead-end sign-up modal — DONE.**
   Scoped to what's safe and root-cause without re-opening the deferred `workspaces/` item: found
   the single function every "you need an account for this" trigger converges on
   (`Workspace::open_require_login_modal`) and fixed it there, rather than touching the ~6 call
   sites (Team* drive/notebook actions, the Teams and Account settings pages, a Drive share
   dialog) that call into it. It now shows a small toast — "SimpleWarp is a local-only build;
   sign-in and team features aren't available" — instead of focusing the full-screen `AuthView`
   sign-up modal, which could never complete anyway (3l: every network path is DNS-blocked).
   None of those ~6 call sites, `AuthManager::attempt_login_gated_feature`, or the `Team*`
   action-gating logic itself needed to change — same shape as fixing `log_out` once in 3l
   instead of editing every place that could reach `Auth`.

   Two more nags fixed alongside it, found while tracing every path into the modal:

   - **`terminal::view::action::TerminalAction::AttemptLoginGatedFeature` — deleted, DONE.**
     A leftover from the pre-4c/4d billing UI ("Upgrade AI Usage"), with zero construction sites
     anywhere in the workspace (confirmed by a whole-repo grep, not just `app/src/`) — the
     button that used to dispatch it went with `buy_credits_banner.rs` in 4c. Removed the
     variant, its `Display` arm, and its two now-dead match arms in `terminal/view.rs`
     (`accessibility_content`'s grouping and the handler itself), plus the two imports
     (`AuthManager`, `AuthViewVariant`) that were only there for it.
   - **A real bug, not just cosmetic: `Workspace::run_workflow_in_active_input` was blocking
     every agent-mode workflow for every user — DONE.** Its comment said "View-only sessions
     should not be able to run workflows" (session sharing's viewer mode, deleted in 4e), but
     the actual condition it guarded was `self.auth_state.is_anonymous_or_logged_out()` — always
     `true` in this fork (3h) — with no relation to viewing left. So every attempt to run an
     agent-mode workflow hit the sign-up nag and `return`ed before running anything, unlike
     every other AI feature Phase 3 made local-first. Deleted the stale guard; workflows run
     unconditionally now, same as the rest of the app.

   **Deliberately not chased further, and why.** `open_require_login_modal` no longer focuses
   `Workspace::require_login_modal`, so that `ViewHandle<AuthView>` field, its construction
   (`build_require_login_modal`), its event handler (`handle_require_login_modal_event`), and
   its conditional render block are now fully inert — confirmed by the post-fix `cargo check`,
   which newly flags `AuthView::set_variant`/`AuthViewBody::set_variant` as unused. Two call
   sites (`LeftPanelEvent::SignInRequested`'s handler, `initiate_user_signup`) still poke this
   now-invisible view directly (`start_sign_in`, `skip_to_browser_open_step`) — harmless (no
   user-visible effect) but not yet deleted. Left standing on purpose: removing
   `require_login_modal` itself cascades into the actual "Sign In"/"Sign Up" entry points
   (`LeftPanelAction::SignIn`, `initiate_user_signup`, `AuthManager::initiate_anonymous_user_linking`,
   `sign_up_url`) and their rendering in `left_panel.rs`, which is a second, separable round —
   the nag itself (this round's target) is gone either way.

   `AuthViewVariant::HitDriveObjectLimitCloseable` (`AuthManager::anonymous_user_hit_drive_object_limit`)
   was traced too and left untouched: it already can never fire, for a reason independent of this
   round's fix — `is_anonymous_user_feature_gated()` reads `self.user`, which never populates in
   this fork (same "the round-trip that would set it is DNS-blocked" root cause as 3i/3l), so the
   `&&`-guarded emit is unreachable in practice already, just not compiler-provably. No code
   changed for it; not worth touching since it produces no user-visible nag today.

   Acceptance: `cargo check --no-default-features --features simplewarp --bin simplewarp`,
   `cargo check -p warp --bin warp-oss`, and `cargo clippy -p warp --lib --all-targets
   --no-default-features --features simplewarp` all clean (0 errors). `./script/format --check`
   clean. `cargo nextest run -p warp --lib --no-default-features --features simplewarp
   --no-fail-fast`: 5984/5992 pass, the same 8 pre-existing failures as 3l (`cloud_preferences_syncer`'s
   six, `workspace::view::tests`'s two) — one of which,
   `test_tools_panel_preferences_activate_after_signup_and_ai_enablement`, also asserts
   `is_require_login_modal_open` after `SignInRequested` and will need that assertion updated
   whenever its pre-existing, unrelated failure is fixed; not touched here since it was already
   failing before this round for a different reason. Disk stayed well within bounds throughout
   (target/ 8.4 GB, 42 GB free). Not re-run in the app.

3o. **`AuthView`, `AuthViewBody`, and the whole remaining login-UI file set — DELETED, closing
   out the multi-round auth thread that started at 3h.** 3n left `Workspace::require_login_modal`
   fully inert (never focused, never rendered) but still constructed at startup. Deleting that
   construction made `AuthView::new` genuinely unreachable — the compiler's own dead-code
   cascade proved it, the same signal 3l used for `NeedsSsoLink`: a fresh `cargo check` after
   removing the one remaining `AuthView::new` call site flagged `AuthView::set_variant`,
   `AuthViewBody::set_variant`, half of `AuthViewVariant`, most of `AuthManagerEvent`'s variants,
   and several `LoginFailureReason`/`AuthStep` variants as newly dead in one pass. `grep -rn
   "AuthView::new"` confirmed zero remaining callers workspace-wide before deleting anything.

   **Two more browser-opening dead ends found and fixed on the way, same shape as 3n's toast
   fix.** `Workspace::initiate_user_signup` (4 call sites: a Settings → Account "Sign up" button,
   a renotification banner, and two more) and `LeftPanelEvent::SignInRequested`'s handler (the
   locked-panel "Sign in" button shown whenever Warp Drive/Conversation History is opened while
   anonymous — always, since `is_anonymous_or_logged_out()` is unconditionally `true` per 3h)
   both still called `AuthManager::sign_up_url()`/`AuthView::start_sign_in()`, opening a real
   browser tab to a URL that can never complete a login (3l: DNS-blocked), even after 3n's fix
   made the modal invisible. Both now just call `open_require_login_modal` (the toast), nothing
   else. A third one — `WorkspaceAction::Reauth`'s `sign_in_url()`/`ctx.open_url` — was traced
   and left alone: its banner is gated on `auth_state.needs_reauth()`, which requires a real 401
   response from a network request that actually connected, impossible under 3c's DNS block, so
   it's unreachable the same way `HitDriveObjectLimitCloseable` was in 3n. `redirect_to_sign_in`
   (behind `WorkspaceAction::SignInAnonymousWebUser`, another "Sign up" button) needed no fix:
   its entire body is `#[cfg(target_family = "wasm")]`, already a no-op on native builds.

   **`AuthViewVariant` itself went too, not just the two dead views it described.** Six external
   files (`workspace/view.rs`, `search/command_search/view.rs`, `settings_view/main_page.rs`,
   `settings_view/teams_page.rs`, `drive/index.rs`, plus `auth_manager.rs` itself) still passed
   an `AuthViewVariant` into `attempt_login_gated_feature`/`open_require_login_modal` purely to
   be discarded — `open_require_login_modal` had already reduced its parameter to `_variant`
   in 3n. Rather than leave a type used by nothing but its own dead file, dropped the parameter
   at its one source (`attempt_login_gated_feature`, `AuthManagerEvent::AttemptedLoginGatedFeature`,
   `open_require_login_modal`) and updated all six call sites — the same "fix the shared
   function, not every caller" shape as every round since 3l. That, in turn, let
   `auth_view_modal.rs` shed everything except `AuthRedirectPayload` (still real: parses the
   `warp://auth/...` deep-link URL for `handle_incoming_auth_url`/`initialize_user_from_auth_payload`,
   both still live per 3h/3l) — kept in place, same file, so the four files that import
   `AuthRedirectPayload` from it needed no path changes.

   **Deleted:** `app/src/auth/auth_view_body.rs` (1,087 lines), `auth_view_shared_helpers.rs`
   (602 lines), `login_failure_notification.rs` (165 lines) — all three had zero callers left
   once `AuthView` did. `AuthManager::create_anonymous_user`/`on_create_anonymous_user`
   (only caller was the deleted `AuthView::handle_login_later`), `sign_up_url`/
   `copy_anonymous_user_linking_url_to_clipboard` (only caller was the deleted `auth_view_body.rs`),
   `AuthRedirectPayload::from_raw_url` (only caller was the deleted paste-token handler), and
   `AnonymousUserCreationError` (`server/server_api/auth.rs`, only referenced by the deleted
   `on_create_anonymous_user`) — each confirmed zero callers, including test files, before
   removal. `auth/mod.rs::init` no longer calls `auth_view_modal::init`/`auth_view_body::init`
   (both registered `FixedBinding`s scoped to `id!(AuthView::ui_name())`/`"AuthViewBody"`, inert
   once nothing constructs those views).

   **Deliberately left standing, and why:** `AuthManagerEvent::CreateAnonymousUserFailed` and
   `::SkippedLogin` are declared but now unconstructed — matched in `|`-grouped catch-alls in
   `ai/connected_self_hosted_workers.rs` and `ai/mcp/templatable_manager/native.rs` that this
   round didn't otherwise touch; harmless to leave, a two-file follow-on if anyone wants it.
   `WorkspaceAction::AttemptLoginGatedAIUpgrade` (a live "Upgrade" button in
   `workflows/workflow_view.rs`) is the same stale "Upgrade AI Usage" leftover as the two
   `AttemptLoginGatedFeature`/`AttemptLoginGatedUpgrade` actions 3n deleted, except this one
   still has a real dispatch site — only mechanically updated to match
   `attempt_login_gated_feature`'s new signature, not traced further. `experiments/mod.rs`'s
   `pub use login_layer::AuthFlowInstructions` is now an unused re-export (its only reader was
   the deleted `auth_view_body.rs`) — a one-line, unrelated-module loose end, not fixed here.

   Acceptance: `cargo check --no-default-features --features simplewarp --bin simplewarp
   --all-targets`, `cargo check -p warp --bin warp-oss`, and `cargo clippy -p warp --lib
   --all-targets --no-default-features --features simplewarp` all clean (0 errors; 48
   warnings, matching 3n's pre-existing baseline plus the two documented loose ends above).
   `./script/format --check` clean. `cargo nextest run -p warp --lib --no-default-features
   --features simplewarp --no-fail-fast`: 5984/5992 pass, the same 8 pre-existing failures as
   3n/3l, 0 new — confirmed with a full `--no-fail-fast` run, not just the fail-fast summary.
   15 files changed, 2,456 lines removed against 24 added. Disk stayed well within bounds
   (target/ 9.3 GB, 40 GB free). Not re-run in the app.

3p. **The three loose ends 3o flagged and left standing — DONE, plus one more found along
   the way.** All three were closed this round, in order of size.

   **`AuthManagerEvent::CreateAnonymousUserFailed`/`SkippedLogin` — DELETED.** Whole-repo
   `grep -rn "ctx.emit(AuthManagerEvent::"` confirmed neither variant is ever constructed
   (their only emitter was the deleted `AuthView::handle_login_later`, per 3o). Removed both
   declarations and simplified the two `|`-grouped catch-all match arms in
   `ai/connected_self_hosted_workers.rs` and `ai/mcp/templatable_manager/native.rs` that named
   them.

   **`experiments::login_layer` (`AuthFlowInstructions`) — DELETED.** An A/B-test definition
   for "explicitly instruct users to go to their browser to continue authenticating" — dead
   the moment `AuthView` (3o) went, since its `get_group()` was never called from anywhere
   once the auth UI it gated was gone. Confirmed via `grep -rn "AuthFlowInstructions::"`
   returning zero callers outside its own file. Deleted `login_layer.rs` outright (module
   declaration, re-export, and its entry in the `LAYERS` registration vec in
   `experiments/mod.rs`) rather than leaving an inert layer in a list that exists specifically
   to enumerate live experiments. The generic experiment framework itself (`Layer`,
   `Experiment<T>`, `LAYERS`) is untouched — three other real experiments still use it.

   **`WorkspaceAction::AttemptLoginGatedAIUpgrade` — DELETED, via the same "trace to the
   root, not the button" approach as every round since 3l.** 3o flagged this as a live dispatch
   site and stopped there. Tracing further: its only two callers
   (`WorkflowModal::issue_request` in `drive/workflows/ai_assist.rs` and the near-duplicate
   `WorkflowView::issue_request`/`WorkflowPane` in `workflows/workflow_view.rs`) both gate the
   whole "you're out of AI credits, upgrade?" branch on
   `GeneratedCommandMetadataError::RateLimited`. `grep -rn "fn generate_metadata_for_command"`
   found exactly one implementor of that trait method in the whole workspace — the local-only
   stub in `server/server_api/ai.rs`, which unconditionally returns `Err(...::Other)`. The
   `From<GenerateMetadataForCommandFailureType>` conversion that could theoretically produce
   `RateLimited` is only reachable from a real network response, which this stub never
   produces. So `RateLimited` can never occur in this fork — the entire team/admin-permission
   branch nested under it (checking `UserWorkspaces::team_for_view`, `has_admin_permissions`,
   `can_upgrade_to_higher_tier_plan`) was dead in both files, same "provably dead but not
   compiler-dead" shape as `is_anonymous_or_logged_out()` always being `true` (3h). Collapsed
   both `Err(err) => { ... }` blocks down to their one live line (`emit
   WorkflowModalEvent::AiAssistError`/`pane.display_error_toast`, unconditionally). That, in
   turn, let the compiler's own dead-code cascade take it from there in one more pass:
   `WorkflowModalEvent::AiAssistUpgradeError` (unconstructed → variant deleted, handler in
   `workspace/view.rs` deleted), `WorkflowView::display_upgrade_error` (uncalled → deleted),
   `WorkspaceAction::AttemptLoginGatedAIUpgrade` (nothing left to dispatch it → variant and its
   `handle_action` arm deleted), and the now-write-only `WorkflowView.auth_state` field
   (nothing left reading it → field and its one constructor assignment deleted). Left
   `GeneratedCommandMetadataError::RateLimited` itself in place — the enum is `Serialize`d into
   telemetry (`serde_json::json!(err)`) regardless of variant, so it costs nothing to keep, and
   deleting it would mean also hand-editing the `From` impl for no behavioral gain.

   **One more found opportunistically, unrelated to the AI-upgrade thread:**
   `server/graphql/mod.rs` re-exported `get_user_facing_error_message` alongside
   `GraphQLError`, but every real caller (`settings_view/platform/*.rs`,
   `ai/agent_sdk/api_key.rs`, `warp_server_client/src/auth/mod.rs`) already calls it via the
   fully-qualified `warp_graphql::client::get_user_facing_error_message` path, not the
   re-export — this warning was pre-existing debt from 3o's `auth_manager.rs` trim, just never
   surfaced until this round's first clean `--all-targets` build. Dropped the re-export,
   kept `GraphQLError` (genuinely still re-exported and used by two files).

   **Traced and confirmed NOT dead — left alone:** `app/src/auth/login_error_modal.rs`'s
   `LoginErrorModal` shows the same "never constructed" warning as everything else this round,
   but its one call site (`auth/web_handoff.rs`) is behind `wasm_bindgen`/wasm-only code —
   a different, real build target (the web/host-embedded terminal), not something this
   native-`simplewarp`-bin build exercises. Not part of the local-only/DNS-blocked dead-code
   pattern this whole thread has been finding; left untouched.

   **Noticed, not acted on — flagged for a future round, not a quick fix:** a full
   `--all-targets` build (this session's first with `DEVELOPER_DIR` set, see the build
   prerequisites note) surfaced ~25 "irrefutable `if let` pattern" warnings in `root_view.rs`,
   all matching on `AuthOnboardingState::Terminal(..)`. On this non-wasm build target that
   enum only has one variant (`WebImport` is `#[cfg(target_family = "wasm")]`, since 3l), so
   every match against it is provably exhaustive already. Collapsing the enum away on native
   builds would mean restructuring `RootView.auth_onboarding_state`'s type per-target (it's
   genuinely two-variant on wasm) and touching ~20 call sites across `root_view.rs` — a real
   simplification, but a refactor of live, working, correctly-compiling code rather than a
   dead-code deletion, and higher blast radius than anything else this thread has done
   unattended. Left for a dedicated round with the user present to review the diff.

   Acceptance: `cargo check --no-default-features --features simplewarp --bin simplewarp
   --all-targets` and `cargo check -p warp --bin warp-oss` both clean (0 errors). `cargo
   clippy -p warp --lib --all-targets --no-default-features --features simplewarp` clean (0
   errors). `./script/format --check` clean (after one `./script/format` pass for reflow).
   `cargo nextest run -p warp --lib --no-default-features --features simplewarp
   --no-fail-fast`: 5984/5992 pass, the same 8 pre-existing failures as every prior round in
   this thread (3l/3n/3o), 0 new. 11 files changed. Disk stayed well within bounds (target/
   7.2 GB, 39 GB free). Not re-run in the app. This closes out every loose end 3o named; the
   `AuthOnboardingState` refactor and the still-untouched `app/src/workspaces/` module (3m)
   are the two remaining threads, both deliberately deferred pending explicit go-ahead.

3q. **Two more test-only-covered billing/onboarding leftovers — DONE, found via 3p's clean
   `--all-targets` build's remaining warning list, not by searching further.**

   **`pricing::onboarding_credit_pack_options` — DELETED.** Converted the server's add-on
   credit-pack pricing into display options "shown on the onboarding offer slide" (its own doc
   comment). That offer slide is `onboarding::agent_onboarding_view`/`slides::offer_slide` —
   a whole separate crate, still alive and heavily used *within itself* (`CreditPackOption` is
   real there), but `grep -rn "AgentOnboardingView\|offer_slide"` against `app/src` returned
   nothing: the app never wires that crate's credit-pack UI up at all, consistent with the
   onboarding cluster 3h–3k already deleted. The function's only callers left were five of its
   own six tests. Deleted the function and its `use onboarding::CreditPackOption` import from
   `pricing/mod.rs`, and trimmed `pricing_tests.rs` down to the one surviving test
   (`promotion_message_is_exposed_verbatim`, which exercises `PricingInfoModel` directly, not
   this function). `PricingInfoModel` itself, `AddonCreditsOption`, and the `onboarding` crate
   are all untouched — genuinely still live elsewhere.

   **`uri::url_reports_checkout_success`/`CHECKOUT_SUCCESSFUL_PARAM` — DELETED.** Parsed the
   `checkoutSuccessful=true` query param a web Stripe-checkout confirmation page would append
   to the desktop hand-off deeplink. Its own test's doc comment named the purpose: "so
   onboarding can advance without opening a settings page" — the same deleted onboarding flow
   as above. Zero production callers (`grep -rn` confirmed), only its own regression test
   (`test_url_reports_checkout_success`, REV-1952). Deleted the const, the function, and the
   test; `ChannelState`/`Url` imports in `uri_tests.rs` are still needed by other tests in the
   file (`use super::*` covers the rest, nothing to trim there).

   **Traced and confirmed NOT dead — left alone:** `AuthManagerEvent::MintCustomTokenFailed`'s
   payload field ("field is never read") is a live event on a live code path (JWT/custom-token
   minting, still real) — its two `|`-grouped matchers just don't inspect the specific error
   value, they react to the event firing at all. Not part of this thread's pattern (nothing
   here is unreachable), so left as-is; a clippy nit, not dead code.
   `ai/agent_sdk/driver/terminal.rs::ShareSessionError::Failed` is agent-session-sharing
   infrastructure, adjacent to the deliberately-paused `app/src/workspaces/` (3m) scope, not
   the login/account/quota-UI thread — not investigated further this round.
   `root_view.rs::AuthOnboardingTarget`'s "never used" warning is the same
   `#[cfg(target_family = "wasm")]`-only shape as `LoginErrorModal` (3p): real on the wasm
   target, invisible on this native build. Folded into the already-deferred
   `AuthOnboardingState` refactor note from 3p, not a separate item.

   Acceptance: same four checks as every round in this thread, all clean — `cargo check` both
   feature sets (0 errors), `cargo clippy -p warp --lib --all-targets --no-default-features
   --features simplewarp` (0 errors), `./script/format --check` (clean after one format pass),
   `cargo nextest run -p warp --lib --no-default-features --features simplewarp
   --no-fail-fast`: 5978/5986 pass (6 fewer total tests than 3p's 5992, from the 6 deleted
   pricing tests), same 8 pre-existing failures, 0 new. 4 files changed. Disk: target/ 9.2 GB,
   37 GB free.

3r. **The wasm/web-embedded-terminal login-handoff surface — DELETED, on explicit go-ahead,
   closing out 3p/3q's `AuthOnboardingTarget`/`AuthOnboardingState` deferral.** ~1,050 lines
   across 5 files, plus a ~620-line net simplification of `root_view.rs`.

   3k through 3q all treated `AuthOnboardingState::WebImport`/`AuthOnboardingTarget`/
   `LoginErrorModal` as real-but-unverifiable: genuine `#[cfg(target_family = "wasm")]` code,
   kept because "a different, real build target" this fork's `cargo check` never exercises.
   Re-examined instead of re-deferred this round, because the question is a scope call, not a
   mechanical one: does `wynn5a/simplewarp` still ship a web/host-embedded-terminal build?
   Two things settled it. First, `.github/workflows/ci.yml`'s `wasm-lint` job (`cargo clippy
   --target wasm32-unknown-unknown -- -D warnings`) is inherited scaffolding this fork's own
   CI never runs — `gh run list` against `wynn5a/simplewarp` returns zero runs, ever. Second,
   the whole surface exists to import a Firebase login session from a host webpage
   (`WebHandoffView::import_user`, `platform::wasm::user_handoff`) — a cloud/login feature,
   the exact category this fork's Decisions table rules out everywhere else. Same reasoning
   3k already used for the `stable`/`preview`/`dev`/`local` channel binaries: upstream
   `warpdotdev/warp` scaffolding this fork inherited, not a surface it ships. Confirmed with
   the user this changes nothing about the native `simplewarp` build or `cargo run` before
   deleting anything — `#[cfg(target_family = "wasm")]` code is compiled out entirely on
   `aarch64-apple-darwin`.

   **Deleted outright:** `app/src/auth/web_handoff.rs` (`WebHandoffView`, `WebHandoffEvent`,
   `HandoffState`) and `app/src/auth/login_error_modal.rs` (`LoginErrorModal` — its one call
   site, kept alive through 3p/3q specifically because it looked wasm-live, was `web_handoff.rs`
   itself). `platform::wasm::user_handoff`/`AuthHandoffError`/its `mod ffi` — `web_handoff.rs`
   was their only caller; `platform::wasm::init` and the `WarpEvent`/`emit_event` re-export stay,
   genuinely used elsewhere for non-auth wasm plumbing (`app/src/appearance.rs`,
   `uri/browser_url_handler.rs`, five more).

   **Collapsed in `root_view.rs`:** `AuthOnboardingState` (`WebImport`/`Terminal`) and
   `AuthOnboardingTarget` are gone; `RootView.auth_onboarding_state: AuthOnboardingState`
   became `RootView.workspace: ViewHandle<Workspace>` directly, since `Terminal` was the only
   variant a native build could ever construct. Every one of the ~25 `if let
   AuthOnboardingState::Terminal(x) = &self.auth_onboarding_state { ... } else { log::warn!
   ("Auth not complete...") }` sites 3p flagged as irrefutable-on-native collapsed to direct
   `self.workspace` use, dropping the dead `else` arm at each one — the "Auth not complete"
   branch could never fire once `WebImport` was the only other variant and it's gone.
   `AuthOnboardingState::log_out`/`show_web_handoff_view`/`complete_web_import` (impls) folded
   into `RootView::log_out`/deleted outright; `handle_web_handoff_event` deleted with its
   subscription in `RootView::new`.

   **One cascade the wasm-scoping check didn't cover on its own: `RootViewEvent`.** Its single
   variant, `AuthOnboardingStateChanged`, existed only to announce a `WebImport`↔`Terminal`
   transition — and `grep -rn "AuthOnboardingStateChanged\|subscribe_to_view(&.*root_view"`
   found zero subscribers anywhere, on either target. A write-only event over a state that no
   longer has two states to transition between: deleted the enum, `type Event = RootViewEvent`
   became `type Event = ()` (an established pattern — three other views in `app/src` already
   use it), and all four `ctx.emit(...)` call sites went with it.

   **A second cascade the same collapse exposed: `RootView`'s own traffic-light rendering.**
   `traffic_light_data()`'s own comment said it: "The workspace view will handle rendering of
   the traffic lights" — confirmed by `grep -rl "traffic_lights::"`, which found
   `workspace/view.rs` has its own independent call into the same shared
   `util::traffic_lights` module. `RootView`'s copy existed only to draw traffic lights during
   the pre-workspace `WebImport` state; `if matches!(self.auth_onboarding_state,
   AuthOnboardingState::Terminal(_)) { return None; }` meant it always returned `None` once
   `Terminal` was the only state, which made the `if let Some(traffic_light_data) = ...`
   block in `render()`, the `mouse_states`/`window_id` fields, and four now-single-purpose
   imports (`ChildAnchor`, `OffsetPositioning`, `ParentAnchor`, `ParentOffsetBounds`) all
   provably dead in the same pass. `util::traffic_lights` itself is untouched — still live for
   `workspace/view.rs`.

   Verified the app itself, not just the checks: built and launched `./target/debug/simplewarp`
   with no args (the exact `RootView::new` → `workspace_args.create_workspace(ctx)` path this
   round rewrote) — it bootstrapped a shell, ran prompt hooks, and indexed the repo with no
   panics or errors in `~/Library/Logs/simplewarp.log`, then shut down cleanly.

   Acceptance: `cargo check` (both feature sets, `--all-targets`, `-p integration`) all clean
   (0 errors). `cargo clippy -p warp --lib --all-targets --no-default-features --features
   simplewarp`: 15 warnings, down from 40 before this round — every one of the ~25 removed
   was an irrefutable-pattern/unreachable-pattern warning from this file; the 15 that remain
   are the already-deferred `tear_down_cloud_mode_setup_phase` cluster plus pre-existing,
   unrelated style lints. `./script/format --check` clean (after one format pass — it
   re-sorted a `use` line alphabetically). `cargo nextest run -p warp --lib
   --no-default-features --features simplewarp --no-fail-fast`: 5978/5986 pass, same 8
   pre-existing failures as every round in this thread, 0 new. 5 files changed, 864 lines
   deleted, 241 added.

3s. **The Warp Drive "join a discoverable team" section — DELETED, the smaller first cut into
   `app/src/workspaces/` (3m) that round scoped down to rather than attempted whole.** ~180
   lines in `app/src/drive/index.rs`, plus two dead getters in `user_workspaces.rs`.

   3m's own note named the starting point: "`joinable_teams`/the join-a-team-via-link flow may
   be smaller and worth checking first." Traced it the same way 3m traced `workspaces`/
   `current_workspace_uid`: `UserWorkspaces.joinable_teams` starts `Default::default()` (empty)
   from a local SQLite read a fresh install never populates, and every path that could ever
   refill it — `TeamClient::get_discoverable_teams`, `::workspaces_metadata` (feeding
   `on_team_left`/`on_workspaces_updated`) — is already stubbed to `local_only_error()` by 3e/3f.
   Same two independent reasons, same conclusion: `joinable_teams` is provably always `vec![]`
   in this fork, so `total_teammates_in_joinable_teams()` is provably always `0` and
   `num_joinable_teams()` is provably always `0`.

   **Traced that fact through every call site, not just one.** `drive/index.rs` had three:
   the `if total_teammates_in_joinable_teams() > 0 { insert JoinTeam section } else { insert
   CreateATeam }` guard building the sidebar's section list (always took the `else`), the
   `JoinTeam` section header's teammate-count string (unreachable once the section is never
   inserted), and a *second*, independent condition in `render_create_team_section` deciding
   the "Create team" button's style (`Accent` when `== 0`, `Secondary` otherwise) — always
   `Accent`. Missing that third site would have left a genuinely dead `else` branch standing
   right next to the deletion, the same "provably dead but not compiler-dead" shape 3h/3l/3p
   named repeatedly in this thread.

   **What went:** `DriveIndexSection::JoinTeam` (never constructed anywhere once its one
   insert site collapsed — confirmed with `grep -rn "DriveIndexSection::JoinTeam"` before
   touching it, same as every enum-variant deletion in this thread), its three match arms
   (section header, trash-index gate, render dispatch), the two `matches!` guards in
   `render_section` that existed only to hide it in the trash index and from anonymous users,
   `render_join_discoverable_team_section` (the whole "View team(s) to join / Or" widget,
   ~65 lines), `MouseStateHandles.join_team_button_mouse_state`, and
   `UserWorkspaces::{total_teammates_in_joinable_teams, num_joinable_teams}` (zero remaining
   callers once the three `drive/index.rs` sites above were gone — confirmed by grep, not
   assumed).

   **What deliberately stayed, and why this is a small cut, not the whole `3m` item.**
   `joinable_teams`, `update_joinable_teams`, `DiscoverableTeam`, `get_discoverable_teams`, and
   `UserWorkspacesEvent::{FetchDiscoverableTeamsSuccess, FetchDiscoverableTeamsRejected}` are
   untouched: `settings_view/teams_page.rs` (4,697 lines, the same file 3m flagged as "not a
   quick first cut") has its own independent "discoverable teams" list
   (`DiscoverableTeamState`, fed by the same `FetchDiscoverableTeamsSuccess` event) that is
   *also* provably always empty by the identical reasoning above — but touching it means
   tracing that file's own consumers, which is exactly the 10x-surface-area, dedicated-round
   work 3m deferred. This round stayed inside `drive/index.rs`, the one self-contained surface
   3m pointed at.

   Acceptance: `cargo check` (both feature sets, `--all-targets`, `-p integration`) clean (0
   errors, same 8 pre-existing warnings as 3r). `cargo clippy -p warp --lib --all-targets
   --no-default-features --features simplewarp` clean (0 errors). `./script/format --check`
   clean, no reformatting needed. `cargo nextest run -p warp --lib --no-default-features
   --features simplewarp --no-fail-fast`: 5978/5986 pass — identical count to 3r, same 8
   pre-existing failures, 0 new, 0 tests lost (no test named the deleted section directly).
   Built and launched `./target/debug/simplewarp`, the exact `DriveIndex::render`/
   `render_create_team_section` path this touched — clean startup, no panics. 2 files changed,
   171 lines deleted, 31 added.

3t. **The whole "discoverable teams" feature — DELETED, all the way down, not just the UI
   3s left standing.** ~365 lines net across 5 files: `settings_view/teams_page.rs` (the
   Settings → Teams "join a discoverable team" list, its own independent surface from 3s's
   Warp Drive one), `workspaces/user_workspaces.rs`, `workspaces/update_manager.rs`, and
   `server/server_api/team.rs`.

   3s deliberately left `joinable_teams`, `DiscoverableTeam`, `get_discoverable_teams`, and
   the `FetchDiscoverableTeams*` events standing, flagging `teams_page.rs`'s own discoverable-
   teams list as "provably dead by the same logic" but out of scope for that round. Traced it
   fully this round: `TeamsWidget::page_sections_for`'s `join_teams: bool` parameter (fed by
   `!discoverable_teams_states.is_empty()`, always false, same as 3s's `drive/index.rs` guard)
   meant `TeamsPageSection::JoinTeams` was never returned — collapsing `page_sections_for`
   down to two parameters made `JoinTeams`, `render_join_teams_section`,
   `render_team_discovery_section`, `render_single_team_in_team_discovery`,
   `render_join_team_button`, and `DiscoverableTeamState` all provably dead in one pass, the
   same cascade shape as every enum-collapse in this thread.

   **Followed the chain past the UI, since nothing else read it once teams_page.rs was gone.**
   `TeamsPageAction::JoinTeamWithTeamDiscovery` dispatches `UserWorkspaces::
   join_team_with_team_discovery`, which calls `TeamClient::join_team_with_team_discovery` —
   already stubbed to `local_only_error()` by 3e/3f, so even before this round the action could
   never succeed. With the one button that dispatched it gone, the whole chain (the action
   variant, `TeamsPageView::join_team_with_team_discovery`, `UserWorkspaces::
   {join_team_with_team_discovery, on_join_team_with_team_discovery}`, and the
   `JoinTeamWithTeamDiscoverySuccess`/`Rejected` events) had zero remaining callers — deleted.
   Same for the fetch side: `fetch_discoverable_teams` was called from exactly one place
   (`on_page_selected`, triggered whenever the Teams settings page opens) and called the now-
   pointless `get_discoverable_teams()` stub; deleted `fetch_discoverable_teams`,
   `on_fetch_discoverable_teams`, the `TeamClient::get_discoverable_teams` trait method +
   stub impl, and the trigger call in `on_page_selected`. `UserWorkspaces.joinable_teams`
   itself had already lost its only two readers in 3s (`total_teammates_in_joinable_teams`,
   `num_joinable_teams`), so once `update_joinable_teams`'s last caller (the fetch/join
   handlers above) went, the field was write-only with nothing left to write meaningfully to
   — deleted the field, `update_joinable_teams`, and the two `FetchDiscoverableTeams*` events,
   plus the two `update_manager.rs` call sites that fed it from workspace-metadata responses.

   **Where the line was drawn, deliberately.** `DiscoverableTeam` (the struct itself),
   `WorkspacesMetadataResponse.joinable_teams: Vec<DiscoverableTeam>` (the wire-format field),
   and `gql_convert.rs`'s `From<GqlDiscoverableTeamData> for DiscoverableTeam` conversion are
   untouched — that field sits inside a response struct shared with other still-live fields
   (`workspaces`, `feature_model_choices`, `user_purchase_policy`), so removing it means
   restructuring `WorkspacesMetadataResponse`/its GraphQL conversion, not deleting a dead
   consumer. Parsed-and-discarded costs nothing; the same boundary 3e/3f drew between "stub
   the transport" and "touch the wire-format types."

   `render_sub_header` (`teams_page.rs`) was a second-order orphan clippy caught only after
   the first pass compiled clean — its one caller was `render_single_team_in_team_discovery`,
   gone earlier in this same round. Same "check `--all-targets`, not `--lib`" lesson 3f named:
   the test file (`teams_page_tests.rs`) had five tests exercising `page_sections_for`'s
   deleted `join_teams` parameter directly (asserting both the `true` and `false` branches);
   `cargo check --lib` didn't see them, `clippy --all-targets` did. Rewrote each to assert only
   the surviving (`false`-equivalent) behavior rather than deleting them, since each still
   tests a real, live branch of `page_sections_for` (workspace resolution / admin status), not
   just the removed one.

   Acceptance: `cargo check` (both feature sets, `--all-targets`, `-p integration`) clean (0
   errors, same 8 pre-existing warnings). `cargo clippy -p warp --lib --all-targets
   --no-default-features --features simplewarp` clean (0 errors, back to 15 warnings — the
   same pre-existing set, confirming no fallout). `./script/format --check` clean after one
   pass (two stray blank lines from the deletions). `cargo nextest run -p warp --lib
   --no-default-features --features simplewarp --no-fail-fast`: 5978/5986 pass, identical
   count and same 8 pre-existing failures as 3s — 0 tests lost, since the five affected tests
   were rewritten rather than deleted. Built and launched `./target/debug/simplewarp` — clean
   startup, no panics. 5 files changed, 365 deletions, 9 insertions net.

3u. **`WorkspacesMetadataResponse.joinable_teams` and `DiscoverableTeam` — the wire-format
   plumbing 3t deliberately left standing, now dead for real.** 7 files, 43 deletions, 3
   insertions net.

   3t drew the line at `joinable_teams`/`DiscoverableTeam` because the field lived inside
   `WorkspacesMetadataResponse`, a struct shared with still-live fields (`workspaces`,
   `feature_model_choices`, `user_purchase_policy`) — "parsed-and-discarded costs nothing."
   Checked whether that was still true: grepped every `.joinable_teams` read site. There
   were none. `update_manager.rs`'s two consumers of `response.metadata` (`on_team_left`,
   `on_workspaces_updated`) only ever destructured `.workspaces` and `.user_purchase_policy`
   — the two call sites that fed `joinable_teams` into `UserWorkspaces` were exactly the ones
   3t deleted. The field had been write-only (constructed, never read) since that round; it
   just hadn't been traced back to the source.

   Deleted `WorkspacesMetadataResponse.joinable_teams: Vec<DiscoverableTeam>` and its
   `let joinable_teams = gql_user.discoverable_teams...` assembly in `gql_convert.rs`. With
   the field gone, `DiscoverableTeam` (`workspaces/team.rs`) had no remaining producer —
   deleted the struct and its `From<GqlDiscoverableTeamData>` conversion, and the now-unused
   `GqlDiscoverableTeamData` import. Test fixtures across `update_manager_tests.rs` and
   `user_workspaces_tests.rs` that built `WorkspacesMetadataResponse { joinable_teams: vec![],
   .. }` literals lost that line; each still constructs a valid response otherwise.

   **Where the line moved to, this time.** `warp_graphql::user::DiscoverableTeamData` and the
   `GqlUser.discoverable_teams` GraphQL query field are untouched — that's the actual wire
   boundary, generated from the schema in a sibling crate, not app-side dead code. The
   `discoverable_teams: vec![]` fixture field in `user_workspaces_tests.rs`'s `gql_user()`
   helper stays for the same reason: it satisfies a required field on that external struct,
   not app logic.

   Also fixed a stale doc comment in `drive/index.rs` — `render_team_section_header` said
   "Used for 1) create team 2) join discoverable teams sections," the second half untrue
   since 3s deleted that section.

   Acceptance: `cargo check` (both feature sets, `--all-targets`, `-p integration`) clean (0
   errors, same pre-existing warnings). `cargo clippy --all-targets --no-default-features
   --features simplewarp` clean (same pre-existing warnings, no new ones).
   `./script/format --check` clean, no reformatting needed. `cargo nextest run -p warp
   --no-default-features --features simplewarp --no-fail-fast`: 5986 run, 5978 passed, 8
   failed (identical to 3t's baseline), 4 skipped — 0 tests lost. Built
   `./target/debug/simplewarp` and launched it — clean startup, spawned its terminal-server
   child normally, no panics.

3v. **20 orphaned billing/plan-tier predicate methods on `Workspace`, `BillingMetadata`, and
   `UserWorkspaces` — not a single feature this time, a pile of leaf helpers nobody called.**
   2 files, 227 deletions, 6 insertions net.

   Different shape of search from 3r/3s/3t/3u: those traced forward from one stubbed network
   call to every consumer. This one went the other direction — for every `pub fn` in
   `workspaces/{user_workspaces,workspace,team}.rs`, grep the whole repo (not just app/src)
   for its name and flag anything appearing exactly once (the definition, nothing else).
   19 methods came back at exactly one occurrence, all pure predicates or getters with no
   side effects: `workspace.rs`'s billing/plan-tier questions (`is_on_build_plan`,
   `is_on_legacy_paid_plan`, `has_active_subscription`, `has_overages_used`,
   `has_failed_addon_credit_auto_reload_status`, `is_enterprise_pay_as_you_go_enabled`,
   `is_enterprise_auto_reload_enabled`, `is_usage_based_pricing_toggleable`, `can_be_deleted`,
   `are_overages_toggleable`, `are_overages_remaining`, `is_at_addon_credits_monthly_limit`,
   `would_addon_purchase_reach_limit`, `get_auto_reload_price_cents`) and
   `user_workspaces.rs`'s (`ai_allowed_for_team`, `is_active_ai_allowed`, `team_spaces`,
   `team_uids_across_all_workspaces`, `is_at_tier_limit_for_some_warp_drive_objects`,
   `update_ai_autonomy_policy_flag`). None called each other — deleting the whole batch in
   one pass didn't orphan anything further, unlike every prior round in this thread.

   These are `pub` items in a lib crate, so the compiler stays silent about them by
   construction (the 3g/4 lesson again) — nothing here would ever surface as a `dead_code`
   warning on its own. They read as **leftovers from earlier UI deletions** (billing pages,
   AI-overages surfaces, build-plan-migration UI cut in phases before this one) that removed
   the last caller without walking back to the leaf method itself — the inverse failure mode
   of what 3s/3t/3u kept catching.

   **Two second-order fixes, caught the same way 3f/3t's lesson predicts: `--all-targets`,
   not `--lib`.** Deleting `ai_allowed_for_team` orphaned the `CustomerType` import (`--lib`
   caught this one directly); deleting it from `user_workspaces.rs`'s top-level import list
   broke `user_workspaces_tests.rs`, which pulls `CustomerType` in transitively via
   `use super::*` and uses it directly in six assertions unrelated to the deleted method —
   `--lib` didn't see this, `--all-targets` did. Fixed by moving `CustomerType` into the
   file's existing `#[cfg(test)]` import block instead of dropping it, same fix shape as
   `AIAutonomyPolicy` (turned out `update_ai_autonomy_policy_flag` itself lived inside a
   `#[cfg(test)] impl UserWorkspaces` block — a test-only helper with zero test callers,
   not production code). `HashSet` (workspace.rs) and `Utc`/`AddonCreditAutoReloadStatus`
   (workspace.rs) went unused the ordinary way, once their sole callers were gone.

   Acceptance: `cargo check` (both feature sets, `--all-targets`, `-p integration`) clean (0
   errors, same pre-existing warnings). `cargo clippy --all-targets --no-default-features
   --features simplewarp` clean (0 errors). `./script/format` needed one pass (import
   re-sort), `--check` clean after. `cargo nextest run -p warp --no-default-features
   --features simplewarp --no-fail-fast`: 5986 run, 5978 passed, 8 failed (identical to 3u's
   baseline), 4 skipped — 0 tests lost or changed, since every deleted method was untested
   dead weight. Built `./target/debug/simplewarp` and launched it — clean startup, spawned
   its terminal-server child normally, no panics.

3w. **Same "grep every `pub fn` for zero callers" sweep, run over `settings_view/` instead
   of `workspaces/`.** 2 files, 39 deletions.

   Not a cloud-tracing round — `settings_view/` is 53 files, 60k lines, and most of it (AI
   settings, environments, MCP servers) has nothing to do with auth/teams/billing. Ran the
   same mechanical check as 3v anyway: every `pub fn` in the 37 non-test files, grepped
   whole-repo, flagged zero-callers-outside-definition. Excluded `#[test] pub fn` hits (two
   in `teams_page_tests.rs`) — those are discovered by the test harness, not called, so the
   sweep's premise doesn't apply to them.

   Two real finds, both dead **builder-style setters nobody used because the one place that
   would've used them sets the fields directly instead**:

   - `update_environment_form.rs`: `set_show_footer_cancel_button`, `set_field_max_width`,
     `set_field_spacing`, `set_description_height`, `set_show_repo_helper_text`,
     `set_show_share_with_team_controls` — six setters on `UpdateEnvironmentForm`, each just
     `self.field = value; ctx.notify();`. The only place configuring those same fields,
     `configure_for_orchestration_modal`, assigns them directly (`self.show_footer_cancel_button
     = true;` etc.) instead of calling its own type's setters, since it already has field
     access from inside the same `impl` block.
   - `custom_router_view.rs`: `router()` and `update_router()` on `CustomRouterView` — both
     already carried `#[allow(dead_code)]`, i.e. someone had already found them dead and
     silenced the warning instead of deleting them (the 3g/4 pattern of admission-not-removal).
     `CustomRouterView` itself is alive (constructed in `warp_agent_page.rs`), just these two
     accessor/mutator methods on it were never called.

   Smaller haul than 3v (8 methods vs. 20) — `settings_view/` isn't primarily cloud-adjacent,
   so most of its dead-code surface was already gone by the time this thread reached it, or
   was never cloud-shaped to begin with. Confirms the sweep generalizes as a mechanical check
   to run over any module, not something specific to `workspaces/`.

   Acceptance: `cargo check` (both feature sets, `--all-targets`, `-p integration`) clean (0
   errors, same pre-existing warnings, no new unused-import fallout). `cargo clippy
   --all-targets --no-default-features --features simplewarp` clean (0 errors).
   `./script/format` needed one pass (stray blank line), `--check` clean after. `cargo
   nextest run -p warp --no-default-features --features simplewarp --no-fail-fast`: 5986
   run, 5978 passed, 8 failed (identical baseline), 4 skipped — 0 tests lost. Built
   `./target/debug/simplewarp` and launched it — clean startup, spawned its terminal-server
   child normally, no panics.

3x. **Same sweep over `terminal/`'s 48 top-level files (66k lines) — plus, for the first
   time, the compiler's own `dead_code` lint as a second, independent source of candidates.**
   12 files, 589 deletions, 2 insertions net; 4 tests deleted, 1 test trimmed.

   `drive/` came up completely empty (already thoroughly swept by 3s) — confirms the method
   doesn't always find something; a clean module is a real result, not a missed search.
   `terminal/` is too large (314k lines including subdirectories) to sweep whole, so this
   round scoped to the 48 top-level files only, deliberately leaving the subdirectories
   (`model/`, `view/`, `input/`, etc.) for a later pass.

   **This module is not cloud-adjacent the way `workspaces/`/`settings_view/` were** — it's
   the core terminal engine, actively developed, not something this fork is stripping. That
   changes the risk calculus: a zero-grep-hit `pub fn` here is far more likely to be either
   (a) ordinary software-entropy leftovers unrelated to this project's mission, or (b) an
   entry point into a still-substantial live subsystem that just happens to have no *current*
   caller. Traced every candidate's callees before deleting, and explicitly **skipped two**
   that the grep sweep alone would have flagged: `Input::process_remote_edits` (CRDT-based
   remote-block-edit sync — `latest_buffer_operations`, `CrdtOperation`, and
   `apply_remote_operations` are all still live and tested in `editor/`, so this reads as one
   orphaned entry point into machinery that still serves other callers, not a dead feature)
   and `TerminalView::apply_external_input_mode_update` (doc comment says "e.g., session
   sharing" — grepping that phrase hits 44 files across `ai/agent_sdk/`, `pane_group/`, and
   more; far too broad and too active to characterize as dead from a name match). Also
   skipped `TerminalView::set_show_pane_accent_border`: it's the *only* caller of
   `PaneConfiguration::set_show_accent_border` in `pane_group/pane/mod.rs`, whose
   `show_accent_border` field is still read at render time and whose update event still has a
   subscriber — the border-rendering machinery is alive, just permanently inert since nothing
   currently flips it on. Deleting the setter would mean reaching into another module's live
   rendering path, not removing an isolated leaf; left it standing rather than deciding that
   scope question by omission.

   **Second source: `cargo check`'s own `dead_code` lint**, read directly rather than
   re-derived by grep. `pub(crate)`/private items are exempt from nothing — the compiler
   already proves them dead with certainty grep can only approximate. One `--lib` warning
   block named eight `TerminalView` methods at once (`clear_queued_command_in_flight`,
   `has_queued_command_in_flight`, `maybe_drain_queue_after_promptless_setup`,
   `ensure_ambient_agent_view_model`, `tear_down_cloud_mode_setup_phase`,
   `should_suppress_ambient_setup_input_sync`, `tag_agent_in`, `tag_agent_out`), plus
   `tear_down_active_setup_command_group` in a sibling file — all genuinely dead in a real
   build. Left three lookalikes alone that appeared only in the `--all-targets` (test) build's
   warning set, not the plain `--lib` one — `should_show_wasm_conversation_details_panel`,
   `should_show_wasm_pane_header_details_button`, `is_orchestration_child_live_unavailable_for_test`
   — meaning something in production code does call them, just from a path that
   `#[cfg(not(test))]`-gates itself out of the test build; not dead in the shipped binary.

   **Cascades, one layer at a time, same "confirm every downstream consumer" discipline as
   3s–3u:** `tear_down_cloud_mode_setup_phase` was the only caller of
   `tear_down_active_setup_command_group`, so both went together. Deleting
   `ensure_ambient_agent_view_model` orphaned the `Event::AmbientAgentViewModelCreated` variant
   it was the sole emitter of (zero subscribers) — deleted that too. Deleting
   `should_suppress_ambient_setup_input_sync` orphaned `SetupCommandState::
   should_suppress_input_sync_for_current_group`, its only caller — deleted both. Two stale
   doc-comment references to the deleted `ensure_ambient_agent_view_model` (in
   `wire_ambient_agent_view_model`'s doc) got cleaned up in the same pass.

   **The one real mistake this round, and the fix.** Assumed "flagged only under `--lib`, not
   `--all-targets`" meant "provably dead in production" and deleted on that basis without
   separately checking test files — exactly backwards from the 3f/3t lesson ("verify with
   `--all-targets`, not `--lib`"), and it bit immediately: `--all-targets` came back with 7
   compile errors. `maybe_drain_queue_after_promptless_setup`,
   `should_suppress_input_sync_for_current_group`, and a `restore_cloud_followup_input_after_
   upload_failure`/`reset_after_cloud_followup_submission`/`freeze_input_in_loading_state`/
   `unfreeze_agent_input` cluster all had dedicated unit tests calling them directly, with zero
   production callers for any of them. Since every one of these methods' only caller anywhere
   was a test built specifically to exercise that one method in isolation — not a shared test
   helper covering other still-live behavior — deleted the method and its dedicated test
   together in each case (`restore_cloud_followup_input_after_upload_failure_restores_prompt`,
   `unfreeze_agent_input_does_not_clear_buffer`, `promptless_setup_complete_drains_queued_
   prompt`, `promptless_setup_complete_with_initial_prompt_does_not_drain_queue` — 4 tests
   gone), same call as 3t made for a dead branch's dedicated tests. One test
   (`setup_command_groups_track_running_group_independently`) mixed assertions about the now-
   dead method with assertions about still-live `is_running`/`finish_group` behavior — trimmed
   to keep only the live half, same as 3t's `page_sections_for` rewrites. Chased the resulting
   second-order orphans the same way: `promptless_cloud_spawn_request` (a test-only fixture
   builder, orphaned once both tests that used it were gone) and the now-unused `TextColors`
   import in `input.rs`.

   Acceptance: `cargo check` (both feature sets, `--all-targets`, `-p integration`) clean (0
   errors; `warp (lib)` warnings dropped from 8 to 5 — exactly the methods deleted from that
   list — and the three wasm/test-only lookalikes deliberately left in place). `cargo clippy
   --all-targets --no-default-features --features simplewarp` clean (0 errors).
   `./script/format` needed one pass (five stray blank lines from the deletions), `--check`
   clean after. `cargo nextest run -p warp --no-default-features --features simplewarp
   --no-fail-fast`: 5982 run (4 fewer than 3w's 5986 baseline, matching the 4 deleted tests
   exactly), 5974 passed, 8 failed (identical pre-existing set), 4 skipped — 0 unexpected
   losses. Built `./target/debug/simplewarp` and launched it — clean startup, spawned its
   terminal-server child normally, no panics.

3y. **Same sweep over `terminal/`'s subdirectories — `model/`, `view/`, `input/` (87k lines,
   198 files) — where "core, actively-developed engine" made false positives a real risk, not
   a theoretical one.** 21 files, 721 deletions, 4 insertions net.

   `drive/` (3x) had come up empty; these three subdirectories came back with 51 grep
   candidates plus one compiler-flagged item (`BlockLifecycleCoordinator::reset_unknown`) —
   the largest single-round haul in this thread. Two shapes of finding:

   **Genuinely cloud-shaped dead code**, same story as every round back to 3r — leftovers
   from features this fork doesn't have a backend for: `Block::is_scrollback_block_for_shared_
   session` ("so viewers can restore the active prompt" — collaborative session viewing, not
   the ambient-agent "session sharing" concept 3x correctly left alone), `AmbientAgentViewModel::
   {reset_for_new_cloud_prompt, should_show_followup_progress}`, `TerminalModel::{is_receiving_
   agent_conversation_replay, set_is_receiving_agent_conversation_replay}` (a getter/setter/field
   trio that survived the mechanical count *because the method name collides with its own field
   name* — grepping the identifier hits the field declaration, its initializer, and the method
   body, pushing the raw count over the sweep's threshold even with zero real callers; caught
   only by separately checking for the `.method()` call syntax specifically — a blind spot in
   the counting heuristic worth remembering for any future sweep), `Input::reset_after_cloud_
   followup_submission` (its already-compiler-flagged sibling `restore_cloud_followup_input_
   after_upload_failure` came along with it — the rest of that cluster is the round's mistake,
   see below), and `terminal/input/slash_commands/mod.rs::record_autodetection_toggle_from_
   slash_command`, an orphan left behind by the `warp_tui` crate deletion (phase 4, item 1,
   done several sessions ago) that nobody had swept back for until now.

   **Ordinary software entropy, unrelated to this fork's mission** — the majority of the haul.
   A "find in block" sub-feature (`bottommost_match`, `topmost_match`, `GridIndex`,
   `calculate_filtered_output_grid_matches`, `set_filtered_output_grid_matches`,
   `reset_output_grid_matches`, `number_of_command_grid_matches`, `number_of_prompt_and_
   command_grid_matches`, `active_or_prev_row`) superseded by a newer implementation
   (`terminal/find/`'s `BlockFindRenderData`) that computes matches a different way, leaving
   the old per-`Block` filtering machinery to just sit there. A `scan_block_for_secrets` /
   `scan_full_block_for_secrets` / `for_each_block_grid`'s-sibling-call / `BlockGrid::
   scan_full_grid_for_secrets` chain, three levels deep, that turned out to have a live cousin
   (`GridHandler::scan_full_grid_for_secrets`, called independently from a constructor) at the
   bottom — confirmed each link stayed alive or died on its own merits rather than assuming the
   whole chain shared one fate. Plus a long tail of one-off leaf getters/setters/constructors
   with a still-live sibling doing the real work next to them (`is_any_session_remote` /
   `is_session_remote` next to the still-used `is_local()`, `command_and_output_with_secret_
   obfuscated` next to still-used `command_with_secrets_obfuscated`, `to_within_block_point`
   next to a still-widely-used `WithinBlock` type, etc.) and `block_onboarding/util.rs::
   render_input_row` — a "create team" inline-row renderer for the onboarding block, dead
   because team creation itself is cloud-gated, with `CREATE_BUTTON_WIDTH` (private, sole use)
   going with it while `render_skip_button`/`name` (still called directly from `onboarding_
   prompt_block.rs`) stayed untouched.

   **Two lines deliberately not crossed**, decided the same way 3x drew its boundaries:
   `contains`/`contains_cell` on `SelectionRange` (`model/selection.rs`) — `contains_cell`
   was a confirmed-dead flagged candidate and got deleted, but whether its sibling `contains`
   is now *also* orphaned couldn't be verified: `contains` is too generic a word for grep to
   distinguish real callers from every other type's same-named method in the codebase (`Range`,
   `HashSet`, etc.). Left it standing rather than guess. And `Blocks::set_should_hide_output_
   grid`, deleted alongside its more clearly-boundaried sibling `should_hide_command_grid`
   setter — but its own getter, `should_hide_output_grid()`, was confirmed to have other live
   readers, so this one field permanently defaults to `false` now rather than going fully dark
   (same "leftover always-false branch" shape as 3u's `joinable_teams`, scoped narrower this
   time since only the mutator side was provably dead).

   **The mistake, caught before it shipped.** Deleted `Input::{freeze_input_in_loading_state,
   freeze_input_in_loading_state_with_text}` and `Input::unfreeze_agent_input` as part of the
   cloud-followup cluster on the same zero-production-caller basis as the round's other finds
   — but `--all-targets` immediately surfaced two dedicated tests
   (`restore_cloud_followup_input_after_upload_failure_restores_prompt`,
   `unfreeze_agent_input_does_not_clear_buffer`) exercising exactly this code, each testing the
   method in isolation with zero production caller for any of it — same shape as 3x's four
   test-only orphans, so deleted each test alongside its method rather than restoring the
   method. Separately, `BlockLifecycleCoordinator::reset_unknown` (compiler-flagged, sourced
   independently of the grep sweep) looked like the same pattern but wasn't: its two callers in
   `terminal_model_tests.rs` (`command_finished_recovers_unknown_started_block_with_real_exit_
   code`, `recovery_advances_finished_active_block_without_republishing_completion`) use it as
   test **setup** to force a specific starting state before exercising real, still-live
   `FeatureFlag::TerminalLifecycleRecovery` recovery behavior — not a dedicated test *of*
   `reset_unknown` itself. Restored it rather than deleting the method or the tests. The
   distinguishing question, going forward: is the test *about* this function, or does it just
   *use* this function to set up a test about something else?

   Acceptance: `cargo check` (both feature sets, `--all-targets`, `-p integration`) clean (0
   errors; `warp (lib)` warnings unchanged at 5, `reset_unknown` present exactly once as
   expected — `--lib` only, since its only callers are tests). `cargo clippy --all-targets
   --no-default-features --features simplewarp` clean (0 errors). `./script/format` needed one
   pass (six stray blank lines), `--check` clean after. `cargo nextest run -p warp
   --no-default-features --features simplewarp --no-fail-fast`: 5982 run, identical to 3x's
   baseline — the two tests caught by the mistake-and-fix above were deleted in the same round
   they were added to the diff, so the net test count never moved. 5974 passed, 8 failed
   (identical pre-existing set), 4 skipped. Built
   `./target/debug/simplewarp`, launched it, and left it running long enough to confirm a live
   terminal session stayed stable (not just clean startup) — no panics, terminal-server child
   spawned normally.

3z. **Same sweep over `ai/blocklist/` (123k lines, 100+ files) and `workspace/` (68k lines,
   70 files), plus the compiler's `dead_code` lint as the second source again.** 27 files,
   851 deletions, 10 insertions net; 5 tests deleted.

   `workspace/` came back nearly empty — 5 grep candidates, 2 of which were false positives
   (see below), for 4 real deletions. `ai/blocklist/` is where the haul was, and it was
   overwhelmingly one shape: **the session-sharing / shared-viewer leftovers that 4e's
   41k-line deletion left standing on the AI side.** `ResponseStreamId::for_shared_session`
   ("when viewing the same shared session from multiple terminals"), `BlocklistAIActionModel::
   {mark_action_as_remotely_executing, apply_finished_action_result}` and the private
   `maybe_sync_view_only_documents_with_local_model` they exclusively drove (all three
   documented as "agent session sharing" and all three early-returning on `!self.is_view_only`),
   `AIHistoryModel::mark_terminal_surface_as_ambient_agent_session_view`, and — from the
   compiler, not the grep — the `IdleTimeoutSender` "debug window refresh" mechanism in
   `agent_sdk/driver.rs`. The rest is ordinary entropy: builder setters whose one would-be
   caller assigns the fields directly (3w's exact pattern, twice: `CTAButton::with_telemetry`,
   `PendingHandoff::with_environment_required`, both already wearing `#[allow(dead_code)]`),
   thin `send_*` wrappers on `BlocklistAIController` whose `_internal` stayed alive through a
   sibling, and a long tail of leaf getters sitting next to the still-live sibling doing the
   real work (`contains_actions` next to `contains_action`, `has_unfinished_actions` next to
   `has_unfinished_actions_for_conversation`, `get_pending_action_by_id` next to
   `get_pending_action`).

   **Three full collapses rather than leaf removals**, where the dead item was the shape's
   only entry point:

   - `CTAButton::open_url` was the only constructor of `CTAButtonAction::OpenUrl`, so the
     variant, its `Clone` arm, and both `ctx.open_url(&url)` match arms in `launch_modal/mod.rs`
     went with it — the 4g "deletion in disguise" shape. `telemetry_event` went the same way:
     the field was `#[allow(dead_code)]`, its only writer was the dead `with_telemetry`, and
     no reader existed, so the field and its four `None` initializers went too.
   - `mark_terminal_surface_as_ambient_agent_session_view` was the only writer of
     `ambient_agent_terminal_surface_ids`, whose two readers were both "Skip shared ambient
     agent sessions" guards — permanently no-ops once the writer is gone. Deleted the field
     and collapsed both guards rather than leaving an always-empty set behind (3u's
     `joinable_teams` precedent).
   - `CLISubagentTarget` — the whole read-only snapshot type — was produced only by
     `target_for_block_in_model`, itself reached only by `active_target` and `target_for_block`,
     both dead, and nothing anywhere consumed the type. Its `latest_instruction` field's only
     writers (`set_latest_instruction`, `restore_latest_instruction`) were dead too, which
     made `CLISubagentEvent::UpdatedInstruction` unemittable; deleted the variant and its four
     one-line no-op `|`-chain arms across `terminal/{view.rs, input/models/view.rs,
     view/queued_prompts_panel.rs}`. Reaching into `terminal/` was the scope call here, and it
     went the opposite way from 3x's `set_show_pane_accent_border`: there the machinery was
     live and only the trigger inert, here the machinery itself was the dead thing.

   **A new blind spot in the counting heuristic, hit three times: a function passed as a
   value, not called.** `Self::handle_network_status_event` in a subscription registration,
   `diff_type.map(changed_lines_from_op)`, and `.is_some_and(UserTakeOverReason::is_stop)` all
   have zero `name(` call-syntax hits and so were flagged, and all three are live. Alongside
   3y's method-name-collides-with-field-name case, that's two ways the mechanical count lies —
   both caught only by reading every candidate's grep output rather than trusting the counter.

   **The round's mistake, and it was the same one 3x recorded.** Concluded `arm_refreshable`
   was test-only from a `grep | head -10` whose real production caller was below the cut,
   deleted it, and the compiler said so immediately. The *reading* was salvageable, though:
   its one caller, `arm_debug_window`, ends in `if self.debug_window_refresh_installed { … }`
   guarding a block whose entire body is `let _ = idle_timeout;` under the comment "Without
   session sharing there is no viewer input to refresh the debug window on" — a stub someone
   already knew was inert. So `arm_refreshable`'s call site became a plain `end_run_after`,
   and the `pending` field, `refresh`, the flag, and the stub all went. Signal worth keeping:
   the compiler flagging *one* method of a pair is not evidence the sibling is dead, and a
   truncated grep is not a caller search.

   **Deliberately left standing: `BlocklistAIActionModel::set_view_only`.** It is
   zero-caller and it is the only writer of `is_view_only` — but `is_view_only` has readers in
   four files (`action_model.rs` ×3, `block.rs`, `inline_action/code_diff_view.rs`,
   `inline_action/requested_command.rs`), so collapsing it is a whole-feature removal of
   shared-session viewer mode, not a leaf. That is the next round's work, and doing half of it
   would leave exactly the "leftover boolean keeps the old shape compiling" state 4e warned
   about.

   5 tests deleted, all of them tests *of* the deleted subject with no production caller (3y's
   distinguishing question): `restore_fired_row_reinserts_removed_row_for_retry`, the three
   `debug_window_refresh_*` / `cancelling_the_idle_timeout_*` tests, and
   `share_session_failed_includes_reason` — the last surfaced only by `--all-targets`, the
   `--lib` build having called the same code clean.

   Acceptance: `cargo check` (`--lib` and `--all-targets`, `simplewarp` feature set) clean —
   0 errors, and the warning set back to the exact pre-round baseline (`auth_manager`'s
   never-read field, `reset_unknown` which 3y kept on purpose, and the three
   `--all-targets`-only `wasm`/orchestration lookalikes 3x correctly left alone). Four rounds
   of unused-import fallout from the deletions were cleaned as they appeared. `cargo clippy
   --all-targets`: 0 errors. `./script/format` clean. `cargo nextest run -p warp
   --no-default-features --features simplewarp --no-fail-fast`: 5977 run, 5969 passed, 8
   failed, 4 skipped — the same 8 pre-existing failures (six `cloud_preferences_syncer`, two
   `workspace::view` tools-panel-after-signup), 0 new. Built `./target/debug/simplewarp` and
   launched it: alive and clean, no panics, no error output.

3aa. **The 8 "pre-existing failures" every round since 3u has reported as baseline — GONE.
   The suite is green.** 2 files, 32 insertions, 0 deletions.

   Eleven rounds recorded "8 failed (identical pre-existing set)" and moved on, treating the
   number as a constant of the environment. It was not: all 8 have **one** root cause, and it
   is this fork's own doing. `crate::features::warp_account_available()` is
   `!cfg!(feature = "local_only")`, and it gates two things the 8 tests depend on:

   - `Workspace::compute_left_panel_views` ANDs it into the Warp Drive tab's visibility, so in
     a `local_only` build `left_panel_views` can never contain `ToolPanelView::WarpDrive` —
     `test_tools_panel_warp_drive_toggle_updates_available_views` fails on its *first* assert,
     and `test_tools_panel_preferences_activate_after_signup_and_ai_enablement` (whose whole
     subject is account-first onboarding across a signup) fails the same way.
   - `initialize_cloud_preferences_syncer` ANDs it into `sync_enabled`, so the syncer never
     applies a cloud value to local. All six `cloud_preferences_syncer` failures are the same
     assertion in different clothes: "cloud value should have been applied to local".

   Both gates were *added by this project* and both carry a comment explaining why (the
   toolbelt drops an icon that could only ever show a sign-in wall; the syncer would otherwise
   warn about a missing personal drive on every launch). The production behaviour is right.
   The tests were simply never re-pointed at it.

   **Fixed with `#[cfg(not(feature = "local_only"))]` on the 8 tests, mirroring the production
   gate exactly** — not deleted. These are not dead-feature tests like the ones rounds 3r–3z
   removed: their subject still exists upstream and in the default feature set, and the
   hash-comparison logic the six syncer tests cover is real, non-cloud logic worth keeping
   under test. Rerouting them around the gate was considered and rejected: the syncer's
   constructor doc says it "is the only entry point used to construct the syncer at app
   startup; production code in `lib.rs` and end-to-end tests both call it so they exercise the
   same code path", and calling `CloudPreferencesSyncer::new` directly would skip the very
   hash computation `test_first_launch_with_no_stored_hash_lets_cloud_win` exists to check.

   **Verified in both directions, because a cfg-gate that hides a real failure is worse than
   the failure.** `cargo nextest run -p warp --lib` (default features, where the feature
   exists) with a filter naming all 8: **8 run, 8 passed**. So the gate excludes them only
   where their premise is false.

   Acceptance: `cargo nextest run -p warp --no-default-features --features simplewarp
   --no-fail-fast`: **5969 run, 5969 passed, 0 failed, 4 skipped** — the first fully green run
   in this thread. The 8 gated tests still pass under default features. `cargo clippy
   --all-targets` (simplewarp): 0 errors. `./script/format` clean.

   Every earlier round's "8 failed (identical baseline)" acceptance line should now read as
   0 failed; the baseline they were comparing against was a bug, not a floor.

### Phase 4, step 4 — the cloud crates, in order

**The "~147 files" risk note at the top of this document is stale.** After 3e/3f, the *direct*
importers of each crate are: `warp_server_client` 12 files, `warp_server_auth` 12 (7 of them
inside `warp_server_client` itself), `firebase` 8 (4 inside those two crates), `warp_graphql` 96,
`cloud_objects` 139. The first three are a tight cluster that falls over once the first one goes.

**But the direct-import count is not the job.** `warp_server_client` owns `BaseClient`, the
transport `ServerApi` derefs to, and `ServerApiProvider` — the singleton that hands out ten
client trait objects — is reached from **126 files**. Every one of those trait impls is already a
wall (`local_only_error()`), so this is the same delete-a-layer-of-walls-and-their-callers work
as rounds 3r–3z, at ten times the size. It is a multi-round job, top-down:

1. one client trait at a time — impl, trait, and the callers that only existed to reach it;
2. then `ServerApi`, `ServerApiProvider`, `BaseClient`;
3. then the three crates, then `warp_graphql` (which cannot go until the trait signatures do).

Smallest first, by impl size and caller count: `ManagedMcpClient` (33 lines, 2), `FactoryClient`
(52, 7), `BlockClient` (58, 1), `ManagedSecretsClient` (69, 2), `WorkspaceClient` (90, ?),
`IntegrationsClient` (167, 4), `TeamClient` (216, ?), `ObjectClient` (254, ?),
`HarnessSupportClient` (458, 8), `AIClient` (2,226 lines, 67 call sites — last).

4l. **`BlockClient` and `ManagedMcpClient`, the two smallest — DONE.** 18 files, 1,613
   deletions, 64 insertions; 15 tests removed, 4 rewritten.

   **`BlockClient` was one caller deep and that caller was a whole settings page.** Its only
   consumer was `ShowBlocksView`, the "Shared blocks" page — 807 lines whose two data calls
   (`blocks_owned_by_user`, `unshare_block`) both return `local_only_error()`. The page was
   already *hidden* in a `local_only` build (`SettingsSection::needs_warp_account` drops it from
   the sidebar), so this round is the **delete** step of the plan's gate → hide → delete. Gone
   with it: the `SettingsSection::SharedBlocks` variant and its Display/slug/from-slug arms, the
   `SettingsPageViewHandle` variant and its two match arms, the `ViewSharedBlocks` custom action,
   its menu item, its keybinding, and `server/block.rs` (the 40-line wire type only that page and
   `server_api/block.rs` used).

   **One live detail the mechanical removal would have broken.** The Scripting page is inserted
   into the sidebar at the *index of* SharedBlocks (`position(…).unwrap_or(nav_items.len())`), so
   deleting SharedBlocks would silently have moved Scripting to the bottom of the list. Re-anchored
   on `Privacy`, the item SharedBlocks sat above.

   **`ManagedMcpClient` was not a wall to delete but a branch to collapse.** It threaded through
   four functions to reach `resolve_mcp_specs_with_local_uuids`, whose four spec kinds now split
   cleanly: `Uuid` already installed locally and `Json` are *local* paths and survive untouched;
   `Uuid` not installed locally could only be resolved by asking the server, so it now fails with
   `ManagedMcpResolutionFailed` directly; and `WellKnown` was already best-effort — a disconnected
   integration skipped the server rather than failing the run — so with no server to ask, every
   one skips. Both outcomes are what the `local_only_error()` call produced anyway; the collapse
   just stops pretending there is a request. `installations_from_managed_client_config_json`
   (parser for a *server-supplied* config) and `ephemeral_mcp_installation_id` were orphaned by
   that and went too, taking `ParsedTemplatableMCPServerResult::variable_values` — a field whose
   sole reader was the deleted parser, and which already wore `expect(dead_code)` on wasm.

   Of the 6 resolver tests, 4 were **rewritten** rather than deleted — the resolver still
   dispatches on spec kind, and "a skipped well-known spec does not drop the others" is still a
   real guarantee. The 2 that asserted a successful server round-trip went, along with the 12
   tests of the deleted parser and one superseded failure-path test.

   **Fallout from 3aa, caught here rather than there.** Gating those 6 syncer tests orphaned their
   helpers (`drain_sync_queue`, the hash read/write pair, `FakeObjectClient` and its module) under
   `local_only` — 11 new warnings that 3aa's acceptance missed because it checked clippy *errors*
   and the test result, not the warning set. All now carry the same `not(feature = "local_only")`
   gate. The lesson 3z already recorded, re-learned: compare the warning set, not just the exit code.

   Acceptance: `cargo check --all-targets` (simplewarp) back to the exact 2+1 baseline warnings.
   Clippy 0 errors, format clean. `cargo nextest run` (simplewarp): **5955 run, 5955 passed, 0
   failed**. Default features, the tests touched here plus the 8 that 3aa gated: 28 run, 28
   passed. Built and launched `./target/debug/simplewarp` — alive, no output, no panics.

   The provider is down to 8 client getters.

4m. **`FactoryClient` — the runner surface, all of it — DONE.** 17 files, 1,510 deletions,
   50 insertions; 5 files removed outright.

   `FactoryClient` is runner CRUD (`getRunners`/`upsertRunner`/`deleteRunner`), and runners are
   *cloud machines*. Its cargo feature `cloud_agent_runners` is in `default` but **not in
   `simplewarp`**, so the whole surface was already gated off here — hide → delete again, and a
   reminder to check the feature set before assuming a caller is live.

   **The `warp runner` CLI subcommand existed only to call it** and went whole: `agent_sdk/runner.rs`
   (584 lines), `warp_cli/src/runner.rs` (227), the `CliCommand::Runner` variant, its dispatch,
   its `as_str_for_tracing` arm, and four `CliTelemetryEvent::Runner*` variants with their
   payload/name/description/enablement arms. `FeatureFlag::CloudAgentRunners` itself **stays** —
   `ai/agent/api/impl.rs` still reads it.

   **Two collapses rather than leaf removals.** The runner *picker* survives (it is orchestration
   UI, and `RunAgentsExecutionMode::Remote` is a 175-reference feature that is not this round's
   business), but nothing can ever populate it again, so the runner-list parameter was threaded
   out of `create_runner_picker` → `populate_runner_picker` → `build_runner_snapshot`. The
   snapshot now offers exactly one row, "Use environment default", and a config that still names
   a `runner_id` selects *nothing* — deliberately, since selecting the default row would
   misreport a config rather than describe it. Second, `runner_display.rs` (174 lines + 148 of
   tests) went entirely: with the lookup map permanently empty, `resolve_run_platform` could only
   ever return `RunPlatform::Default`, so the details panel's platform row is now a direct
   "Linux · x86-64" with the same precedence check inlined — a run that names a runner hides the
   row instead of guessing.

   **The round's mistake: a deleted clap subcommand has string-keyed references the compiler
   cannot see.** `cargo check` was clean and clippy was clean, and then **140 `workspace::view`
   tests failed at runtime** with `Command 'runner' is undefined` — `mut_subcommand("runner", …)`
   in `warp_cli/src/lib.rs` and `find_subcommand_mut("runner")` in its tests. Grepping the
   *variant* name found nothing; only grepping the literal `"runner"` did. Worth remembering for
   every remaining CLI-facing deletion in this thread: **after removing a `CliCommand` variant,
   grep the lowercase subcommand string, not just the type.** (The help test was re-pointed at
   `agent` rather than deleted — its subject is that `WARP_API_KEY` appears in subcommand help,
   which is still true.)

   Acceptance: `cargo check --all-targets` (simplewarp) back to the 2+1 baseline warnings; clippy
   0 errors; format clean. `cargo nextest run` (simplewarp): **5939 run, 5939 passed, 0 failed**.
   Built and launched `./target/debug/simplewarp` — alive, no output, no panics.

   The provider is down to 7 client getters.

4n. **Attempted `HarnessSupportClient`; reverted. The easy clients are gone — everything left
   on this chain is a feature round.** No code change; this entry is the survey, so the next
   attempt does not repeat it.

   **The ranking in 4l was by impl size, and impl size is the wrong axis.** Ranking by what a
   deletion actually costs:

   | Client | Impl | Getter call sites | What it is actually attached to |
   | --- | --- | --- | --- |
   | ~~`BlockClient`~~ | 58 | 1 | the Shared blocks settings page — **done, 4l** |
   | ~~`ManagedMcpClient`~~ | 33 | 2 | four lines of spec resolution — **done, 4l** |
   | ~~`FactoryClient`~~ | 52 | 7 | the `warp runner` CLI + the runner picker — **done, 4m** |
   | `WorkspaceClient` | 90 | 1 | `app/src/workspaces/` — 40 uses in `user_workspaces.rs` alone |
   | `TeamClient` | 216 | 2 | same, plus `TeamUpdateManager` across 19 files |
   | `ManagedSecretsClient` | 69 | 4 | managed secrets — 30 app files + a 2,013-line crate |
   | `IntegrationsClient` | 167 | 10 | agent-SDK integrations (Slack/Linear connections) |
   | `HarnessSupportClient` | 458 | 8 | the cloud-agent harness reporting path — see below |
   | `ObjectClient` | 254 | 5 | `cloud_object`, already listed as a refactor not a deletion |
   | `AIClient` | 2,226 | 104 in 49 files | last, by construction |

   A one-call-site getter (`WorkspaceClient`) turned out to be the *most* expensive of the small
   ones: its single caller is `UserWorkspaces::new`, and `app/src/workspaces/` is what 3m already
   surveyed and set aside as remote_server-scale.

   **`HarnessSupportClient` looked like a repeat of 4m and is not.** Both its gates are off in
   `simplewarp` (`agent_harness` and `oz_handoff` are in `default` only), and it has the same
   CLI-subcommand shape — `warp harness-support` (327 + 133 lines) plus
   `driver/checkpoint_coordinator.rs` (634 + 869 of tests). Deleting those six files compiled
   down to **14 files importing `server_api::harness_support`**: `driver/snapshot.rs` (1,916
   lines, 9 refs), `driver/harness/{mod,claude_code,codex,gemini}.rs`, `artifact_upload.rs`,
   `presigned_upload.rs`, `server_api/ai.rs`. The client is threaded through the whole
   third-party-harness snapshot-and-resume path, which is a feature on the scale of 4e's session
   sharing, inside a 40,574-line `agent_sdk/`. Reverted rather than pushing a half-reasoned diff
   through fourteen files of live-looking harness code.

   **What this means for the chain.** `warp_server_client` cannot fall until `ServerApi` stops
   implementing these traits, and each remaining trait is now a planned feature deletion, not a
   sweep. The cheapest genuine next step is probably `IntegrationsClient` (10 call sites, all in
   `agent_sdk/` + `settings_view/update_environment_form.rs`); the rest should be scheduled the
   way 4e was, one feature at a time, rather than treated as client-trait cleanup.

4o. **The `warp integration` CLI, and three of `IntegrationsClient`'s seven methods — DONE.**
   14 files, 1,409 deletions, 10 insertions; 3 files removed outright.

   4n called `IntegrationsClient` the cheapest next step. It is — but only its CLI half. The
   trait's seven methods split into two groups that do not come out together:

   - **The `warp integration` surface (gone).** `create_or_update_simple_integration` and
     `list_simple_integrations` were reached from exactly one place, `agent_sdk/integration.rs`
     (529 lines) and its renderer `integration_output.rs` (355). Both calls return
     `local_only_error()`, so all three of `warp integration create|update|list` were dead.
     Gone with them: `warp_cli/src/integration.rs` (105), the `CliCommand::Integration` variant,
     its dispatch and wasm arms, its `as_str_for_tracing` and `is_read_only` arms, three
     `CliTelemetryEvent::Integration*` variants with their four arms each, the
     `integration_command` cargo feature and `FeatureFlag::IntegrationCommand`, and six parse
     tests in `lib_tests.rs`.
   - **The GitHub/environment half (stays).** `check_user_repo_auth_status` gates the repo
     arguments of `warp environment` and `warp agent skills`; `get_user_github_info` and
     `suggest_cloud_environment_image` drive the repo dropdown and the Docker-image suggestion
     in the Environments settings page — 127 references in `update_environment_form.rs` plus
     its own test file. That is the cloud-environments feature, a separate round.

   **One collapse, by the 4g/3u "only writer" rule.** `get_integrations_using_environment`
   answered "is this environment still used by an integration?" before `warp environment
   update|delete` touched one. With `warp integration` deleted there is no way to create an
   integration, so the answer is permanently empty and the prompt could never fire — the same
   deletion, not a second judgement call. `confirm_if_integrations_using_environment` went, and
   with it the `--force` flag on both subcommands: its documented purpose was "without checking
   for integration usage", so it had nothing left to skip.

   `title_case_identifier` in `text_layout.rs` was orphaned by `integration_output.rs` and went
   too — caught by comparing the warning set, per 3z/4l.

   **Known and deliberately left: `ai/agent_management/cloud_setup_guide_view.rs` (742 lines).**
   Its step 2 tells the user to run `oz integration create slack|linear`, which no longer parses.
   Step 1 (`/create-environment`, `oz environment create`) was already failing at runtime, so the
   page was uniformly broken before this round — but it is *also* the zero state of the agent
   management page (`ViewState::SetupGuide` renders whenever there are no cloud agents, which in
   this build is always). Deleting it means inventing a new zero state, which is a design call,
   not a mechanical one. It goes with cloud environments; do not half-edit it first.

   Acceptance: `cargo check --all-targets` and clippy (simplewarp) at the exact pre-round warning
   baseline — every remaining warning traced to a file this round did not touch. `cargo nextest
   run` (simplewarp): **5,939 run, 5,939 passed, 0 failed**; `warp_cli` + `warp_features`: 214
   run, 214 passed. The 4m landmine was checked up front — grepping the literal `"integration"`
   found `mut_subcommand("integration", …)` and the arg-slice reject block in `warp_cli/lib.rs`,
   neither of which any grep for the *variant* name reaches.

   The provider still has 7 client getters — `get_integrations_client` stays, because the trait
   keeps its four GitHub/environment methods. `IntegrationsClient` is down from 7 methods to 4.

4p. **Cloud environments: the whole management surface, and `IntegrationsClient` with it — DONE.**
   76 files, 16,892 deletions, 155 insertions; 22 files removed outright. The largest round in
   this project so far, and the first one taken as a *feature*, the way 4n said the rest of the
   chain would have to be.

   **Why this closes the chain step.** `IntegrationsClient`'s four surviving methods were all
   attached to environments: `get_user_github_info` and `suggest_cloud_environment_image` drove
   the Environments settings form, `check_user_repo_auth_status` and `poll_oauth_connect_status`
   gated the repo arguments of `warp environment` and `warp agent skills`. Deleting the feature
   deleted all four, so the trait, its impl, `server_api/integrations.rs` and
   `get_integrations_client` are gone. **The provider is down to 6 client getters.**

   **The proof that made every collapse safe.** `CloudAmbientAgentEnvironment` is a *cloud
   object*: it only ever enters the store through `cloud_objects` sync, and creating one needs
   `owner_for_new_environment`, which needs `AuthStateProvider::user_id()`. In a build with no
   account that is always `None` and the store is permanently empty. Every `get_all`/`get_by_id`
   in the ~20 surviving lookup sites therefore answers empty by construction — the same "only
   writer" argument as 4g/3u, applied to a whole feature.

   **Deleted outright (22 files):** the settings surface — `environments_page` (2,094 + 1,480 of
   tests + a 141-line button), `update_environment_form` (3,573 + 1,307), the agent-assisted
   modal (767 + 365), the delete-confirmation dialog, the handoff creation modal; the entry
   points — `cloud_setup_guide_view` (742), `terminal/view/init_environment/` (673),
   `create_environment_modal` (109 + 84), the footer `environment_selector` (520),
   `environment_management_pane`, `ambient_agent/first_time_setup` (374); the CLI —
   `agent_sdk/environment.rs` (1,114), `oauth_flow.rs`, `warp_cli/src/environment.rs` (trimmed to
   the two shared `--environment` arg groups that `warp agent` and `warp schedule` still use);
   and `server_api/integrations.rs` plus `settings_view/telemetry.rs`, whose only variant was
   `EnvironmentsPageOpened`.

   **Two judgement calls worth naming.**

   - *The agent-management zero state.* `ViewState::SetupGuide` was the page's zero state — it
     rendered the oz onboarding guide whenever there were no conversations, which in this build
     is always. Deleting the guide meant the page needed *something*, so the state became
     `ViewState::Empty` with a body built from the same vocabulary as `render_no_results_view`
     two functions down (icon + label, centred). The header already showed the "New agent"
     button in this state, so it survives unchanged. This is the only place in the round where
     new UI was written rather than removed; it is ~20 lines and copies an existing pattern.
   - *`WarningBoxConfig` collapsed to one argument.* The Environments page was the only caller
     that used the builder's description, icon override, max width, or action button. With it
     gone both remaining callers pass a formatted title and nothing else, so the config struct,
     its five builder methods, `WarningBoxTitle::Text`, and `WarningBoxButtonConfig` went and
     `render_warning_box` now takes the title directly — 255 lines to 80.

   **Collapses rather than deletions.** The `&` handoff and cloud-mode-v2 submit paths both used
   to open a creation modal when no environment existed; they now stop, which is the same
   observable outcome minus a modal that could not have created one. The orchestration
   environment picker survives (it is orchestration UI, same call as 4m's runner picker) but its
   "New environment" footer and the `create_environment_requested` trait method went with the
   modal. The `--environment` flag on `agent run`/`run-cloud` stays and stays hidden.

   **The 4m landmine, hit for real this time.** `cargo check` and clippy were both clean and
   140 tests then failed at runtime with ``Command `environment` is undefined``:
   `mut_subcommand("environment", …)` in `warp_cli/src/lib.rs`, plus an arg-slice reject block
   above it. Grepping the *variant* name finds neither. The pre-round grep for the literal was
   done for `"integration"` in 4o and **not** repeated for `"environment"` here. Do it every
   time, for every deleted subcommand name, before running the suite.

   **A near-miss worth recording.** A helper that deleted "from marker to next marker" was used
   to remove `active_init_environment_block`; its end marker was 2,100 lines further down, so it
   would have silently swallowed the whole span. The script asserted on a later edit and aborted
   before writing, which is the only reason the file survived. Bulk edits must delete by matched
   brace depth or by verified line range — never by "next marker I can think of".

   Acceptance: `cargo check` and clippy (simplewarp, `--all-targets`) back to the exact pre-round
   warning baseline — 3 lib warnings plus the same 11 clippy lints, every one traced to a file
   this round did not touch. `cargo nextest run` (simplewarp): **5,850 run, 5,850 passed, 0
   failed**; `warp_cli` + `warp_features`: 209 run, 209 passed. Format clean.

   **What is deliberately left.** `CloudAmbientAgentEnvironment` itself and
   `ai/cloud_environments/` (410 lines) plus `cloud_object_models/cloud_environment.rs` (289)
   stay, together with the ~20 lookup sites that now provably answer empty — the model is a
   `cloud_objects` participant and comes out with that layer, not before it. `ambient_agent/`
   (8,889 lines: auth secrets, harness/host/model selectors, the loading screen) is the
   *cloud agent mode* UI — adjacent to this feature, not part of it, and its own round.

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

   **`warp_server_client` next, and it is not a clean cut.** At the time this was written the
   crate looked like four separate things: `iap` (839 lines of identity-token minting), `auth`
   (845, the session and token layer that `app/src/auth/` is built on), `base_client` (417),
   and `network_logging` (164, which backs the in-app network log view) — with `iap` plus
   `base_client` named as the tractable slice. **`iap` is gone as of 4b**, which covered "six
   crates" without naming this one specifically; checked directly (`grep -rn "IapConfig\|iap::"`
   across the workspace, zero hits) rather than assumed. That leaves `base_client` as the only
   candidate slice, and it is not tractable on its own: `graphql_helpers::send_graphql_request`
   (a live function, not the stubbed `app/src/server::ServerApi` wrapper of the same name 3e
   built) and `public_api::get_public_api` both take a `&BaseClient`, and `auth/mod.rs` calls
   `send_graphql_request` directly for real login/session-refresh GraphQL operations — the same
   `app/src/auth/` entanglement 3h–3q have been tracing, not a separate crate-level cut. The
   crate stays whole until that thread's own deferred `AuthOnboardingState` refactor (3p) or the
   still-untouched `app/src/workspaces/` module (3m) gets an attended round.

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

4k. **The two orphans 3k deferred — DONE, and nothing else new.** ~20 lines. 3k's "left for a
   future clippy sweep" note named `Workspace::open_vertical_tabs_panel_if_enabled`
   (`workspace/view.rs`) and `OnboardingTutorial::intention` (`workspace/view/onboarding.rs`)
   as genuinely zero-caller back then; a fresh `cargo clippy -p warp --lib --all-targets
   --no-default-features --features simplewarp` confirmed both still have none — not even a
   test — and deleted both.

   `intention`'s own body pattern-matches `OnboardingTutorial::NoProject { intention }` etc.,
   which binds a local variable of the same name as the method it's inside — easy to
   misread as a self-call. It isn't one; the method's only "callers" a naive grep turns up are
   that field-destructuring syntax in unrelated functions, not `.intention()` invocations. The
   same field name, method name, and enum-field-shorthand collision that made 4j warn about
   trusting `git log -S` output at face value.

   **Checked and confirmed to bundle correctly with `AuthOnboardingTarget`, not a third orphan:**
   `AuthOnboardingTarget::to_workspace` (`root_view.rs`) still shows the same "never used"
   warning as its enum, and a first pass read that as a second dead orphan alongside the two
   above. It isn't — `AuthOnboardingState::complete_web_import`, `#[cfg(target_family =
   "wasm")]`, calls it at `root_view.rs:2534`. An unqualified `grep -rn "AuthOnboardingTarget"`
   missed the call because the compound pattern's alternation didn't line up with this shell's
   quoting; a second, narrower `grep -rn "\.to_workspace("` found it. **A "confirmed dead"
   result from one grep pass is only as good as the pattern — re-run a narrower, unambiguous
   search before deleting anything the type's own doc comment or a sibling round already called
   live.** 3k's original bundling of the enum with this method was correct; left both alone.

   Acceptance: `cargo check` (both feature sets, `--all-targets`, `-p integration`), `cargo
   clippy -p warp --lib --all-targets --no-default-features --features simplewarp` (0 errors,
   40 warnings — down from 49, all of it the `tear_down_cloud_mode_setup_phase` cluster and
   pre-existing style lints), and `./script/format --check` all clean. `cargo nextest run -p
   warp --lib --no-default-features --features simplewarp --no-fail-fast`: 5978/5986 pass, the
   same 8 pre-existing failures as every round in this thread, 0 new. 2 files changed, 21 lines
   removed. Not re-run in the app — neither deletion was reachable UI.

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

   **Rounds 4ad–4ay kept sweeping by feature instead of by flag** (each recorded under its
   own Definition-of-done entry; 4ak–4av in one bulk entry there, 4aw–4ay on their own):
   30 more variants went with their verticals, including 3 definition-only deletions in
   4at and 1 in 4aq. **216 variants remain** of the original 292. 4at's sweep also identified 29 further
   off-by-default flags, two of which (AgentHarness, GeminiEnterprise) look live-by-design;
   those and the flags behind the not-yet-deleted modules are what is left.

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
      - [x] **The SuperGrok subscription OAuth is gone** (4z, 2026-09-02): ~1,150 lines. The
            `supergrok` cargo feature was in `default` only, so `FeatureFlag::SuperGrok` was
            constant-false in this build and the whole vertical was unreachable: the
            `crates/ai/src/grok_subscription` module (loopback PKCE server + token refresh),
            `GrokTokens` and its secure-storage read/write on `ApiKeyManager`, the Connect/
            Disconnect row and paste-the-code editor on the Warp Agent settings page, the
            request-time token refresh branch in `ResponseStream::spawn_request`,
            `AIApiError::GrokSubscriptionTokenRefreshFailed`, two telemetry events, and four
            `crates/ai` deps only it used (`base64`, `rand`, `http_client`, `serde_urlencoded`).
            The proto field `grok_oauth_access_token` on the request `ApiKeys` stays (external
            crate) and is sent empty. **Lead left behind:** `LLMProvider::Xai` now has no
            client-side credential at all (the subscription was the only xAI auth; there is no
            pasted xAI key), so the variant, its `GrokLogo` icon, and
            `ProviderCredentialTelemetryProvider::Xai` are a dead provider — a small follow-up.
      - [x] **Three more constant-false flag verticals are gone** (4aa–4ac, 2026-09-02, ~3,100
            lines): `LLMProvider::Xai` (credential-less after 4z), the Codex Warp plugin manager
            (`CodexPlugin`/`CodexNotifications`; Codex OSC 9 detection kept), and the first slice
            of `HOANotifications` — the HOA onboarding flow (`HOAOnboardingFlow`), the
            OpenCode/Gemini plugin managers (`OpenCodeNotifications`/`GeminiNotifications`), and
            the `WARP_CLI_AGENT_PROTOCOL_VERSION` PTY export. **Method**: the `simplewarp`
            feature set omits 49 `default` cargo features, each mapping to one `FeatureFlag`
            that is constant-false but still compiled; rank them by `FeatureFlag::X` site count
            and delete the vertical behind each. Remaining top targets: `SharedWithMe` (40
            sites), `OpenWarpNewSettingsModes` (38), the rest of `HOANotifications` (mailbox,
            toast stack, `AgentNotificationsModel` mailbox path, `show_agent_notifications`
            setting, and the `HeaderToolbarItemKind::NotificationsMailbox` variant — a
            **persisted** enum, so deleting it needs a serde fallback for old configs),
            `CloudMode` (+5 sub-flags), `AccountFirstOnboarding`, `AgentManagementView`
            (the 4,400-line `ai/agent_management` module, minus the live `telemetry.rs` and
            `details_action_buttons.rs`).
      - [x] **The agent notifications mailbox is gone** (4ad, 2026-09-02, ~3,430 lines): the
            `ai/agent_management/notifications` module, the mailbox half of
            `AgentNotificationsModel` (now only the legacy `ConversationNeedsAttention` toast
            emitter), the workspace mailbox/toast-stack views and actions, the vertical-tabs
            unread dots, the features-page toggle and toast-duration editor, and the
            `AgentNotificationShown` telemetry. **Kept by decision**:
            `HeaderToolbarItemKind::NotificationsMailbox` (persisted enum; `is_supported()` is
            false) and the `show_agent_notifications` / `notification_toast_duration_secs`
            settings (persisted; nothing reads them).
      - [x] **The plugin-install chip and `FeatureFlag::HOANotifications` are gone** (4ae,
            2026-09-02, ~1,610 lines): the footer's "Enable notifications" / "Update Warp
            plugin" chip (buttons, debounce, auto install/update, dismissal), the
            `OpenPluginInstructionsPane` chain from footer to workspace and the
            `plugin_instructions_block` module, `PluginModalKind` / `PluginInstructions`,
            `CLIAgentSessionsModel`'s auto-failure tracking, the four `CLIAgentPlugin*` chip
            telemetry events, the `AISettings` chip-dismissal accessors, and the
            `hoa_notifications` cargo feature. **Kept by decision**: the two persisted
            chip-dismissal maps on `AISettings`. **Kept because live**: `plugin_manager_for`
            and `ClaudeCodePluginManager` (driver.rs, local_harness_launch.rs,
            terminal/view.rs). `PluginInstallError`'s Display now includes the command log,
            so the driver's warn line keeps the detail the chip used to write to a file.
      - [x] **The SharedWithMe flag is folded away** (4af, 2026-09-02, ~1,800 lines): ~50
            `!flag || permission` guards across Drive, notebooks, workflows and env-var
            collections now take their flag-off branch; the "Remove" (leave shared object)
            action and its `UpdateManager::leave_object` path, `CloudObject::can_leave`,
            `CloudModel::has_directly_shared_objects`, the bincode guest / link-sharing codec in
            `cloud_object_persistence`, `CloudObjectGuest`/`CloudLinkSharing::from_server` and 16
            flag-on tests are gone. `owner_to_space` maps User to Personal and Team to Team.
            **Kept**: `Space::Shared` (20+ live matches, web-anonymous path) and the guest /
            link-sharing columns and struct fields (written empty). Lead: the sharing UI that
            still reads `guests` / `anyone_with_link` is a future slice.
      - [x] **The dead team-billing surface is gone** (4ag, 2026-09-02, ~950 lines): 4af made
            rustc surface `UserWorkspaces` items with no non-test callers: team ownership
            transfer, add-on credits purchase, usage-based pricing update, billing / upgrade
            links, per-window team setter, purchase-policy and billing-metadata accessors, and
            their `TeamClient` / `WorkspaceClient` trait methods, events and 19 tests.
      - [ ] **The other cloud crates remain** (4), and so do the three modules that are refactors
            rather than deletions: `remote_server`, `auth`, and `cloud_object` (3), now including
            the remainder of `drive`. The remaining dead `FeatureFlag` variants fall out of those
            (5), together with the ~85 items 4e left dead but standing.
      - [x] **The `OpenWarpNewSettingsModes` app-side guards are folded** (4ah, 2026-09-02,
            ~520 net lines): the editor/code-review page's dead `ExternalEditorView` child view
            and its always-early-returning `init_actions_from_parent_view`, the
            "preferred conversation layout" dropdown on the AI settings page (field, action
            variant, handler, render block — the underlying `open_conversation_layout_preference`
            setting stays, it's still read in `workspace/view.rs`), five `should_render` /
            widget-push guards across `code_indexing_page`, `features_page` and
            `appearance_page` folded to their flag-off branch, and the anonymous-sign-up-banner
            guard clause in `workspace/view.rs` / `terminal/view.rs`. Also deleted the
            `OneTimeModalModel` free-AI-removal auto-trigger chain
            (`check_and_trigger_free_ai_removal_modal`, `maybe_recheck_free_ai_removal_modal`,
            the `has_fetched_workspaces` field, the `AIRequestUsageModel` subscription) since it
            never fired — its first line was the same dead flag check — which in turn left
            `free_ai_removal_modal_decision` and its 13-case test matrix with no caller;
            deleted both. **Kept**: `is_free_ai_removal_modal_open` / `force_open_free_ai_removal_modal`
            and the `Notice`-variant modal view, reachable via the debug-only force-open menu
            action; the `did_check_to_trigger_free_ai_removal_modal` setting, now write-only.
            **Untouched, confirmed independent**: the `PromptSuggestions` variant of the same
            shared `FreeAiRemovalModal` view, which is a live, unrelated feature. Only
            `OpenWarpNewSettingsModes` moved; `AccountFirstOnboarding` and all of
            `crates/onboarding/` (six slide files, `model.rs`'s three-way branching, telemetry,
            ~19 flag-on-only tests) are a separate, riskier round — not done yet.
      - [x] **`crates/onboarding`'s `model.rs`/`telemetry.rs` flag branches are folded** (4ai,
            2026-09-02, ~1,250 net lines): both flags' checks in `model.rs`'s `settings`/
            `set_models`/`send_completion_telemetry`/`complete`/`back`/`next`/`set_step`/
            `progress` collapsed to their legacy arm; `is_ai_enabled` simplified to a constant
            `true`; `ai_setup_flow_active` and `send_account_first_action` deleted (zero callers
            once folded). `telemetry.rs`'s `flow_version`/`with_flow_version` deleted (both
            permanent no-ops), fixing up 4 event-payload arms including one
            (`OnboardingSlidesCompleted`) the initial plan missed. 16 of 18 flag-gated
            `model_tests.rs` tests deleted outright (tested only the vanishing branches), 2 kept
            with the vestigial `override_enabled` line dropped; same pattern for 2 of 5
            `telemetry_tests.rs` tests, with one replacement assertion added to the surviving
            default-case test to avoid a coverage gap. **Scope explicitly narrowed mid-round**:
            research found `crates/onboarding`'s `model`/`agent_onboarding_view` modules are
            Rust-private (`mod`, not `pub mod`) — the whole slide-wizard state machine is
            unreachable from the real app, only from the crate's own opt-in preview binary — and
            `app/src/settings/onboarding.rs`'s `apply_onboarding_settings`/
            `apply_account_first_onboarding_settings` have zero production callers anywhere.
            User decision: don't resolve "is the wizard subsystem reachable at all" this round;
            treat `OnboardingStateModel` as a live state machine in its own right and only fold
            its two flags. **Left deliberately untouched** (still live source, referenced by
            out-of-scope slide files under their own copy of the same flags):
            `NoAiConfirmationSource` and its confirm/cancel/dismiss methods, `AiSetupChoice`/
            `AiAccessChoice`, and the `OnboardingStep::{AiSetup,AiAccess,Customize,ThirdParty}`
            variants — folding `back`/`next` did make `OnboardingStep::ThirdParty` provably
            unconstructed (new `cargo check` warning), confirming it was already unreachable in
            production before this round; left as documented debt rather than chased into the
            slide files. **Next candidates, in order of clean-up value**: the 6 onboarding slide
            files + `agent_onboarding_view.rs` (its `account_first` fold alone would delete the
            entire dead `preload_onboarding_images` function and orphan 3 `VISUAL_IMAGE_PATHS`
            consts), then a dedicated investigation into whether `app/src/settings/onboarding.rs`
            and the wizard's `SelectedSettings`-consuming pipeline are dead code independent of
            these flags.
      - [x] **The entire onboarding-wizard subsystem is deleted** (4aj, 2026-09-02, ~8,600 net
            lines): the 4ai reachability lead turned out correct. `AgentOnboardingView` was
            publicly re-exported and its `init()` was called from `app/src/lib.rs:1737`, but
            nothing in the app ever constructed an instance — `init()` only registered
            keybindings scoped to a view that never rendered, reachable only via the crate's own
            opt-in `onboarding` preview binary. User approved deleting the whole thing rather
            than continuing to fold guards inside it. Deleted: `agent_onboarding_view.rs`,
            `model.rs` (`OnboardingStateModel`) + its tests, all 20 files under `slides/`, all 6
            files under `visuals/`, `components/feature_optout_dialog.rs`, the preview bin
            (`src/bin/main.rs`, `telemetry_provider.rs`, the `[[bin]]`/`bin` feature in
            `Cargo.toml`), and `app/src/settings/onboarding.rs` + its tests (every function —
            `apply_onboarding_settings`, `apply_account_first_onboarding_settings`,
            `apply_ui_customization_settings`, `apply_agent_settings`,
            `action_permissions_for_onboarding_autonomy` — had zero production callers). Both
            `FeatureFlag` variants and their cargo features are now fully deleted. **Confirmed
            separate, untouched**: `crates/onboarding/src/callout/` (`OnboardingCalloutView`,
            live via `app/src/terminal/view.rs`) and `OnboardingTutorial` in
            `app/src/workspace/view/onboarding.rs` (an app-local type, unrelated naming collision
            with the crate) — only that file's dead `impl From<SelectedSettings>` and one
            now-permanently-false flag guard in `dispatch_tutorial_when_bootstrapped` were
            removed, the rest of the file stays. `telemetry.rs`'s `OnboardingEvent` was not fully
            dead — `callout/model.rs` constructs 3 of its ~20 variants live
            (`CalloutDisplayed`/`CalloutNext`/`CalloutCompleted`) — so it needed variant-level
            pruning rather than deletion; replaced `telemetry_tests.rs`'s 5 dead-variant tests
            with one small test covering the 3 survivors (previously zero coverage of the live
            path). Two more items orphaned by the deletion and confirmed zero-caller via
            `cargo check`: `FtueAccountClass` enum + its `as_str`/test (existed only to route
            account-first signup outcomes) — deleted; `AIExecutionProfilesModel::should_preserve_onboarding_profile`
            — kept, since it still has a real behavioral test caller
            (`explicit_local_collection_is_preserved_from_onboarding`), matching the 4ah
            `request_limit()` precedent for a general accessor with its own test coverage.
            nextest 5707 green (down from 5720, expected from the test deletions).
      - [x] **Flag-vertical rounds 4ak–4ay (2026-09-03 → 2026-09-08); 4ak–4av in bulk
            (twelve rounds, ~20 flags), 4aw–4ay after — the enum down to 216 of the
            original 292.** Detailed in their commit messages; this is the summary the
            plan would otherwise miss. 4az (2026-09-08) followed with the first
            non-flag slice since 4m: the dead handoff-snapshot upload chain, ~1,550
            lines across `remote_server`, `server_api/ai`, `agent_sdk`, and the
            daemon — SSH remote untouched. 4ba (2026-09-08) took the
            `WorkspaceClient` billing trait with its callers, including the whole
            build-plan-migration modal (~900 lines). 4bb (2026-09-08) bundled
            three small-fry slices (~250 lines) and took the enum from 212 to
            208 variants.
            - **CloudConversations (4ak)** folded to always-off: the sharing menu items, the
                  pane-header share button, the privacy cloud-storage toggle, the cloud-delete
                  calls on conversation delete/remove, and the always-`None` server-load path.
                  The flag itself and the CLI `--conversation` hide/validate guards stay (shared
                  pattern with CloudEnvironments/CloudRunners/OzPlatformSkills), as does
                  `CloudConversationData::CLIAgent` (a multi-caller fork in `pane_group/`,
                  deferred).
            - **AgentManagementView (4al)** deleted the dashboard view and its type-selector
                  popup and folded every call site (tab-bar/toolbar/panel/vertical-tabs;
                  `ToggleAgentManagementView` and `ViewAgentRunsForEnvironment` went outright).
                  `OpenAgentManagementView` stays as a no-op stub — it is a cataloged
                  local_control/CLI command surface — and `WindowSnapshot.agent_management_filters`
                  stays as a persisted SQLite column, now always `None` (removal needs a
                  migration). The dashboard was the sole consumer of nine
                  `AgentConversationsModel` methods; they went with 11 of their tests.
            - **FullSourceCodeEmbedding (4am)** deleted the embedding-backed retrieval path:
                  `ai/codebase_auto_indexing.rs` whole, the client-side
                  `RemoteCodebaseIndexModel`, the 357-line speedbump-banner subsystem, `/index`,
                  and the remote rows on the code-indexing page. Mid-round scope correction:
                  `crates/ai`'s `full_source_code_embedding` module came back via
                  `git checkout` — the remote-server daemon (`server_model.rs`, behind the
                  separate, protected `RemoteCodebaseIndexing` flag) imports
                  `CodebaseIndexManager` directly, and the planned dependent deletions were
                  abandoned before landing. The incidentally-nested `ProjectContextModel`
                  subscription and the page's live LSP-management UI stay, per the
                  `code_editor_review_page.rs` precedent.
            - **OzHandoff (4an, two commits)** removed local-to-cloud handoff. Part 1: the
                  commit pipeline, the driver snapshot writer, the checkpoint coordinator, the
                  checkpoint-upload path (two `HarnessSupportClient` methods with it), the
                  settings widget, `/handoff`, and orphaned telemetry — with
                  `AutoCloudHandoffController` and `start_local_to_cloud_handoff*` folded to
                  no-op stubs because part 2 still called them by name. Part 2: the
                  `&`-prefix compose mode, the footer chip, the whole
                  `auto_handoff_sleep_modal/` view, the handoff state machine with
                  `PendingHandoffChanged`/`HandoffSnapshotUploadFailed`, four backing settings,
                  and the newly-dead env-overpick in `handoff/touched_repos.rs`. Kept:
                  `PendingCloudLaunch` stubs, the `OpenLocalToCloudHandoffPane`/
                  `AutoHandoffActiveAgentToCloud` handlers, and the URI trigger — all routing
                  into part 1's stub. The remote daemon's own snapshot upload path is untouched.
            - **CloudMode and its sub-flags (4ao–4as)** — these were in `default`, so folding
                  means always-on, the opposite direction of every round before. 4ao/4ap deleted
                  the base flag and `CloudModeFromLocalSession` (disabled-state branches, 3
                  sites); 4aq found `CloudModeImageContext` had zero readers anywhere — orphaned
                  upstream, definition-only delete; 4ar folded `CloudModeSetupV2` +
                  `HandoffCloudCloud` together because they were entangled in shared boolean
                  expressions, deleting the legacy ambient loading footer and screens (all of
                  `loading_screen.rs`), `CloudAgentStartupPresentation*`, and the
                  auto-open-details-panel one-shot mechanism; 4as made the V2 input composer
                  unconditional, deleting the v1-spawns-cloud-agent test whose premise was the
                  off-branch.
            - **Dead-weight sweep (4at)** — four zero-reader removals: the `loginless_conversion`
                  cargo feature (no flag at all), `AgentManagementDetailsView` (orphaned by 4al),
                  `ConversationArtifacts`, and `HOARemoteControl` (its chip was already gone).
                  29 further off-by-default flags were identified in the same sweep and
                  deferred; two of them (AgentHarness, GeminiEnterprise) look live-by-design.
            - **Five small verticals (4au)** — `CreateEnvironmentSlashCommand` (zero readers;
                  dead weight 4at missed), `GlobalAIAnalyticsCollection` (the telemetry
                  predicate collapses to `AgentModeAnalytics.is_enabled()`, taking the
                  `is_telemetry_enabled` parameter and its nine call sites), `SyncAmbientPlans`
                  (constructor defaults the field to `false`, which stays — profiles/editor
                  still write it), `UsageBasedPricing` (plan-info row renders unconditionally;
                  GraphQL wire types stay), `WarpPacks` (plain folder icon; `is_warp_pack`
                  persistence stays).
            - **DriveObjectsAsContext + CloudAgentRunners (4av)** — deleting the first removed
                  the only construction sites of
                  `AIContextMenuCategory::{Workflows,Notebooks,Plans}`, so the variants
                  collapsed with their name/icon/navigation/selection arms and both feeding
                  modules (`search/ai_context_menu/{workflows,notebooks}/` —
                  `WorkflowDataSource::new` was already wearing `#[allow(dead_code)]`) went
                  with their 5 tests; `InsertPlan`'s only constructor was the notebooks search
                  item, so it and its `terminal/input.rs` arm went too
                  (`QueryFilter::Workflows`/`Notebooks` stay — `command_search` builds them).
                  The second pinned `supports_orchestration_runners` and
                  `runner_controls_enabled` to `false` and left the runner-picker UI for the
                  remote-mode orchestration round, per 4m's scoping.
            - **The ambient agents flag family (4aw)** — all four variants
                  (`AmbientAgentsCommandLine`, `ScheduledAmbientAgents`, `AmbientAgentsRTC`,
                  `AmbientAgentsImageUpload`) folded to always-off and deleted. The
                  ambient-agents CLI went whole: the `agent run-cloud` and `schedule`
                  subcommands (with their argv guards, help-hide guards, and the sibling
                  CloudEnvironments/CloudConversations/AgentHarness guards that targeted
                  `run-cloud` args), `warp_cli/schedule.rs`, `RunCloudArgs`, the app-side
                  `agent_sdk/schedule.rs` and the `run_agent` half of
                  `agent_sdk/ambient.rs` (the tasks/status/message/get-conversation CLI
                  stays), `ScheduledAgentManager` (the sync-layer `StringModel`/`JsonModel`
                  impls in `ambient_agents/scheduled.rs` stay — update_manager and
                  sync_queue consume them), its telemetry variants, and the six
                  scheduled-agent tips in the ambient-agent tip pool. RTC died with it:
                  the `UpdateManagerEvent::AmbientTaskUpdated` variant, the
                  update-manager handler (the cloud-crate `ObjectUpdateMessage` variant
                  stays, its arm is now an empty no-op), and the whole throttle subsystem
                  in `AgentConversationsModel` (polling resumes; the 30s poll is the only
                  refresh path again). Image upload took
                  `driver/attachments.rs` whole (upload `process_attachment`, download
                  `fetch_and_download_attachments`, `AIClient::get_task_attachments`) and
                  collapsed the `--task-id` metadata fetch to metadata-only; the
                  handoff-snapshot stub from 4an went with it. The now-test-only
                  `create_object_online`/`update_object_online` wear `allow(dead_code)`
                  pending the cloud_objects layer. Clippy note: master's presubmit clippy
                  is currently red with 11 pre-existing items (last green before the two
                  post-4av commits); this round's error set is byte-identical to that
                  baseline.
            - **Five cloud-only verticals (4ax)** — `FileAndDiffSetComments`,
                  `EmbeddedCodeReviewComments`, `ContextLineReviewComments`,
                  `TeamApiKeys`, and `McpDebuggingIds`. The review-comments fold removed
                  the header "Add comment" item from the overflow menu in both renderers
                  and collapsed the legacy header's comments-only dropdown to the plain
                  add-diff-set-context button — while keeping the dropdown mechanism
                  itself, since the live `render_new` header still hosts the
                  DiffSetAsContext/Discard items through it. The diffset (General)
                  comment composer entry points went
                  (`OpenCommentComposerFromHeader`, `get_existing_diffset_comment`,
                  `ReviewCommentBatch::diffset_comment`); the composer itself and
                  `AttachedReviewCommentTarget::General` stay — imported comments and the
                  comment-list edit path still reach them. In the code editor the legacy
                  positioned comment editor now renders unconditionally (the embedded
                  renderer was the flag's other half) and the gutter comment button on
                  non-diff lines shows only where a comment exists. `TeamApiKeys`
                  collapses to `NamedAgents` on the platform page and deletes the
                  has-team machinery in the create-key modal; `McpDebuggingIds` takes the
                  MCP server card's debug-lines block. Left for later rounds: HOA
                  (entangled with editor selection-context and the remote right panel),
                  `RemoteCodeReview` (remote-server pass), `WaitForEventsParentRegistration`
                  (orchestration).
            - **HoaCodeReview (4ay)** — the HOA CLI-agent code-review interplay is
                  gone whole: `TerminalView::active_cli_agent` (flag-gated to `None`
                  anyway) and with it the right panel's CLI-review dispatch branch,
                  `ReviewDestination::Cli`, the four
                  `*to_cli_agent_or_rich_input` senders with `try_send_text...`'s
                  HOA gate, and the five `cli_agent.rs` prompt builders with their
                  tests; the review-context flows (selection, diff set, diff hunk,
                  attach-path) now go straight to their generic input-buffer paths.
                  The three flag-gated `with_selection_as_context` editor wirings went
                  (the ungated code-review one stays — `CodeSelectionAddedAsContext`
                  telemetry maps to `Always` now); five HOA tests and the
                  `SelectionAddedAsContext` event's `selected_text` field went with
                  them. The rich-input feature itself (`cli_agent_rich_input`, live in
                  simplewarp) is untouched.
            - **The dead handoff-snapshot upload chain (4az, 2026-09-08, ~1,550
                  lines)** — the last local→cloud pipeline that 4an left
                  standing, deleted end to end. 4an removed every client-side
                  *sender* of the handoff snapshot; what remained was the
                  upload machinery behind them, kept alive only by its own
                  daemon echo. Gone: the `UploadHandoffSnapshot` /
                  `UploadHandoffSnapshotResponse` proto messages and both oneof
                  entries (field numbers 14/26 not reused), the daemon's RPC
                  arm and `handle_upload_handoff_snapshot` in `server_model.rs`,
                  the whole `app/src/remote_server/handoff_snapshot.rs` handler,
                  the client-side sender `RemoteServerManager::upload_handoff_snapshot`
                  and its `RemoteServerOperation::UploadHandoffSnapshot`
                  variant, `app/src/ai/agent_sdk/driver/snapshot.rs` (the
                  1,079-line gather+GCS-upload pipeline — the module held
                  nothing else), `blocklist/handoff/{snapshot,touched_repos}.rs`
                  (the touched-repos walker's only consumer was the daemon
                  handler), `AIClient::{upload_local_handoff_snapshot,
                  get_handoff_snapshot_attachments}` with their wire types
                  (`InitialSnapshotToken`, `UploadLocalHandoffSnapshot*`,
                  `SnapshotUploadFileInfo`, `TaskAttachment`),
                  `SpawnAgentRequest.initial_snapshot_token` (every constructor
                  passed `None`; one round-trip test dropped its token
                  assertions), harness_support's now caller-less
                  `SnapshotFileInfo`, and the `PendingCloudLaunch`-adjacent
                  module docs that described them. **SSH remote is untouched by
                  design**: the deleted RPC was a cloud upload a client requests
                  from a daemon, not transport, buffers, git, ripgrep, code
                  index, or any other daemon capability; the client sender had
                  zero callers and the daemon arm zero senders, so no live SSH
                  surface changed. `handoff/mod.rs` keeps
                  `HandoffLaunchAttachments`/`PendingCloudLaunch` — the 4an
                  stubs still construct them. **Environment note**: this round
                  ran under a sandbox where `$HOME` is read-only, so `cargo`
                  needed a project-local `CARGO_HOME` copy and the app could not
                  be launched from here (both the GUI app and the headless
                  daemon need `~/.warp*` state dirs); the run-in-app smoke test
                  is left to the user's normal shell. Five `warp --lib` test
                  failures (two pane_group, slash-command tilde, two worktree
                  sidecar) reproduce byte-identically on a clean stash with a
                  verified recompile — pre-existing, not this round's.
            - **The `WorkspaceClient` billing trait (4ba, 2026-09-08, ~900
                  lines)** — the second of 4n's client chain, deleted whole with
                  every UI that only existed to reach it. All three methods
                  (`generate_stripe_billing_portal_link`, `refresh_ai_overages`,
                  `update_addon_credits_settings`) were `local_only_error()`
                  walls. Gone: the trait, its mock, and the getter;
                  `UserWorkspaces`' client field/param and its three method
                  pairs; `UserWorkspacesEvent::{GenerateStripeBillingPortalLink,
                  GenerateStripeBillingPortalLinkRejected, AiOveragesUpdated,
                  UpdateWorkspaceSettingsSuccess, UpdateWorkspaceSettingsRejected}`
                  — the last pair's only emitter was the addon-credits response
                  handler, so geap/aws/warp-agent-page subscribers drop it and
                  keep `TeamsChanged`; `PricingInfoModel::addon_credits_options`,
                  `UserWorkspaces::{current_workspace_mut, workspace_from_uid_mut}`
                  (their only caller was the overages refresh); the blocklist
                  controller's `maybe_refresh_ai_overages`; the drive panel's
                  delinquent-payment banner (`render_payment_issue_banner`,
                  `ManageBilling`, its four copy constants and mouse state);
                  main_page/teams_page "Manage billing"/"Manage plan" links with
                  their actions; `GrowTeamWarningCta::UpdateBilling` (payment-
                  past-due CTAs now go to support, matching the deleted
                  self-serve path); `PromptSuggestionsEvent::OpenBillingPortal`
                  (no dispatcher existed); and **the whole
                  `build_plan_migration_modal.rs`** (870 lines) — its trigger
                  required an authed onboarded team admin, unreachable locally.
                  With it: the `OneTimeModalModel` build-plan half (the
                  `SunsettedToBuildDataUpdated` subscription, the feature-intro
                  dismissal resume hook, `is_any_modal_open`'s second disjunct)
                  and the two `[Debug]` palette bindings. **Kept by decision**:
                  the persisted `build_plan_migration_modal_dismissed` setting
                  (read-none write-none now; removal needs a settings migration,
                  same as 4ah's `did_check_to_trigger_free_ai_removal_modal`).
                  ~20 test files dropped their `MockWorkspaceClient` args
                  mechanically; no mock had expectations. The syncer flake
                  (`test_sync_local_pref_to_cloud_after_initial_sync`) failed
                  once under default features and passed 3/3 isolated, then the
                  full suite passed twice — the 4h cross-test-interference
                  family, not this round's.
            - **Small-fry bundle (4bb, 2026-09-08, ~250 lines)** — three more
                  slices in the 4at/4au shape, found by re-running the
                  zero-reader sweep over the enum (now 212 → 208 variants):
                  **`PeriodicHandoffCheckpoints`** — zero readers anywhere, a
                  definition-only delete like 4aq. **The handoff-chip toolbar
                  migration** (`maybe_ensure_handoff_chip_in_toolbar`) — a
                  startup migration whose whole job was to append
                  `AgentToolbarItemKind::HandoffToCloud` to persisted custom
                  toolbar layouts; 4an already made the chip's `is_available()`
                  permanently false, so in default builds the migration ran,
                  mutated the user's persisted layout, and produced a chip that
                  could never render — an actively harmful no-op. Gone with it:
                  `FeatureFlag::OzHandoff` and `FeatureFlag::HandoffLocalCloud`
                  (the migration was their last real reader; the remaining
                  mentions were doc comments) and their `oz_handoff` /
                  `handoff_local_cloud` cargo features. **Kept**: the persisted
                  `HandoffToCloud` toolbar variant itself (serde
                  backwards-compat, same reason as 4ad's `NotificationsMailbox`)
                  and the `did_add_handoff_chip_to_toolbar` setting (now
                  read-none write-none; removal needs a migration).
                  **`SharedBlockTitleGeneration`** — the server call it enabled
                  (`generate_block_title`) was deleted in 4i, so the whole
                  remaining surface was a settings toggle controlling nothing:
                  the flag + `shared_block_title_generation` cargo feature, the
                  Warp-Agent-page toggle row/binding/action/switch, the
                  `is_..._toggleable` predicate (whose enterprise-team and
                  dogfood checks died with it, taking `ActiveAIWidget`'s
                  `view_handle` field), the
                  `ToggleSharedBlockTitleGenerationSetting` telemetry event (4
                  arms), the `is_shared_block_title_generation_enabled` getter,
                  the workspace flag-context insert, and the
                  `SHARED_BLOCK_TITLE_GENERATION_FLAG` palette constant. The
                  persisted `shared_block_title_generation_enabled_internal`
                  setting stays (read-none write-none, migration-gated).
            - **The TeamClient deletion (4bc, 2026-09-10, ~8,300 net lines,
                  91 files, 11 files deleted)** — the last of the `server_api`
                  client-removal table entries: `TeamClient` (216 impl lines)
                  plus its mock, `TeamUpdateManager` (the 10-minute poller),
                  `team_tester`, and the client's whole UI — the 4,327-line
                  Teams settings page, its tab-menu entry, the Drive "Create a
                  team" / "Add teammates" blocks, the `warp://settings/teams`
                  and `warp://team/...` deep links, and the
                  `OpenTeamSettingsPage` action chain. `UserWorkspaces` lost
                  the `team_client` field, 13 team-management methods, and 20
                  event variants. The seven `TeamUpdateManager` consumers
                  (auth logout, agent-sdk model-list / whoami / worker setup,
                  blocklist quota arm, cloud-objects polling, Drive
                  SetCurrentWorkspace) rewire to direct clients;
                  SetCurrentWorkspace now persists via a new
                  `UpdateManager::persist_current_workspace`. One rewiring gap
                  the first full test run caught: deleting the
                  `TeamTesterStatus::initiate_data_pollers` hop had also
                  dropped the login-time kick that started cloud-object
                  polling, so nothing ever fetched the initial load while
                  online (the `NetworkStatus` Online event only fires on a
                  *change*). Restored as a direct
                  `UpdateManager::start_polling_for_updated_objects` call in
                  auth's user-fetched path and in the two test helpers
                  (`cloud_object/model/model_tests`, `test_utils`'s
                  `create_update_manager_struct`) that stubbed the hop.
                  Orphaned with the
                  page: `TransferOwnershipConfirmationModal`,
                  `AdminActions`, `ClickableTextInput`,
                  `CloudActionConfirmationDialog` (its only live reader at HEAD
                  was teams_page; the empty-trash dialog referenced it in a
                  comment only), and `ChipEditorState` +
                  `retain_authenticated_teams` + the GQL workspaces-metadata
                  conversion feeding the team switcher. **Kept by ledger
                  decision**: `PricingInfoModel` (the auth-class question), but
                  this round deleted `update_pricing_info` / `plans` /
                  `PricingInfoModelEvent` and pricing_promotion's never-firing
                  subscription, so the model is now permanently-empty state
                  behind a live API; `update_workspaces` is
                  `cfg(any(test, feature = "integration_tests"))` — production
                  code no longer refreshes the cached workspace list.
                  `AIRequestUsageModel`'s team-bonus-credit methods and the
                  sunsetted-to-build change-detection chain (zero subscribers)
                  went too. Acceptance: check/clippy clean in both presubmit
                  configs — strictly better than baseline (the pre-existing
                  `request_limit` dead method fell with its siblings, leaving a
                  9-error baseline that predates the round); format clean;
                  full nextest 8757/8763 with the only failures being six
                  proven pre-existing on HEAD (five ssh integration tests and
                  `test_create_folder_from_command_palette`, all environmental).
      - [x] **HarnessSupportClient, the `warp harness-support` CLI, and the harness
                  upload/save/resume chain are gone** (4bd, 2026-09-10, ~3,300 net
                  lines, 29 files, 3 files deleted). Every method on the trait was
                  already a `local_only_error()` wall; 4n called this a feature
                  round and it was, but 4az/4bb had already carved out the big
                  parts, so what remained split cleanly along the local/server
                  seam. **Deleted**: the `warp harness-support` CLI (args module,
                  `CliCommand` variant, hidden-subcommand block, dispatch +
                  `is_gui_required` arms, five `HarnessSupport*` telemetry events,
                  parse tests); the trait + `ServerApi` impl +
                  `get_harness_support_client` (**provider down to 5 client
                  getters**); `ResumePayload`/`fetch_resume_payload`/
                  `fetch_transcript_envelope`; the runners' `save_conversation`
                  + `handle_session_update` + `cleanup` and the trait methods
                  themselves, with `SavePoint`, `HarnessCleanupDisposition`,
                  `HARNESS_SAVE_INTERVAL`, `ResumeOptions::ThirdParty`, the
                  driver's `resume_payload` slot, `TerminalDriver::block_snapshot`,
                  and `SetupStep::ThirdPartyHarnessExternalConversation`;
                  parent_bridge's `MessageBridge` runtime + hook-output rendering
                  (762 → 278 lines); `agent_events`' `bounded_run_ids` and its two
                  constants (the bridge was the only consumer); and the CLI's
                  ambient-task context kick in `agent_sdk/common`. **Collapses**:
                  `prepare_harness`'s `AgentRunPrompt::ServerSide` arm fails
                  directly with `PromptResolutionFailed`; `load_conversation_information`'s
                  third-party arm errors instead of fetching a server transcript.
                  **Kept, and why**: the runner world itself (claude_code/codex/
                  gemini build + launch + `/exit`) now executes as purely local
                  runs; the wake path stays, so `resolve_prompt_for_task`/
                  `fetch_transcript_for_task` and parent_bridge's staging
                  helpers stay in `server_api/harness_support.rs` (which is now
                  just those helpers plus the `UploadTarget` family — note the
                  attachment-upload client surface (`prepare_attachment_uploads`,
                  `resolve_upload_target`, `upload_to_target`) turned out to be
                  prod-orphaned at HEAD and is deliberately left standing as its
                  own slice); the transcript reader halves (`read_envelope`,
                  `find_session_file`, `parse_session_meta`, …) carry
                  `#[cfg_attr(not(test), allow(dead_code))]` as the
                  on-disk-contract guards for the live write paths, the same
                  pattern as `AgentEventConsumerControlFlow::Stop`. The
                  in-app CLI-agent surfaces (harness_availability, local child
                  launch, workspace transcript rehydration) are untouched, and
                  the `oz-harness-support` *plugin* name belongs to that world.
                  Acceptance: check `--all-targets` clean in both presubmit
                  configs with the warning set **identical to HEAD** (lib
                  baseline still exactly 4); clippy `-p warp` 0 errors in both;
                  format clean; nextest 8736/8729 with the six known
                  environmental ssh/palette failures plus `test_ctrl_c`, which
                  passes in isolation (full-suite load flake, verified on the
                  work tree).

      - [x] **The prod-orphaned attachment-upload client surface is gone** (4be, 2026-09-10, 6
            files, +37/-342). `prepare_attachments_for_upload` and
            `download_task_attachments` (both `local_only_error()` walls) had zero production
            callers, and `resolve_upload_target`/`upload_to_target` were driven only by their own
            tests. Deleted the two AIClient methods with their impl stubs and wire types,
            `UploadTarget` and its normalization, `upload_to_target`, and the harness_support
            attachment serde tests. `UploadField`/`UploadFieldValue` moved to
            `presigned_upload.rs`, home of the live artifact-upload path. Acceptance: check both
            configs identical to HEAD, clippy both configs baseline-only, format clean; nextest
            8730 run / 8724 passed with only the six known environmental failures (five ssh,
            palette).
      - [x] **Managed secrets and the auth-secret UI chain are gone** (4bf, 2026-09-10, 44 files,
            +88/-4219). The client trait fell in 4r; auth secrets are fetched and created against
            a server this build does not have. Deleted the `warp_managed_secrets` crate, the
            GraphQL `ManagedSecretType` module, the FTUX creation UI (ftux view, dropdown,
            composer selector, `auth_secret_types.rs`, workspace "New API key" modal), the
            driver's secret injection (hardcoded-empty `secrets`), MCP's `apply_secrets`, and the
            CreateNew picker flow end to end. Kept the orchestration picker shell, the
            `harness_auth_secrets` wire field, and `SecretRef`. Acceptance: check/clippy both
            configs baseline-only, format clean; nextest 8688 run / 8681 passed — six
            environmental plus `test_with_launch_config_with_active_pane` (passes in isolation,
            load flake).
      - [x] **ObjectClient, the sync queue, and the sharing dialog are gone** (4bg, 2026-09-14,
            151 files, +812/-26569, 18 files deleted). The last client-trait vertical before
            AIClient/AuthClient: every ObjectClient method was a wall, and the sync machinery
            around it only fed that wall. Deleted `server_api/object.rs` (provider down to 2
            getters: auth + ai), `sync_queue.rs` + tests, UpdateManager initial-load tracking,
            `cloud_objects/listener`/`fake_object_client`/`test_utils`, the
            `cloud_object_client` crate, `sharing/dialog` + style, `word_block_editor.rs`,
            `grab_edit_access_modal`, and the integration websockets module with its 4 registered
            tests and 4 nextest names. `UpdateManager::new` takes the persistence sender only;
            the syncer decoupled from SyncQueue/UpdateManager events. Local persistence stays.
            Four acceptance repairs, all suite-caught: CloudModel startup registration restored
            (every integration test died at launch); migration/activation waits re-pointed at the
            syncer flag; the notebook baton auto-grab (which waited on the same dead load signal
            via an already-ready async block, flipping open into edit mode) removed with its
            tests re-pinned to view-on-open; profiles/notebook test graphs given the syncer and
            WarpDrivePrivacySettings singletons. Acceptance: check both configs identical to HEAD
            (lib 4 baseline), clippy `-p warp` 0 errors, format clean; nextest 8564 run / 8558
            passed with only the six known environmental failures (five ssh, palette). The
            mermaid backspace integration test caught the baton regression (fails pre-fix, passes
            on HEAD and after). simplewarp bin check clean.
      - [x] **AIClient, first five slices (4bh–4bl, 2026-09-14, ~2,100 net lines): 62 trait
            methods down to 34.** 4bh took the 7 zero-caller walls (usage history, merkle/
            embeddings, conversation format, block snapshot, conversation delete, agent-event
            report) with their orphaned wire types. 4bi/4bj/4bl took three whole CLI verticals
            that only ever reached walls — `warp memory-store`/`warp memory` (9 methods),
            `warp agent list/get/create/update/delete` (9 methods), `warp agent skills`
            (1 method) — each with its clap args, dispatch/auth/telemetry arms, parse tests,
            and wire types. 4bk took the two warn-and-continue fire-and-forget walls
            (setup-observability event post, cursor PATCH) and collapsed the reporter struct
            and `HarnessRunner::start` chain around them. Standard acceptance every round;
            warning set identical to baseline throughout (each round's cascade — SecretRef,
            skill wire cluster, mock `expect_` shapes the method-name grep misses, an
            `is_gui_required`-style second match in `agent_sdk/mod.rs` — caught by check,
            not by inspection).
      - [ ] **AIClient, the rest (4bm survey, no code change): the remaining 34 methods all
            have live callers with error fallbacks, so each is a feature round per 4n, not
            client cleanup.** Traced every one to its caller and classified, so the next
            attempts do not repeat it: cloud-run lifecycle (`create/update_agent_task`,
            `spawn_agent`, `submit_run_followup`, `cancel_ambient_agent_task`,
            `list/get_ambient_agent_task`, `list/get_agent_run_raw(s)`,
            `download_run_transcript_to_path`, `get_task_git_credentials`) — live ambient
            terminal UI and driver paths; conversation sync (`fork/rename_conversation`,
            `get_ai_conversation`, `list_ai_conversation_metadata`, `send/list/read/mark`
            messages, `get_public/run_conversation`) — `--conversation` resume and
            orchestration messaging; artifacts (`create/confirm_file_artifact_upload_target`,
            `get_artifact_download`) — the live upload pipeline's wall steps around the live
            presigned-S3 middle; assistant surfaces (`generate_commands_from_natural_language`
            palette search, `generate_dialogue_answer` assistant panel,
            `generate_metadata_for_command` workflow AI-assist, `provide_negative_feedback`
            refund-credits UI); code-review generation (`generate_code_review_content` —
            dialog plus daemon flows with a `--fill` fallback on only one branch);
            login-gated refreshes (`get_request_limit_info`, `get_feature_model_choices`,
            `get_available_harnesses`, `list_connected_self_hosted_workers` — dead in
            practice via `is_logged_in`, live in shape for other channels, the 3k question
            again); `get_free_available_models` (ungated but reached only through the
            `agent_mode_evals`-gated startup fetch, a CI-checked feature). After these go,
            the chain is `AuthClient` (external-crate trait, entangled with `AuthManager`),
            then `ServerApi`/`Provider`/`BaseClient`, then the crates.

            Every round ran the standard acceptance — check both feature sets, clippy 0 errors,
            format clean, nextest green (5707 → 5519 as each deleted subject's tests went with
            it) — and the later rounds built and launched the app.
      - [x] **AuthClient, first wave (4bn, 2026-09-14, 21 files, +50/−959): 18 trait methods
            down to 12, `warp_server_client` loses its `oauth2` dependency.** The chain's
            next link, taken as four slices:

            1. *Zero-caller trait surface.* `AuthClient::fetch_user_properties` had no caller
               outside the impl — demoted to an inherent `AuthClientImpl` method (the trait
               shape only served the mock). `ServerApi::send_graphql_request`, the one wall on
               `ServerApi` proper with zero callers, deleted with its `BoxFuture` import.
            2. *`create_anonymous_user`.* Its last production caller was the debug global
               action `workspace:debug_create_anonymous_user` ("Create anonymous user" in the
               debug menu) — 3o already deleted every real creation path. Deleted the action,
               the menu item, the trait method, and the impl.
            3. *Custom-token minting.* `fetch_new_custom_token` + `on_custom_token_fetched`
               were reachable only through `AuthManager::initiate_anonymous_user_linking` and
               `open_url_maybe_with_anonymous_token` — the first only from the Drive
               personal-object-limit card, unreachable since `personal_object_limits()`
               populates solely from a warp-server user fetch (deleted the card, its two
               actions, `render_personal_object_limit_row`, and the overlay block, ~330 lines
               of provably dead rendering); the second only from the privacy page's data
               management link, whose anonymous branch requires an anonymous *logged-in*
               user, which this build cannot have. The page now opens the plain
               `data_management_url()` directly. Gone with them: `MintCustomTokenError`,
               `AuthManagerEvent::MintCustomTokenFailed` (its two `|`-grouped matchers
               trimmed), `login_options_url`, `URLConstructorCallback`, and the wasm-only
               `parse_current_url`/`update_browser_url` branch.
            4. *Device auth.* `warp login` was the sole consumer of
               `request_device_code`/`exchange_device_access_token` — the 4o precedent
               applies (a CLI vertical that can never work without warp-server's OAuth
               endpoints). Deleted `CliCommand::Login` with its dispatch/auth/telemetry arms,
               `admin::login`, `AuthManager::authorize_device`/`on_device_code_received`/
               `request_device_code_with_timeout`, `AuthManagerEvent::ReceivedDeviceAuthorizationCode`,
               `AuthSession`'s device methods and its whole `oauth_client` (the crate's last
               `oauth2` use — dependency dropped), and the `device_authorization_uses_warp_agent_cli_client`
               test. The 4m landmine was checked up front: grepping the literal `"login"`
               found only GitHub-JSON parsing and the parse tests, which were re-pointed at
               `whoami` rather than deleted (they test global-flag parsing, not login).

            What remains on `AuthClient` is live in shape: `get_or_refresh_access_token`/
            `fetch_user` (load-bearing `AuthManager` + remote-server auth context), the
            privacy-settings sync group (`get_user_settings`, the three `set_is_*`,
            `update_user_settings` — all `is_logged_in`-gated server pushes), API-key
            management (`list/create/expire_api_key`, `list_agent_identities` —
            `api_key_management` is in the simplewarp feature set), `set_user_is_onboarded`,
            and the custom-token pair's remaining sibling surfaces. Each is a feature round
            per 4n; the trait itself cannot shrink further without them.

            Acceptance: check both feature sets at the 4-warning lib baseline (one test-file
            import trimmed along the way; the notebook_tests `Duration` warning is the known
            4bg leftover), clippy 0 errors in both, format clean, nextest green (warp lib
            5284, `warp_server_client`+`warp_cli` 167). Not re-run in the app — no reachable
            surface changed: the deleted UI was all provably unreachable, and the privacy
            page's link still opens the same URL it always opened in a logged-out client.
      - [x] **The `CLIAgentRichInput` flag is folded away** (4bo, 2026-09-14, 48 files,
            +1,065/−5,220 — the first flag-vertical round since 4bb): the
            `cli_agent_rich_input` cargo feature was absent from the simplewarp set, so the
            rich-input composer was permanently inert there — `open_cli_agent_rich_input`
            always early-returned, `open_input` had no other production caller, and the whole
            input-session lifecycle was unreachable. Deleted the flag with the vertical:
            the input-session half of `CLIAgentSessionsModel` (`CLIAgentInputState`,
            `CLIAgentInputEntrypoint`, `CLIAgentRichInputCloseReason`, `open_input`/
            `close_input`, drafts, `is_input_open`, `InputSessionChanged` + its six
            subscriber arms, and the `input_state`/`should_auto_toggle_input`/`draft_text`
            session fields), the composer flow in `use_agent_footer` and the footer
            RichInput button/chip (`AgentToolbarItemKind::RichInput` stays in the persisted
            toolbar enum with `is_available()` false, HandoffToCloud precedent), all of
            `terminal/input/cli_agent.rs`, the Ctrl-G binding with its context flags,
            four settings with their page rows (`auto_toggle_rich_input`,
            `auto_open_rich_input_on_cli_agent_start`,
            `auto_dismiss_rich_input_after_submit`, `submit_on_ctrl_enter`), the three
            `CLIAgentRichInput*` telemetry events, and the `hide_cursor_cell` render
            plumbing whose only writer was the rich-input cursor suppression.
            **Kept**: the session-detection half of the sessions model (status, listeners,
            Ctrl-C cancel windows), `write_cli_agent_text*` for the live driver
            `submit_text_to_cli_agent_pty` path, `submit_text_to_cli_agent_pty` itself,
            and live-path test coverage (dropped-image paste, voice insert-to-PTY now
            unconditional, status→conversation-status, Ctrl-C arming) — five tests
            initially over-deleted with the rich-input batch were recovered from HEAD and
            re-kept with the dead fields stripped. `SkillManager`'s provider-variant
            helpers (`skill_exists_for_any_provider`, `best_supported_provider`) went with
            the CLI-agent skill filtering they served. Also removed two dead
            `drive/index_tests` helpers (`create_workflow`, `label_for_menu_item`) that
            were already dead on HEAD's clippy baseline.

            Acceptance: check both feature sets at the 4-warning baseline (verified
            byte-identical to HEAD via stash diff), clippy error set a strict subset of the
            HEAD baseline in both configs, format clean, nextest green (warp lib 5250
            default / 5248 simplewarp vs 5284 before — delta = tests deleted with their
            subject; `warp_server_client`+`warp_cli`+`warp_features` 168). Not re-run in
            the app — no reachable surface changed in the product build. **Next flag
            targets by site count**: `WarpControlCli` (24), `GeminiEnterprise` (23,
            live-by-design per 4at), `EditableMarkdownMermaid` (23); 98 constant-false
            flags remain of 208 enum variants.
      - [x] **The `WarpControlCli` flag is folded away** (4bp, 2026-09-14, 69 files,
            +56/−12,194): the `warp_control_cli` cargo feature was absent from the
            simplewarp set, so Warp Control — the app-side local-control server, the
            `warpctrl` CLI client it serves, and the Scripting page that enabled them —
            was permanently inert. The whole vertical went, end to end:

            - *Server side*: all of `app/src/local_control/` (Axum listener, credential
              broker, resolver, permissions, the allowlisted action handlers), the
              `LocalControlBridge`/`LocalControlServer` singleton registration and the
              `--warpctrl` early-dispatch block in `lib.rs`, and `LocalControlSettings`
              with its tests (its only readers were the flag-gated server and page).
            - *Settings UI*: the whole Scripting page (`scripting_page.rs`, install
              widget, `local_control_mode` dropdown), the `SettingsSection::Scripting`
              variant with its nav insert, page-handle arm, slug/from_slug spellings,
              and the `from_slug` doc comment's warpctrl mention (persisted-session
              compat stays — the warpctrl callers do not).
            - *CLI side*: all of `crates/warp_cli/src/local_control/` (`warpctrl`
              parser, `warp control` command tree, run-and-exit client) with its 732
              test lines, the `local_control` crate itself (protocol, discovery,
              catalog, auth), both dependency edges, and `Channel::warpctrl_command_name`.
            - *Periphery*: `InstallWarpctrl`/`UninstallWarpctrl` actions, bindings, and
              view methods; the `cli_install.rs` warpctrl fns with `path_resolves_to`
              (its only production caller) and the single-test file it left behind;
              the `warpctrl` bundled skill (resources dir, template variables,
              activation arm); the DOGFOOD_FLAGS entry, the `features.rs` mapping, the
              cargo feature, and the wrapper-creating branches in the macOS/Linux
              bundle and run scripts.

            Tests re-pointed rather than deleted where they covered live behavior:
            the RequiresFeature gating tests in `skill_manager_tests` and
            `read_skill_tests` now use `FactoryMcp` (already the sibling example);
            the redundant `warp_control_bundled_skill_activations_track_warp_control_feature`
            test was dropped. `specs/warp-control-cli/` stays — no spec has ever been
            deleted in this repo, including the TUI's.

            Acceptance: check both feature sets at the 4-warning lib baseline (same
            four warnings as HEAD: `is_transient_graphql_or_http_error`,
            `reset_unknown`, `should_preserve_onboarding_profile`, `Loaded`), clippy
            error set byte-identical to HEAD in both configs via stash diff (8 pairs
            per config, including the pre-existing `for loop over a single element`
            in lifecycle tests), format clean, nextest green (warp lib 5213 default /
            5211 simplewarp vs 5250/5248 — delta = tests deleted with their subject;
            `warp_cli`+`warp_features`+`warp_server_client` 149). Not re-run in the
            app — every deleted surface was unreachable in the product build.
            **Next flag targets by site count**: `EditableMarkdownMermaid` (23;
            `GeminiEnterprise` (23) is live-by-design per 4at — treat carefully);
            97 constant-false flags remain of 207 enum variants.
      - [x] **The Autoupdate vertical is deleted** (4br, 2026-09-15, 50 files,
            +42/−4,771): first round under the remote-dependent-only scope. The
            update pipeline talked to the Warp server (`/client_version[/daily]`
            via `ServerApi::fetch_channel_versions`, installers from
            `releases_base_url`), and the `autoupdate` cargo feature was absent
            from both the default and simplewarp sets — the polling loop never
            started in any SimpleWarp binary. Gone end to end:

            - *Core*: all of `app/src/autoupdate/` (3,676 lines — the
              `AutoupdateState` request-queue/download state machine, the
              macOS/Windows/Linux installers with their platform tests, and the
              `fetch_channel_versions` wrapper), `AutoupdateState::register`,
              the `RelaunchModel` singleton with the terminate-path
              `apply_pending_update`/`spawn_child_if_necessary`/`cancel_relaunch`
              hooks, `remove_old_executable`, the startup `check_and_report_update_errors`
              call, and `AppExecutionMode::can_autoupdate` (no other callers).
            - *Server-side surface*: `ServerApi::fetch_channel_versions`,
              `ServerApi::server_time`, and the `ServerTime` type — the
              server-time chain existed only to date the "your build expired"
              nag, so `RootView`/`Workspace`/`WorkspaceArgs` lost their
              `server_time` fields and `set_server_time`.
            - *UI*: the tab-bar overflow "Update Warp" menu with its pill
              button, `ToggleTabBarOverflowMenu` action, overflow-menu getter,
              and the `AutoupdateUIRevamp` avatar red dot (the
              `AutoupdateUIRevamp` flag folded away with them); the
              autoupdate workspace banners (`UnableToUpdateToNewVersion`,
              `UnableToLaunchNewVersion`, `VersionDeprecated`) with their
              dismissed flags; five `WorkspaceAction`s (`ApplyUpdate`,
              `CheckForUpdate`, `DownloadNewVersion`, `AutoupdateFailureLink`,
              `ToggleTabBarOverflowMenu`) and the assisted-update Linux tab
              (`add_tab_for_assisted_autoupdate`) plus the
              `AutoupdateState_UpdateReady` dynamic context id; the settings
              Account page's update status/CTA (`VersionInfoWidget` keeps the
              plain version + copy row) and the
              `MainPageAction`/`MainSettingsPageEvent`/`SettingsViewEvent`
              `CheckForUpdate` chain; the two editable bindings and
              `BindingGroup::AutoUpdate`; `Icon::AutoUpdate` with its svg.
            - *Config & flags*: `AutoupdateConfig` and the
              `autoupdate_config` channel-config field (the `None`s in the four
              bins went with it), `ChannelState::releases_base_url`/
              `show_autoupdate_menu_items`, `ContextFlag::PromptForVersionUpdates`
              (variant, five `disable_flag` calls, `FromStr` arm),
              `FeatureFlag::Autoupdate` (also removed from `RELEASE_FLAGS` —
              release bundles of the default build lose autoupdate too, which
              is the point) and `AutoupdateUIRevamp`, both cargo features, and
              the two `features.rs` mappings.
            - *Periphery*: the `--finish-update` CLI arg with
              `finish_update_flag()`, the `IgnoredAfterAutoUpdate` arg-forward
              error and its match arms, `AppStartupInfo::from_relaunch`, the
              `warp_finish_update` DCS hook chain (`DProtoHook::FinishUpdate` →
              `FinishUpdateValue` → `ModelEvent::FinishUpdate` → the
              terminal-view relaunch arm), the debug dump's Linux package-type
              line, and seven telemetry events (two generic, five
              Windows-installer) — no emitters remain. The stale
              `APPIMAGE_NAME` comment in `script/linux/bundle` was trimmed; the
              export itself stays (bundle_appimage names the artifact with it).

            **Kept**: the `channel_versions` crate (live users in warpify,
            block context, warp_terminal, secure_storage) — only the
            autoupdate-local wrapper and `is_incoming_version_past_current`
            went. The integration test that asserted "overflow menu not
            showing" was re-pointed at `assert_context_menu_is_open(false)` —
            the old assertion had always been trivially true.

            Acceptance: check both feature sets at the 4-warning lib baseline
            (same four as HEAD), clippy 0 errors in both configs (warnings =
            the known set plus nothing new), format clean, nextest green (warp
            lib 5202 default / 5201 simplewarp vs 5213/5211 — delta = the
            deleted module's tests; `warp_cli`+`warp_features`+`warp_core`+
            `warp_server_client` 198). Not re-run in the app — no reachable
            surface changed: every deleted UI sat behind the never-enabled
            flag, and the version row in Settings still shows the same string.
            **Remaining remote-dependent queue**: `PromptSuggestionsViaMAA`,
            `PredictAMQueries`, `RemoteCodebaseIndexing`.
      - [x] **Remote codebase indexing is deleted** (4bs, 2026-09-15, 28 files,
            +46/−2,104): second round under the remote-dependent-only scope.
            The flag ("codebase indexing inside remote server daemon
            processes") gated a full daemon↔client vertical for indexing repos
            on remote hosts and syncing embeddings through the Warp indexing
            backend (`CodebaseIndexManagerConfig` carries the `store_client`);
            the cargo feature was in the default set only, so SimpleWarp builds
            never had it. Gone end to end:

            - *Daemon*: all four request handlers (`IndexCodebase`,
              `ResyncCodebase`, `DropCodebaseIndex`,
              `GetFragmentMetadataFromHash`) with
              `prepare_codebase_index_request`,
              `validate_remote_codebase_index_auth`,
              `validate_fragment_metadata_lookup`, the
              `FragmentMetadataLookupError` response mapping, and the
              proto-conversion free fns; the bootstrap statuses snapshot and
              `CodebaseIndexManagerEvent` subscription with the status-push
              helpers; `apply_codebase_index_limits` and its
              `Initialize`/`UpdatePreferences` call sites; and
              `codebase_index_status.rs` with its tests (whole file served
              this vertical).
            - *Client*: `RemoteServerManager`'s `mutate_codebase_index` family
              (`ensure_codebase_indexed`, `resync_codebase`,
              `trigger_codebase_incremental_sync`, `drop_codebase_index`),
              `get_fragment_metadata_from_hash`,
              `update_codebase_index_limits`, the
              `RemoteCodebaseIndexUpdateOperation` enum, the
              `CodebaseIndexStatusesSnapshot`/`CodebaseIndexStatusUpdated`/
              `CodebaseIndexMutationFailed` events with their session-id arms,
              `remote_path_for_status`,
              `RemoteCodebaseIndexStatusWithPath`,
              `connected_session_for_host` (only caller was
              `mutate_codebase_index`), and the `codebase_index_limits` field
              threaded through `ReconnectParams`, `run_connect_and_handshake`,
              and `InitializeParams`; `RemoteServerClient` loses the two
              status-decode arms and the `UpdatePreferences` limits argument;
              `codebase_index_proto.rs` with its tests (whole module).
            - *Limits sync*: `current_codebase_index_limits` and the
              `wire_auth_token_rotation` plumbing that pushed app-resolved
              limits to daemons (auth-token rotation and crash-report
              forwarding stay); `Initialize`/`UpdatePreferences` lose the
              `codebase_index_limits` field.
            - *Protocol*: 15 messages/enums removed from
              `remote_server.proto` (`IndexCodebase`, `ResyncCodebase`,
              `CodebaseResyncMode`, `DropCodebaseIndex`,
              `GetFragmentMetadataFromHash` + its success/response/error
              chain, `FragmentMetadata`, `MissingFragmentMetadata`,
              `CodebaseIndexStatus*`, `CodebaseIndexLimits`), oneof slots 9–11
              and 13 (host-scoped), 12–13 and 24 (server messages), and field
              5/2 reserved.
            - *Agent API*: `SearchCodebase` is no longer advertised for
              `WarpifiedRemote` sessions (the flag never added it locally);
              the flag-off and not-connected tests re-point into two
              omission tests, the flag-on test deleted.
            - *Flag*: the variant and its DOGFOOD_FLAGS entry, the cargo
              feature (removed from the default set; `features.rs` mapping
              gone), and `supports_indexing()`'s daemon arm, now a plain
              `false` like the proxy.
            - *Cascade into local indexing*: the daemon-only deferred restore
              path (`defer_persisted_index_restore`,
              `start_persisted_index_restore`,
              `restore_persisted_indices_on_startup`,
              `BuildQueue::start`/`BuildQueueState::Paused` — the queue is now
              unconditionally running), `update_max_limits` and
              `update_embedding_generation_batch_size` (runtime limit updates
              only existed to serve daemons), and
              `daemon_codebase_index_snapshot_storage` in `app/src/lib.rs`
              (`new_with_snapshot_storage` stays — test-covered app-default
              path).
            - *Periphery*: two dead telemetry events with no emitters
              (`RemoteCodebaseIndex.StatusChanged`,
              `RemoteCodebaseIndex.AutoIndexRequested`) with their
              `RemoteCodebaseIndexStatusTelemetrySource`/
              `RemoteCodebaseAutoIndexTrigger` enums, and the three
              ignore-arms over `RemoteServerManagerEvent` (terminal session,
              terminal view, remote server controller).

            `specs/APP-3792/` stays — no spec has ever been deleted in this
            repo.

            Acceptance: check both feature sets at the 4-warning lib baseline,
            clippy output byte-identical to HEAD in both configs via stash
            diff, format clean, nextest green (warp lib 5194 default / 5193
            simplewarp vs 5202/5201 — delta = tests deleted with their
            subject; `remote_server`+`warp_features`+`ai` 426). Not re-run in
            the app — remote-host indexing was unreachable in SimpleWarp (the
            cargo feature was not in the simplewarp set) and no local surface
            changed: the settings Code Indexing page, agent SDK, and
            init-project flows use the same `CodebaseIndexManager` methods as
            before.
            **Remaining remote-dependent queue**: `PromptSuggestionsViaMAA`,
            `PredictAMQueries`.
      - [x] **Prompt suggestions via MAA are deleted** (4bt, 2026-09-15, 29 files,
            +55/−2,112): third round under the remote-dependent-only scope.
            The flag ("prompt suggestions sourced via MAA") gated the
            server-driven suggestion path — the `MaaPassiveSuggestionsModel`
            issued `PassiveSuggestions` requests at the Warp backend and
            rendered the returned prompt/code-diff banners; the cargo feature
            was in neither the default nor the simplewarp set, so the path
            never ran in any SimpleWarp binary. Gone end to end:

            - *Model*: `passive_suggestions/maa.rs` with its tests (whole
              modules served this vertical) and the `PassiveSuggestionsModels`
              pair struct — `TerminalView` keeps a single
              `legacy_passive_suggestions_model` with its existing event
              subscription; the local legacy path (static + AI-query
              suggestions, passive code diffs) is untouched.
            - *Agent plumbing*: `AIAgentInput::PassiveSuggestionResult` with
              `PassiveSuggestionResultType`/`PassiveCodeDiffEntry`, its
              `Display`/`query-text` arms, the `convert_to` MAA-proto
              conversion, the `convert_conversation` transcript arms, the
              redaction arms, the persistence `Query` mapping, the
              controller's `pending_passive_suggestion_results` queue with
              `send_passive_suggestion_result` and the drain/append sites,
              and the telemetry `AIAgentInput::PassiveSuggestionResult`
              event.
            - *Action*: the `SuggestPrompt` executor's flag-gated
              `NewPromptSuggestion` emit is gone (the action still resolves
              through its result channel); `PromptSuggestionExecutorEvent`
              collapses to `()` with zero remaining references.
            - *UI*: the `passive_code_diff` inline banner module (whole
              file), the prompt-suggestion banner's dogfood-only server
              request token with its debug-link token plumbing and the
              `CopyServerRequestId` context-menu action (no other
              constructors), and the MAA-only `DiffStorageHelper`/`FileDiff`
              imports.
            - *Flag*: the variant, the cargo feature, and the `features.rs`
              mapping.

            `maa_api` (`warp_multi_agent_api`) usages stay — that is the
            multi-agent transport, a different thing from MAA-sourced
            suggestions.

            Acceptance: check both feature sets at the 4-warning lib baseline
            (same four as HEAD), clippy 0 errors in both configs (warnings =
            the known set plus the pre-existing lifecycle `for loop over a
            single element`), format clean, nextest green (warp lib 5191
            default / 5190 simplewarp vs 5194/5193 — delta = the 3 tests
            deleted with their subject; `warp_features`+`warp_server_client`+
            `warp_core` 72; dependent crates
            `warp_features`/`input_classifier`/`settings`/`warp_core` check
            clean). Not re-run in the app — the MAA path sat behind a cargo
            feature in neither build set, and the legacy suggestion banners
            use the same render code as before.
            **Remaining remote-dependent queue**: `PredictAMQueries`.
      - [x] **Predicted Agent Mode queries are deleted** (4bu, 2026-09-15, 16
            files, +1/−326): fourth and final round under the
            remote-dependent-only scope — the queue is now empty.
            The flag ("prediction of Agent Mode queries") gated the
            `PredictAMQueries` warp-server endpoint chain: after each block
            completed, the input debounced a request carrying the block's
            summarized command/output plus the partial AI-query buffer, and
            rendered the returned suggestion as ghosted text. The cargo
            feature was in neither the default nor the simplewarp set, so the
            chain never ran in any SimpleWarp binary. Gone end to end:

            - *Request chain*: `ai/predict/predict_am_queries/` with its
              request/response wire types (whole modules served this round),
              `ServerApi::predict_am_queries`, and in `terminal/input.rs` the
              `predict_am_query` method, the
              `DEBOUNCE_AI_QUERY_PREDICTION_PERIOD` debounce channel + worker,
              `predict_am_queries_future_handle`, the
              `is_nl_ai_autosuggestion_triggering_event` helper (sole caller
              was the trigger block), and the flag-gated trigger in
              `handle_editor_event`.
            - *Setting*: `natural_language_autosuggestions_enabled_internal`
              with its `is_natural_language_autosuggestions_enabled` getter —
              after the chain died it had zero behavioral consumers (the
              accept-path for `AutosuggestionType::AgentModeQuery` never
              checked it), so the whole user-facing toggle went with it: the
              Active AI section, `ToggleNaturalLanguageAutosuggestions` action
              + binding pair, the `NATURAL_LANGUAGE_AUTOSUGGESTIONS_FLAG`
              context flag (its only consumer was the toggle's own
              search-binding registration), and the
              `ToggleNaturalLanguageAutosuggestionsSetting` telemetry event
              whose `EnablementState` was this flag.
            - *Periphery*: the integration-testing user-defaults key, and the
              workspace context-flag insertion. `WarpAiExecutionContext`,
              `last_user_block_completed`, and the `AgentModeQuery`
              autosuggestion type all keep other live users (next-command
              model, ask-AI flow, legacy prompt suggestions) and stay.

            Acceptance: check both feature sets at the 4-warning lib baseline
            (same four as HEAD — two transient dead-code warnings from this
            round's own deletions were also removed: the `json!` import and
            `Input`'s `server_api` field, whose last reader was
            `predict_am_query`; the constructor parameter stays,
            `NextCommandModel` still consumes it), clippy 0 errors in both
            configs (warnings = the known set, all pre-existing), format
            clean, nextest green (warp lib 5191 default / 5190 simplewarp —
            identical to HEAD; no tests referenced the deleted code).
            Not re-run in the app — the flag was off in every SimpleWarp
            binary, so the ghost-text path and the Settings toggle were
            already invisible/inert; no reachable surface changed.
            **Remaining remote-dependent queue**: empty — remote-dependent
            flag rounds complete; next up per plan: feature rounds for
            local-but-disabled flags.
      - [x] **Server LLM model directory refresh chain is deleted** (4bv,
            2026-09-15, 25 files, +65/−1580, `7b883bd47`): takes two of the
            4bm-surveyed AIClient methods — `get_feature_model_choices`
            (login-gated refresh group) and `get_free_available_models`
            (ungated but reached only through the `agent_mode_evals`-gated
            startup fetch) — plus the entire consumption chain. AIClient
            34 → 32 methods. Both were pure-remote paths: with no Warp
            server, `LLMPreferences` now starts from the compiled-in
            `ModelsByFeature::default()` catalog with local providers,
            custom endpoints, and custom routers layered on top. Gone end
            to end:

            - *Client surface*: the two `AIClient` trait methods and their
              `ServerApi` local-only stubs, with all `TryFrom`/`From`
              conversion impls in `server_api/ai.rs`
              (`FeatureModelChoice`/`AvailableLlms`/`LlmInfo`/
              `RoutingHostConfig`/`LlmSpec`/`LlmUsageMetadata`/
              `DisableReason` from both the query and workspace shapes).
            - *GraphQL*: `free_available_models.rs` and
              `get_feature_model_choices.rs` deleted, the `llms` selection
              removed from `get_user.rs`, and `workspace.rs` loses
              `FeatureModelChoice`/`AvailableLlms`/`LlmInfo`/`LlmPricing`/
              `LlmSpec`/`LlmUsageMetadata`/`DisableReason`
              (`LlmProvider`/`LlmModelHost` stay — still used by the
              surviving workspace conversions, e.g. `crates/ai`).
            - *Preferences*: the server refresh paths
              (`refresh_authed_models`/`refresh_public_models`/
              `refresh_available_models`, `update_feature_model_choices`,
              `on_server_update`, `get_cached_models` with
              `MODELS_BY_FEATURE_CACHE_KEY`), the
              `agent_mode_models_unavailable` flag with its getter/setter,
              the new-model popup state (`AvailableLLMsUpdate`,
              `new_choices_since_last_update`,
              `should_show/mark/hide_*_popup`), and
              `sanitize_disabled_custom_model_preferences` (subsumed by the
              surviving `reconcile_disabled_model_preferences`); the
              constructor no longer subscribes to auth/network/workspaces
              for refreshes. `set_models_by_feature_for_test` added so
              tests pin the catalog directly.
            - *Consumers*: the `AuthManager` `llms` passthrough with
              `UserProperties::llms`; the controller's
              `should_refresh_available_llms_on_stream_finish` and
              `should_refresh_model_config` refresh arms; the agent-SDK
              classifier's list-unavailable branch (genuinely-unknown ids
              still get "Unknown model id" with suggestions);
              integration-testing assertions/steps simplified to the same;
              the `ProfileModelSelector` popup with its `input_model`/
              `controller` parameters (callers in `terminal/input.rs`,
              `universal_developer_input.rs`, `agent_input_footer`
              trimmed); `BlocklistAIInputModel::last_ai_autodetection_ts`;
              `FeaturePopup::FromCallable` (remaining callers all
              `FromString`); `dedupe_model_display_names`. Tests
              re-pointed at `set_models_by_feature_for_test` /
              `reconcile_disabled_model_preferences`; deleted tests covered
              only the removed server behavior.
            - *Kept*: `ModelsByFeature::default()`, provider listing,
              custom endpoints/routers, `AvailableLLMs::new` (tests),
              `LLMInfo::new_for_test` (narrowed to `cfg(test)` —
              integration-testing no longer uses it).

            Acceptance: `check -p warp --tests` clean in both feature sets
            (warnings = the pre-existing set in untouched files),
            `warp_graphql` 7 / `ai` 332 green, targeted warp lib suites
            green (`llms` 31, `agent_sdk::common` 4,
            execution-profiles+ambient 88, `ai::` 1811), format clean.
            Full-workspace nextest is not runnable on this machine
            (missing `xcrun metal` — environmental, pre-existing,
            unrelated to this round); verified via a fake-`xcrun` shim
            that only stubs the shader bytecode step. Clippy under this
            toolchain reports pre-existing errors in untouched files
            (`notebook_tests`, `snapshots`, `profiles`,
            `retry_strategies`, `lifecycle`, `warp_completer`) — no new
            lints from this round.
            **Remaining queue**: the login-gated refresh group is down to
            `get_request_limit_info`, `get_available_harnesses`,
            `list_connected_self_hosted_workers`; then `AuthClient` (12),
            `ServerApi`/`Provider`/`BaseClient`, then the crates.
      - [x] **Zero-caller ServerApi time/channel stubs are deleted** (4bw,
            2026-09-15, 1 file, +0/−34, `6be66104`): 4br deleted the
            callers and left the stubs. `server_time` / `ServerTime` (the
            "your build expired" nag clock) and `fetch_channel_versions`
            (the autoupdate poll entry) had zero callers in any config,
            so the struct, both methods, and the now-unused
            `channel_versions::ChannelVersions` / `chrono` / `instant`
            imports went. The `channel_versions` crate and the
            `app/Cargo.toml` edge stay — `warpify`, `block_context`, and
            `warp_terminal` still use `overrides::TargetOS`.

            Acceptance: `check -p warp --tests` clean in both feature
            sets (warnings byte-identical to HEAD via stash diff: the
            3 pre-existing lib-test warnings), clippy 0 errors in both
            configs with no `server_api` mentions, format clean,
            nextest green (`server_api` 31 both sets,
            `channel_versions` 14). Not re-run in the app — deleted
            code was unreachable (no callers).
            **Remaining queue**: unchanged from 4bv.
      - [x] **Agent-tip analytics stub + self-hosted workers refresh chain
            are deleted** (4bx, 2026-09-16, 19 files, +19/−438,
            `accd2e9c`): takes the last two entries of the 4bv queue.
            `ServerApi::send_agent_tip_shown_analytics_event` was an
            always-`local_only_error` stub — the status-bar gating
            helpers that consulted it (`should_show_*_tip`) and the one
            call site went with it; `TelemetryEvent::AgentTipShown`
            stays (analytics events are catalog-wide). And
            `AIClient::list_connected_self_hosted_workers` plus its
            structs and stub: `ConnectedSelfHostedWorkersModel` and
            every refresh/subscription call site (snapshots,
            host_selector, input, run_agents_card,
            orchestration_config_block) are gone; the host pickers keep
            default/warp/recent/custom. AIClient 32 → 31 methods.

            Acceptance: `check -p warp --tests` clean in both feature
            sets, clippy 0 errors in both configs, format clean,
            nextest green. Not re-run in the app — deleted code was
            unreachable in SimpleWarp (the stub could only error, and
            the workers list required a Warp server).
            **Remaining queue**: the login-gated refresh group is down
            to `get_request_limit_info`, `get_available_harnesses`;
            then `AuthClient` (12), `ServerApi`/`Provider`/`BaseClient`,
            then the crates.
      - [x] **The last two login-gated refresh chains are deleted** (4by,
            2026-09-16, 45 files, +190/−1843, `1ec2452c`): takes
            `get_request_limit_info` and `get_available_harnesses` —
            with them the login-gated refresh group from the 4bv queue
            is complete. AIClient 31 → 29 methods.

            *Request-limit chain*: the `AIClient` method, its two
            `agent_mode_evals`-split local-only stubs, and the
            `GetRequestLimitInfo` GraphQL query. `AIRequestUsageModel`
            keeps only what local code still reads — the default
            `RequestLimitInfo`, `requests_remaining`/
            `has_base_plan_requests_remaining`, the always-true
            `has_any_ai_remaining`, `codebase_context_limits`, and the
            `provide_negative_feedback_response_for_ai_conversation`
            refund path (still a live `AIClient` method, deferred to
            the AIClient round). Deleted from the model:
            `refresh_request_usage`(+`_async`), `update_request_limit_info`,
            `last_update_time`, the `AIRequestLimitInfo` pref cache,
            `bonus_grants` + `BonusGrant`/`BonusGrantScope` + the
            already-orphaned `gql_convert` conversions,
            `ambient_only_credits_remaining`, the whole ambient-credits
            banner state, `AMBIENT_AGENT_TRIAL_CREDIT_THRESHOLD`, and
            the `RequestUsageUpdated` event. `RequestUsageInfo` is gone.

            *Downstream-dead consumers removed with it*: the constructor
            loses `ctx`; refresh blocks in `auth_manager` (AuthComplete),
            `blocklist/controller` (stream finish), the PromptAlertView
            enter-key path in `terminal/input`, the out-of-credits
            modal's non-paid branch, drive `ai_assist` and
            `workflow_view` (AI-metadata assist), and
            `agent_profiles_page::on_page_selected` (trait default
            restored); `search/command_search/warp_ai::on_query_finished`
            had zero callers and went whole; `agent_profiles_page` and
            `blocklist/block` subscriptions narrow to
            `RequestBonusRefunded` (now a single-variant enum);
            `comment_list_view`'s button-sync subscription deleted;
            `remote_server`'s RequestUsageUpdated→crash-prefs forwarder
            deleted; `BonusGrantNotificationModel` (only fired on
            RequestUsageUpdated) deleted with its registration, its
            `GeneralSettings::bonus_grants_shown` key, and the
            workspace-view toast hookup; the settings quota-banner chain
            (`AIRequestQuotaInfo` setting + `CycleInfo`/`BannerState`
            schemars types + `should_display_quota_reset_banner` +
            `mark_quota_banner_as_dismissed` + `update_quota_info` + 9
            tests + the never-shown `display_chip` quota popup +
            `FeaturePopup::AlertIcon`, whose last constructor user it
            was); the old AI-assistant panel's startup fetch,
            `Requests::update_request_limit_info`, the
            `AIAssistantRequestLimitInfo` cache and its logout cleanup,
            and `Requests::ai_client` (never read after the fetch left)
            — which let `AIAssistantPanelView::new` drop the `ai_client`
            parameter end to end.

            *Harness chain*: the `AIClient` method, its stub, and the
            `GetAvailableHarnesses` query. `HarnessAvailabilityModel`
            keeps its reads (`available_harnesses`, `display_name_for`,
            `should_show_harness_selector`, `has_any_enabled_harness`,
            `is_harness_enabled`, `models_for`, and the local
            auth-secrets stub that always resolves `Failed`) but is now
            a static `default_harnesses()` (Oz): the refresh/cache/
            `normalize_harness_display_names` machinery, the
            NetworkStatus/AuthManager/UserWorkspaces subscriptions, and
            the now-unconstructible `Changed` event variant with its
            subscribers went (the two orchestration pickers resubscribe
            for `AuthSecretsFetchFailed` alone);
            `invalidate_auth_secrets` had no caller left.

            Acceptance: `check -p warp --tests` clean in both feature
            sets (warnings = the pre-existing set), clippy shows no new
            findings — one new lint from this round's own edit
            (`Option::map` in agent_message_bar) folded; the 13
            `needless_return` lints in `terminal/input.rs` sit in code
            this round did not touch, exposed by a cold clippy cache in
            the simplewarp config. `./script/format` clean. nextest
            green: warp lib 5170 default / 5169 simplewarp
            (`--no-fail-fast`; one unrelated notebooks flake on the
            first pass, clean on rerun), warp_graphql 7. Not re-run in
            the app — with no login every refresh short-circuited, so
            `bonus_grants` was always empty and the harness list always
            the default; no reachable surface changed.
            **Remaining queue**: login-gated refresh group complete;
            next is `AuthClient` (12), then
            `ServerApi`/`Provider`/`BaseClient`, then the crates.
      - [x] **Seven dead AuthClient surfaces are deleted** (4bz, 2026-09-16,
            20 files, +14/−2736, `72d9fa1e`): the AuthClient round.
            Trait 12 → 5 methods — `get_or_refresh_access_token`/`fetch_user`
            (load-bearing: `AuthManager` refresh/API-key auth, remote-server
            auth context) and `list_api_keys`/`create_api_key`/
            `expire_api_key` (the `warp api-key` CLI subcommand, live via
            `api_key_management` in the simplewarp set — `--agent-uid` comes
            straight from a flag, never from identities).

            *Privacy-settings sync group* (`get_user_settings`, the three
            `set_is_*`, `update_user_settings`): all `is_logged_in`-gated
            server pushes behind `PrivacySettings`, so the 4bv–4by precedent
            applies. The local toggle behavior is byte-for-byte unchanged —
            the three setters keep persisting through
            `WarpDrivePrivacySettings` and emitting; the
            `is_logged_in()`-gated spawns, `fetch_or_update_settings` +
            `initialize_from_fetched_settings_or_update_settings` +
            `overwrite_local_settings_if_cloud_disabled` +
            `update_server_with_local_settings` (the fetch fired only from
            AuthComplete), `SyncedUserSettings`, `PrivacySettings`'s
            `auth_state`/`auth_client` fields, and the auth_manager
            fetch block (its snapshot binding stays — telemetry flush still
            reads it) went. `maybe_sync_with_warp_drive_prefs` survives via
            its other caller, `CloudPreferencesSyncer::sync`.

            *Onboarding push*: `set_user_is_onboarded` was reachable only as
            the server half of `AuthManager::set_user_onboarded`; the method
            is now local-only (`set_is_onboarded(true)` + persist), and its
            three callers (root_view sync-on-AuthComplete, the two
            workspace-view onboarding triggers) keep the local marking.

            *Agent identities + the GUI key manager*: `list_agent_identities`
            had one caller — the create-key modal's agent dropdown on
            `SettingsSection::OzCloudAPIKeys`, whose page is dropped from the
            sidebar by `needs_warp_account()` and remapped by `available()`
            at every entry point (deeplink, session restore). Deleted the
            page (`platform_page.rs`, `platform/` — modal, expire button,
            their tests) with its settings wiring: the view handle variant,
            registration, event handler, modal-content and should_render
            arms, the "Cloud platform" umbrella (single subpage), and the
            `platform` deeplink route (tests re-pointed at the
            billing_and_usage precedent: route no longer resolves). The
            `SettingsSection::OzCloudAPIKeys` variant itself stays, like
            Account, for SQLite session-restore mapping. The two
            `filterable_dropdown` test helpers (`set_filter_query_for_test`,
            `visible_items_len_for_test`) lost their last caller with the
            modal tests. GraphQL `get_user_settings`/`update_user_settings`/
            `set_user_is_onboarded` operations deleted;
            `auth/mod_tests.rs` (only tested `on_settings_updated`) deleted.

            Acceptance: `check -p warp --tests` clean in both feature sets
            (warnings = the pre-existing set), clippy diffed against a
            stash-captured HEAD baseline is byte-identical in both configs
            (the work-tree-only diff first surfaced the two orphaned test
            helpers, folded), format clean, nextest green (warp lib 5162
            default / 5161 simplewarp, warp_graphql+warp_server_client 28).
            Not re-run in the app — the toggles, the onboarding marking, and
            the settings sidebar all behave identically; every deleted path
            required a login SimpleWarp cannot have or a page it cannot show.
            **Remaining queue**: `AuthClient` is done; next is
            `ServerApi`/`Provider`/`BaseClient`, then the crates.
      - [x] **ServerApi/Provider/BaseClient survey** (4ca, 2026-09-16 — no
            code change except one latent compile fix, below): the picture for
            the last stretch of the client chain, so the rounds don't re-derive
            it.

            *Provider*: 4 getters — `get`, `get_auth_client`, `get_ai_client`,
            `get_http_client`. The event loop keeps two special arms
            (`UserAccountDisabled` → `app:log_out`, `NeedsReauth` → AuthManager)
            and re-emits the rest; `AuthEvent` is down to 4 variants, and the
            only external listeners of the re-emit are remote_server's bearer
            forwarder (`wire_auth_token_rotation`) and the builtin-MCP re-sync,
            both on `AccessTokenRefreshed`, which fires only after a real
            refresh. 86 non-test provider access sites, 30 of them `.get()`
            (the whole `ServerApi`); 81 files reference the symbol at all.

            *ServerApi inherent*: the only live payload is **telemetry** —
            `TelemetryApi` (Rudderstack) is 8,024 lines in `server/telemetry/`
            with 767 send-macro call sites. Everything else is walls with
            live-shaped callers: three SSE `stream_agent_events*` (ambient.rs,
            agent_events/driver.rs), `notify_login` (no-op log; one
            AuthComplete caller), `set_ambient_agent_task_id` (header
            decoration, 10 sites), and the ai.rs inherent helpers
            (`*_for_task`) feeding the ambient messaging loop.

            *Trait impls left on ServerApi*: `AIClient` — 29 walls in 4bm's
            groups minus the taken rounds: assistant surfaces 4 (palette
            search, dialogue answer, workflow metadata, the
            `provide_negative_feedback` refund — AIRequestUsageModel's last
            `ai_client` caller), cloud-run lifecycle 11, conversation sync 10,
            artifacts 3, code review 1. Plus **`StoreClient` (7 walls), which
            4n's table missed**: it backs the whole
            `full_source_code_embedding` vertical — 12,111 lines in crates/ai,
            the "Indexing and projects" settings page, init_project,
            driver/environment — and `CodebaseIndexManager` receives
            `ServerApiProvider…get()` as its `Arc<dyn StoreClient>`, so every
            store call fails and indexing can never progress.

            *AuthClient* (5): the refresh pair is live in shape — `fetch_user`
            only from AuthManager (5 sites, including `authenticate_api_key`
            via launch mode `ApiKey` and the CLI `CommandAuthentication::
            PendingApiKey` path), `get_or_refresh_access_token` from
            remote_server's auth_context (remote-daemon bearer), the workspace
            `CopyAccessTokenToClipboard` dev action, and tests. The api-key
            trio is `warp api-key` (`agent_sdk/api_key.rs`;
            `api_key_management` is in the simplewarp set).

            *BaseClient* (338 lines): live duties are the http client,
            AuthState identity (`user_id` 51 sites, `anonymous_id` 13, via the
            Deref), the refresh pair, and header decoration. Zero callers
            outside the crate: `access_token_ignoring_validity`,
            `allowed_to_refresh_token`, `event_sender`, `send_auth_event`,
            `is_auth_refresh_allowed`, `get_or_create_ambient_workload_token`,
            `ambient_headers`, `graphql_request_options*` — the last two feed
            only graphql_helpers and `fetch_user_properties`; the ambient
            workload-token machinery decorates requests that are all walls.

            *warp_server_client* remaining modules: `auth/` (trait + impl +
            session + events), `base_client`; `graphql_helpers`, whose
            `send_graphql_request` has exactly **three** callers left — the
            api-key trio (`fetch_user_properties` sends its operation
            directly); `network_logging` — live, backs the in-app network log
            view, needs a new home when the crate falls; `public_api` —
            **orphaned**: `get_public_api`/`get_public_api_response` have zero
            production callers (their own tests only; the app's
            `get_public_api_response_for_task` is unrelated), and the module
            survives only as the `HttpStatusError` re-export that app
            presigned_upload re-exports; `drive.rs`/`ids.rs` — one-line
            cloud_objects re-export shims (app folders, generic_string_model,
            server/ids).

            *The crates, in fall order*: `firebase` (145 lines, 2 importers —
            both warp_server_client/auth exchange-credential types) falls with
            the AuthClient refresh pair. `warp_server_client` needs the traits
            gone plus a network_logging decision. `warp_server_auth` (1,270
            lines, 12 importers) is local identity — AuthState holds
            user_id/anonymous_id for telemetry and local persistence — and
            outlives the chain; not on the critical path. `warp_graphql`
            (9,079 lines, 49 importers) has exactly **four** GraphQL
            operations still built workspace-wide — `GetUser`, `ApiKeys`,
            `GenerateApiKey`, `ExpireApiKey`, all in AuthClientImpl — so its
            client half falls with the api-key trio, but its *type* half
            (CloudObject, GenericStringObjectFormat,
            EmbeddingConfig/NodeHash/ContentHash, AgentHarness, LlmProvider,
            AIConversation types, Time, GuestSubject) is the data model of
            cloud_objects and full_source_code_embedding and falls with them.
            `cloud_objects` (2,346 lines, 105 importer files) is the last
            layer: local Drive object models + persistence after 4bg removed
            the sync.

            *Recommended round order, smallest genuine first*: (1) delete the
            orphaned `public_api` functions (quick win, rides along anywhere);
            (2) the artifacts AIClient group — sandwiched between its own
            walls (create/confirm targets are walls, so the presigned-S3
            middle never receives a URL): artifact_upload.rs,
            presigned_upload.rs, the download path, and retry_strategies'
            HttpStatusError classification; (3) the `warp api-key` round —
            answer the 3k question first (with every server surface a wall,
            an API key buys nothing), then take the CLI, the api-key trio, and
            the two `authenticate_api_key` paths together; this falls
            graphql_helpers and BaseClient's graphql_request_options
            machinery; (4) StoreClient + full_source_code_embedding; (5) the
            four assistant surfaces (each a UI fallback to collapse); (6)
            conversation sync + cloud-run lifecycle — the ambient terminal UI
            verticals, largest; (7) **telemetry — a scope decision**: it is
            the one live remote payload left in ServerApi, and deleting it
            empties ServerApi to identity + transport; (8) the fold —
            ServerApi/Provider/BaseClient collapse, warp_server_client and
            firebase fall, network_logging moves, warp_server_auth stays as
            local identity, and cloud_objects + the warp_graphql types are the
            endgame.

            *Latent compile break found and fixed here*: the
            `#[cfg(all(test, feature = "skip_login"))]`
            `new_for_test_with_bearer_token` passed six args to the
            five-param `new_with_parts` — `agent_source` was added to the
            signature and this cfg'd call site, which presubmit never
            compiles, kept both `None`s. Dropped one; any `fast_dev` test
            build would have failed to compile. Accepted with a
            `cargo check -p warp --tests --features skip_login`.
      - [x] **The orphaned `public_api` request functions are deleted** (4cb,
            2026-09-16, 2 files, −201): `get_public_api`/`get_public_api_response`
            had zero production callers — only their own tests (the app's
            `get_public_api_response_for_task` is a different, task-scoped
            helper). The module keeps `HttpStatusError`, which the app
            presigned_upload re-export and the retry classifiers still used.
            4ca survey item (1).
      - [x] **The artifacts upload/download group is deleted, walls and callers
            together** (4cc, 2026-09-16, 47 files, +92/−4,025, 10 files removed
            outright): AIClient 29 → 26. Every wall in the group sat inside its
            own dead pipeline — create/confirm targets were the only producers
            of the presigned-S3 middle's input, and `get_artifact_download` the
            only producer of the lightbox's URLs.

            *Deleted*: `presigned_upload.rs` (the presigned-POST middle:
            `UploadField`, `UploadBody`/`FileUploadBody`, request building) with
            its tests; `agent_sdk/artifact_upload.rs` (`FileArtifactUploader`
            and the association resolver) with its tests; the `warp artifact`
            CLI vertical — `warp_cli/src/artifact.rs`, `agent_sdk/artifact.rs`
            (get/download/upload dispatcher + output writers), `CliCommand::
            Artifact` with its dispatch/auth/telemetry arms, the reject block
            and `mut_subcommand` hide (the 4m landmine, grepped for the literal
            up front), eight parse/auth tests, `CliTelemetryEvent::
            Artifact{Upload,Get,Download}` with all four arms, the
            `artifact_command` cargo feature (it was in *both* the default and
            simplewarp sets — the subcommands were live-shaped but all three
            hit walls), and `FeatureFlag::ArtifactCommand` itself; the
            `UploadArtifact` executor (255 lines of tests with it); the
            download fetches behind the UI; and `recording_artifact_view_url`
            with its two tests.

            *Collapses rather than deletions* (the recording vertical stays for
            its own round): the `UploadArtifactExecutor`'s dispatch arms became
            the same `Sync` "not synced to the server yet" error the executor
            already returned on every local conversation — `should_autoexecute`
            now `false`, so the action needs explicit approval before erroring
            (a permission-conservative change: the executor's file-read
            permission check is gone with it); recording finalize keeps the
            `!should_upload` discard/cancel paths and the empty-actions error,
            and the upload path becomes an immediate error — the capture, smart
            cut, overlay burn-in, thumbnail generation, and all three
            `ActiveRecording` fields they read (`frame_rate`, `summary`,
            `description` on the struct — the `StartRecording` *variant* keeps
            its summary/description; the output renderer reads the summary for
            the block title) are gone, along with `FinalizeReason::
            termination_reason`, whose only caller built the deleted
            `RecordingStopped` payload. The screenshot lightbox and
            file-download buttons open in their failure states directly (the
            same failure UI the wall's `Err` arm produced), and
            `OpenRecordingArtifact` degrades to the same "Failed to open
            recording." toast.

            *One judgement call*: `api::ToolType::UploadFileArtifact` is no
            longer advertised to local agent sessions (`get_supported_tools`
            dropped the flag-gated push). The tool could never succeed — its
            execution was the deleted wall — so offering it to local runners
            was a trap; the wire enum and the `AIAgentActionType::
            UploadArtifact` variant stay for protocol compatibility.

            *HttpStatusError moved, not deleted*: driver.rs still synthesizes
            it from SSE invalid-status errors, and the retry classifiers plus
            five test files downcast to it, so the type now lives in
            `server/retry_strategies.rs` (its only remaining role), and
            warp_server_client's `public_api` module — orphaned since 4cb's
            trim — is gone entirely.

            Acceptance: `check -p warp --tests` clean in both feature sets at
            the baseline warning set; clippy diffed **byte-identical** against
            a stash-captured HEAD baseline in both configs; format clean;
            nextest green (warp lib 5,102 default / 5,101 simplewarp,
            `warp_cli`+`warp_features`+`warp_server_client`+`warp_graphql` 142).
            Not re-run in the app — every deleted path required a
            warp-server artifact; every kept surface now shows the same failure
            state it already showed.
      - [x] **The `warp api-key` round is deleted, 3k question answered, walls
            and callers together** (4cd, 2026-09-16, 33 files, +28/−1,961, 9
            files removed outright): AuthClient 5 → 2 methods, and the two
            fallen mechanisms were exactly 4ca's predicted ones.

            *The 3k answer*: an API key bought nothing. The credential wrap
            (`exchange_credentials(LoginToken::ApiKey)`) is local, but the only
            thing that turns a key into a session is `fetch_user_properties` —
            a `GetUser` GraphQL call to warp-server, the server this fork does
            not run. Success would buy the logged-in gating of surfaces that
            are all walls; `get_or_refresh_access_token` would hand back
            `AuthToken::ApiKey` to decorate those same walls; `warp api-key`
            list/create/expire were three more warp-server operations; and
            nothing durable — `persist_action()` is `DoNothing` for API-key
            credentials. The only world where the path completes is pointing
            `SERVER_ROOT_URL` at a real Warp deployment, the out-of-scope call
            3k already made for the upstream channel binaries.

            *Deleted*: the `warp api-key` CLI vertical — `warp_cli/src/api_key.rs`
            (+tests), `agent_sdk/api_key.rs` (+tests), `CliCommand::ApiKey` with
            its dispatch/auth/telemetry arms, the `ApiKey{List,Create,Expire}`
            telemetry variants with all four arms, and the
            `APIKeyManagement`-gated reject/hide blocks; `FeatureFlag::
            APIKeyManagement` itself with the `api_key_management` cargo
            feature (in both the default and simplewarp sets); the global
            `--api-key`/`WARP_API_KEY` flag on `GlobalOptions` with its getter,
            `LaunchMode::App { api_key }`, `LaunchMode::api_key()`, and
            `AuthInitialization::PendingApiKey` (startup collapses to
            refresh-if-logged-in); both `authenticate_api_key` paths —
            `StartupUserAuthentication::ApiKey` (deleted: its one variant
            inlined) and `CommandAuthentication::PendingApiKey` (the enum and
            its `command_authentication` helper deleted, the logged-out check
            inlined into `launch_command`); `AuthManager::authenticate_api_key`;
            and the team-API-key warning (`maybe_warn_team_api_key`), whose
            `api_key_owner_type()` could only be `Some` through the deleted
            authentication — a no-op the moment the paths went.

            *The falling mechanisms*: `graphql_helpers` (`send_graphql_request`
            had exactly the trio left as callers) with its six tests;
            `BaseClient::graphql_request_options` and with it
            `AuthenticatedGraphqlConfig` — always constructed `default()` in
            production, its reserved-header filtering and the two tests
            exercising it — plus `is_reserved_authenticated_graphql_header`;
            the three GraphQL operations `ApiKeys`/`GenerateApiKey`/
            `ExpireApiKey` (only the trio built them) and
            `get_user_facing_error_message` (only the trio and the deleted CLI
            formatted its errors); the orphaned `ApiKeyUid` alias; and, one hop
            out in warp_server_auth, `AuthState::initialize_for_credential_
            validation`, whose only caller was the deleted startup path.
            `Credentials::ApiKey`/`AuthToken::ApiKey`/`exchange_credentials`'s
            ApiKey arm stay — `agent_mode_evals` constructs those credentials
            in `BaseClient::new`, and warp_server_auth's local-identity
            plumbing is explicitly out of this round's scope. The `fetch_user`
            refresh pair and `graphql_request_options_with_token` (now
            `fetch_user_properties`'s only helper) stay.

            *Tests re-pointed, 4bn precedent*: the two
            `command_authentication` mapping tests and the api-key
            failure-promotion test went with their subjects;
            `multiple_global_flags_before_subcommand_parse` now pins
            `--output-format` + `--debug` instead of `--api-key` + `--debug`;
            `api_key_before_subcommand_parses` was deleted outright — its
            regression (global flag before subcommand) is already covered by
            `debug_before_subcommand_parses`. `validated_api_key_is_promoted`
            and `test_persist_skips_when_api_key_authenticated` stay — they pin
            kept AuthState/Credentials behavior.

            Acceptance: `check -p warp --tests` clean in both feature sets,
            simplewarp and warp-oss bins included; clippy diffed against a
            stash-captured HEAD baseline is byte-identical in both configs,
            and the small-crate run is *cleaner* than baseline (HEAD carried a
            pre-existing unused-import warning in warp_cli that this round's
            deletion removed); format clean; nextest green (warp lib 5,087
            default / 5,086 simplewarp — 11 deleted tests, exactly the
            accounting; one unrelated notebooks flake passed on rerun;
            `warp_cli`+`warp_features`+`warp_server_client`+`warp_graphql`
            126). Not re-run in the app — every deleted path required
            warp-server to answer; the kept surfaces (login gating, refresh
            error paths) behave identically.
      - [x] **StoreClient + full_source_code_embedding is deleted, walls
            and callers together** (4ce, 2026-09-16, 83 files, +130/−15,894,
            40 files removed outright): the whole 4ca item (4). Every store
            call was already a guaranteed `local_only_error`, so indexing
            could never progress; the settings page, the init-project step,
            and the driver environment wait were UI around a dead core.

            *Deleted*: `crates/ai/src/index/full_source_code_embedding`
            (manager, codebase_index, chunker, merkle_tree, snapshot,
            sync/store clients, search shaping, ~12k lines with tests); the
            seven `StoreClient` walls on `ServerApi`; the seven GraphQL
            operations (generate/populate/update/sync/rerank/fragments/
            config); the "Indexing and projects" settings page (2,015 lines)
            with its nav section, `SettingsViewEvent::OpenLspLogs` /
            `OpenProjectRulesPane` pair, and `IS_AUTOINDEXING_ENABLED` flag;
            `CodeSettings::auto_indexing_enabled`;
            `FeatureFlag::FullSourceCodeEmbedding` /
            `CodebaseIndexPersistence` / `CodebaseIndexSpeedbump` with the
            `full_source_code_embedding`, `codebase_index_persistence`, and
            `codebase_index_speedbump` cargo features; the
            `CodebaseIndexManager` singleton + `SyncQueue<SyncTask>` + daemon
            restore log in `lib.rs`; the init-project `CodebaseContext` step
            (`CodebaseIndexingResult`, mouse handles, onboarding permission
            copy); the driver `prepare_environment` indexing wait
            (`subscribe_to_codebase_index_events`, `index_repo_codebase`,
            `record_codebase_indexing`, the `Harness::Oz` gate and `harness`
            arg); the `ToggleAutoIndexing` / `FullEmbed*` /
            `AgentModeSetupCodebaseContextAction` telemetry events; the
            `codebase_context` integration-testing step; and the orphaned
            `all_lsp_servers` / `total_lsp_server_count` helpers (only the
            deleted page called them).

            *Collapses*: `PersistedWorkspace` project-rules subscription is
            now unconditional (the flag gate only ever hid it in tests);
            `UserWorkspaces` CodeSettings subscription drops the
            `AutoIndexingEnabled` arm (single-arm `match` → `if let` per
            clippy); init-project step indices renumber 5 → 4.

            *Kept deliberately*: `SettingsSection::CodeIndexing` + slug /
            `from_slug("Code")` compat — old sessions restoring that page hit
            the existing `settings_page().is_none()` guard and stay on the
            default page. `codebase_context_enabled` setting,
            `IS_CODEBASE_INDEXING_ENABLED`, and the index-metadata
            persistence stay: they now front outline-based context, not the
            embedding pipeline.

            *Acceptance repairs (both suite-caught)*: (1) the unconditional
            subscription exposed a test-init ordering bug —
            `PersistedWorkspace::new` before `ProjectContextModel` in
            `pane_group/mod_tests.rs` and `workspace/view_tests.rs`
            (production `lib.rs` already had the right order); swapped both.
            (2) `arrow_down_across_adjacent_collapsed_umbrellas` still pinned
            the deleted first subpage — re-pinned to `EditorAndCodeReview`
            like the four sibling tests this round already updated.

            Acceptance: `check -p warp --tests` clean in both feature sets;
            clippy matches the stash-captured HEAD baseline modulo one
            deletion-shifted line number; format clean; nextest green (warp
            lib 5,087, small crates 126).
      - [x] **The four assistant surfaces are deleted, walls and callers
            together** (4cf, 2026-09-17, 38 files, +124/−2,381, 5 files
            removed outright): 4ca item (5). The four `AIClient` walls —
            `generate_commands_from_natural_language`,
            `generate_dialogue_answer`, `generate_metadata_for_command`,
            `provide_negative_feedback_response_for_ai_conversation` — and
            each surface collapsed to the failure state its error arm already
            produced. `AIRequestUsageModel` becomes a client-less singleton
            (`new()`, `type Event = ()`).

            *Palette search*: `WarpAIDataSource`'s `AsyncDataSource` half is
            gone (it could only return the wall's error), with its
            `DataSourceRunError` impl, the
            `GenerateCommandsFromNaturalLanguageError::RateLimited` downcast,
            `render_error_header` + `render_error_header_with_upgrade_link`
            (the team/billing/upgrade-link branching existed only for that
            downcast), the `CommandSearchAction::{OpenUpgradeLink,
            AttemptLoginGatedUpgrade}` variants with their dispatch arms, and
            the sync source's never-read `ai_client`/`ai_execution_context`
            plumbing — `CommandSearchView::new(ctx)` now, and
            `reset_state`/`reset_command_search_mixer` lose the
            `ai_execution_context` threading (computed in workspace view only
            to feed the deleted source). The sync Translate/Open items and
            their `AISettings` gate stay.

            *Dialogue answer*: `Requests::issue_request` records the trimmed
            question and appends the guaranteed "We're experiencing technical
            difficulties…" answer synchronously — no spawn, no server. With
            nothing in flight, `RequestStatus` falls whole (send/editor
            gating became unconditional; the abort machinery, `team_uid`,
            `GenerateDialogueResult`, the out-of-credits team/billing arm,
            and `request_limit_info` + its getters go), as do the fake
            "Credits used: 0 / 150." footers (panel zero-state and transcript
            details, with `render_request_limit_info` and its alert consts in
            `utils.rs`), `current_transcript_summarized` + the
            missing-context notice it gated, the minute `tick` (existed to
            move the refresh-time display), and the `ActiveSession`
            execution-context feed. `AIAssistantPanelView::new(ctx)` — no
            `server_api`. `new_with_transcript` is `#[cfg(test)]` now (the
            three transcript tests keep it).

            *Workflow metadata*: the "Autofill" button vertical is deleted
            from BOTH `WorkflowModal` (drive) and `WorkflowView` —
            `AiAssistState`, `ai_metadata_assist_state`, the
            `RequestInFlight` arms of the new-argument/save disabled checks,
            `is_ai_assist_button_disabled`, the button render + tooltip +
            mouse handles, `issue_request`,
            `populate_missing_field_with_suggestion` (success-path-only
            helpers), `WorkflowModalAction::AiAssist`,
            `WorkflowModalEvent::AiAssistError` with the workspace toast
            handler, the `AI_ASSIST_*` consts, and `drive/workflows/
            ai_assist.rs` whole (`GeneratedCommandMetadata`,
            `GeneratedArgument`, the error enum). Both constructors lose
            `ai_client`. `AutoGenerateMetadataSuccess/Error` telemetry gone
            (all six arms).

            *Refund*: the `provide_negative_feedback_response_for_ai_
            conversation` request path,
            `AIRequestUsageModelEvent::RequestBonusRefunded`, the block.rs
            subscriber, and the "We've refunded you N credits" footer
            (`request_refunded_count` through block/view_impl/output) are
            gone; the thumbs-down handler keeps the rating, the thank-you
            toast, and `AgentModeRatedResponse` telemetry.

            *Falling with them*: the four GraphQL operations
            (`generate_commands`, `generate_dialogue`,
            `generate_metadata_for_command`, `request_bonus` — the latter
            two already orphaned by the wall deletion) and
            `warp_graphql::ai::{RequestLimitInfo,
            RequestLimitRefreshDuration}` with the two `From` impls in
            `ai_assistant/mod.rs`, which also loses `AIGeneratedCommand`/
            `AIGeneratedCommandParameter`/`GenerateCommandsFromNaturalLangu
            ageError`. One hop out:
            `UserWorkspaces::{team_from_uid_across_all_workspaces,
            is_custom_llm_enabled_for_team}` (callers were the out-of-credits
            arm and the two credit footers) with its team-precedence test,
            and the panel's never-read `view_handle`.
            `AIRequestUsageModel::new_for_test(ai_client)` sites → `new()`
            (9 files). `WorkflowType::AIGenerated` +
            `AIWorkflowOrigin::{AgentMode, LegacyWarpAI}` stay — Agent Mode
            still produces them.

            *Baseline note*: today's HEAD captures emit four dead-code lints
            in `server_api/ai.rs` that this round's diff cannot have caused —
            `TaskListFilter`'s never-read fields and
            `ArtifactType`/`RunSortBy`/`RunSortOrder`::`as_query_param`
            (zero callers at HEAD by grep; the caller went with an earlier
            round's deletion, 4ce's diff likely truncated them). They are
            4bm cloud-run-group surfaces (`list_agent_runs` request shaping),
            so they fall with the next rounds rather than here.

            Acceptance: `check -p warp --tests` clean in both feature sets;
            both bins; clippy diffed against a stash-captured HEAD baseline
            in both configs (workspace −D and `--all-targets --tests -D`) is
            clean apart from the four noted HEAD-carried lints — zero new;
            format clean; nextest green (warp lib 5,085 default / 5,084
            simplewarp — the 2/3 fewer tests are the deleted
            `test_populating_missing_fields_with_suggestion`,
            `test_member_team_settings_win_over_workspace_settings`, and a
            feature-gated test; small crates 126). Built and launched
            `./target/debug/simplewarp` — alive, no output, no connections.
      - [x] **Conversation sync is deleted, walls and callers together** (4cg,
            2026-09-17, 42 files, +241/−4,261, 3 files removed outright): 4ca
            item (6), all ten conversation-sync walls on `AIClient` —
            `fork_conversation`, `rename_conversation`, `get_ai_conversation`,
            `list_ai_conversation_metadata`, the Orchestrations-V2
            `send/list/read/mark` message quartet, `get_public_conversation`,
            `get_run_conversation` — plus the four task-scoped inherent
            helpers (`*_for_task`), which were themselves walls over
            `get/post_public_api_response_for_task`. `AIClient` 26 → 16.

            *The `warp run` CLI vertical*: `TaskCommand::Message`
            (send/list/watch/read/mark-delivered), `TaskCommand::Conversation
            get`, and `TaskGetArgs --conversation` deleted from `warp_cli`
            with their args structs, tracing strings, telemetry variants
            (`ConversationGet`, `RunConversationGet`,
            `RunMessage*`), auth arms, and nine parse tests; the
            `agent run --conversation` flag and its
            `CloudConversations` accept/hide checks went too, so
            `FeatureFlag::ConversationApi` and `FeatureFlag::CloudConversations`
            are deleted with the `conversation_api` (both sets) and
            `cloud_conversations` (default) cargo features. `OZ_RUN_ID_ENV`
            task-scoping helpers (`task_id_from_oz_run_id_env`,
            `task_id_for_message_send`) died with the commands that read them.
            *The fork flow*: the server-side fork branch in
            `Workspace::fork_ai_conversation` is gone — it fired on every plain
            fork (local StreamInit tokens exist), logged a guaranteed
            "Server-side fork failed" warning, and fell back to the local-only
            fork that is now the only path; `create_local_fork` loses
            `server_forked_conversation_id`. *Rename becomes local-only*: the
            `/rename` slash command and conversation-list rename stay; the
            begin/complete/fail in-flight machinery
            (`InFlightConversationRename`,
            `BeginConversationRenameError`, `restore_conversation_title`) is
            replaced by `rename_conversation_locally` (validate → apply →
            success toast) — the old flow optimistically applied the title and
            then always reverted it with "Failed to rename conversation:"
            because local StreamInit tokens made `begin_conversation_rename`
            pass its server-token gate into a guaranteed wall error. The 3n
            local-first precedent: the local half worked, only the remote half
            was dead. *Resume*: `fetch_and_validate_conversation_harness`
            deleted with the flag; `load_conversation_information`'s Oz arm
            collapses to its `ConversationLoadFailed` error (the `--task-id`
            path that still calls it fails earlier at the task fetch — that
            path is 4ch's). `AgentDriverError::ConversationHarnessMismatch`
            and the harness-mismatch classification arm went with the
            validator; `convert_conversation_data_to_ai_conversation` +
            `RestorationMode` lost their last caller and are deleted
            (three `set_server_metadata` semantics tests re-pointed at a local
            `new_restored` factory). `ResumeOptions::Oz` is deliberately kept
            (driver machinery still consumes it; its last producer is 4ch's).

            *Orchestration messaging*: `MessageHydrator` is deleted whole —
            its only duties were the read/mark walls. The
            `SendMessageToAgentExecutor` resolves synchronously to the same
            `SendMessageToAgentResult::Error` its server-failure arm produced
            (no spawn, no 15s timeout, no request types), keeping the
            `TeamAgentCommunicationFailed` telemetry; the
            `SseForwardingConsumer` forwards events unhydrated;
            `on_streaming_exchange_updated` and the `pending_message_ids`
            state machine are gone; `prime_parent_bridge_staged_for_self_managed_wake`
            stages the wake record without hydration. The SSE streamer,
            wake listener, and parent-bridge plumbing stay for 4ch. The
            agent-management page's cloud-metadata fetch half collapses
            (`list_ai_conversation_metadata` join, missing-id refetch, and the
            always-false `cloud_metadata_loaded` gone — the fetch lands in
            `CloudFailed` exactly as before); `merge_cloud_conversation_metadata`
            stays for the entry-projection tests and 4ch's page removal.
            *Falling with them*: the seven wire types
            (`ForkConversationResponse` … `ReadAgentMessageResponse`),
            `write_json`, `resolve_orchestration_harness_label`, and
            `parse_ambient_task_id`; `CliCommand::Run(Box<TaskCommand>)` +
            `TaskCommand::List(Box<ListTasksArgs>)` (clippy
            large_enum_variant surfaced by the deletions; the lint is
            allowed on `CliCommand` for `Agent`'s 456-byte run args).

            Acceptance: `check -p warp --tests` clean in both feature sets,
            both bins; clippy diffed against pre-round baselines in both
            configs (workspace `-D` and `-p warp --all-targets -D`) shows zero
            new lints — only the four HEAD-carried `as_query_param` lints
            shifted line numbers inside `server_api/ai.rs`; format clean;
            nextest green (warp lib 5,045 default / 5,044 simplewarp — 40
            deleted tests; small crates 117 — 9 deleted parse tests). Built
            and launched `./target/debug/simplewarp` — alive, no output.
      - [x] **Cloud-run lifecycle (4ch) — DONE 2026-09-18.** Scope taken: the
            11 cloud-run AIClient walls plus the whole ambient terminal UI
            vertical. `AIClient` is GONE as a trait: after the earlier rounds
            deleted their callers, the last four zero-caller walls
            (`spawn_agent`,
            `list_ambient_agent_tasks`, `get_ambient_agent_task`,
            `submit_run_followup`) were deleted together with the entire
            `server_api/ai.rs` module — wire types (`SpawnAgentRequest`,
            `RunFollowupRequest`, `AgentRunEvent`, `SpawnAgentResponse`,
            `TaskListFilter`), the `ai_tests.rs` spawn/followup/agent-run
            tests, `ServerApiProvider::get_ai_client`, and the never-
            constructed `ClientError`/`CloudAgentCapacityError`. The 15
            Artifact serde tests moved from `ai_tests.rs` into
            `artifacts/mod_tests.rs` (Artifact stays; one duplicate and the
            AgentRunEvent test dropped).

            *Deleted this round (previous session's uncommitted work,
            verified and finished by this session)*: `terminal/view/
            ambient_agent/` whole (the cloud-run terminal UI), `agent_events/`,
            `agent_sdk/ambient.rs`, driver `git_credentials`, claude_code
            `parent_bridge`/`wake_driver`, `text_layout`, ambient_agents
            `spawn`/`scheduled`/`telemetry`/`github_auth_*`,
            `orchestration_event_streamer`/`orchestration_events`/
            `orchestration_child_tracker`, `local_agent_task_sync_model`,
            `generate_code_review_content`, cloud_agent_capacity_modal,
            workspace `auto_handoff`, pane_group child_agent
            hydration/materialization, the `warp run|agent` CLI task
            machinery (`task.rs`, `json_filter.rs`, `date_time.rs`,
            `sort_order.rs`), and the four `/continue-locally` + v2
            cloud-mode input surfaces (`GuiSlashCommandDataSource::
            for_cloud_mode_v2`, `is_cloud_mode_v2`). The purely local
            scheduled-ambient-agent *model* was preserved by moving it to
            `cloud_object/model/scheduled_ambient_agent_model.rs`.
            `cancel_task_with_toast`/`cancel_task_silently` collapsed to
            warn+toast no-ops (same failure the wall produced).

            *Conversation details panel*: the task-mode half deleted
            (`PanelMode::Task`, `from_task`, fetch-error notice +
            `TaskFetchError`, environment/platform rows, `OpenInOz` button +
            `oz_run_url`, Copy RunId/EnvironmentId/DockerImage/FetchError/
            Error actions, its 8 from_task tests). **The local half was
            deliberately kept and its data feed RESTORED**: the deleted
            ambient `view_impl.rs` owned `fetch_and_update_conversation_
            details_panel`, including the local-conversation fallback
            (APP-3595); it now lives in `terminal/view.rs` feeding from the
            active local `AIConversation` via `from_conversation`, wired to
            `ToggleConversationDetailsPanel`, the history-event filter, and
            `FinishedReceivingOutput`. `ConversationDetailsPanel::new`
            callers and `for_conversation`/`set_config` are live again.

            *Also collapsed*: `load_conversation_from_server` and the whole
            doomed transcript-viewer server-load flow
            (`load_cloud_conversation_into_new_transcript_viewer` is now a
            direct failure toast; pane_group's
            `load_data_into_conversation_transcript_viewer` +
            `load_data_into_transcript_viewer` +
            `ambient_agent_task_id(&CloudConversationData)` deleted;
            `load_conversation_data`/`load_conversation_by_server_token`
            lost their server/ctx params), `mcp_config_tests` re-pointed to
            serialize `AgentConfigSnapshot` directly, `zero_state.rs`
            rewritten to the non-v2 behavior, status_bar `render` rebuilt
            without the cloud-mode-setup and ambient branches,
            `working_directory_chip`'s outer `Stack` restored, the
            `is_ambient_agent` param dropped from
            `MockTerminalManager::create_model` and all call sites,
            `RichContentType::AmbientAgentBlock` deleted, the AI-context
            menu's cloud-task source removed, `ProfileModelSelector::new`
            call sites updated to the 4-arg signature, and `/continue-locally`
            tests re-pinned (`input_tests` uses `/compact` as the negative
            control; the registration test deleted).

            *Dead-code cascade fixed so far*: `details_action_buttons::
            for_task`, three `AgentManagementTelemetryEvent` variants
            (CloudRunOpened, SessionLinkCopied, SlashCommandContinueLocally)
            with all four arms, agent_tips `link`/`kind`/`AgentTipKind`/trait
            `link()`/generic `AITipModel::new`, ten never-used
            `AmbientAgentTask`/`AmbientAgentTaskState` methods (run_id,
            conversation_id, active_execution_*, credits_used,
            creator_display_name, is_working, is_failure_like, is_terminal,
            status_icon_and_color), llms `cloud_runnable_oz_model_id_or_
            fallback` + `CLOUD_FALLBACK_OZ_MODEL_ID`, the four
            `TaskFetchError`-era imports in agent_conversations_model, and
            assorted unused imports/variables.

            *Dead-code cascade finished*: every warning the round left behind
            is gone. `uri/mod.rs` (`CLOUD_SETUP_SOURCE` and the four
            cloud-mode terminal finders),
            `slash_commands/data_source/core.rs`
            (`InlineItem::from_saved_prompt`, `with_compact_layout` and the
            now-always-false `compact_layout` field with its two
            `search_item.rs` render branches), `pending_user_query.rs`
            (`insert_cloud_mode_queued_user_query_block`,
            `remove_cloud_mode_queue_row`, `PendingUserQueryKind::CloudMode`,
            and `QueuedQueryModel::remove_initial_cloud_mode_row`),
            `harness_availability.rs` (`has_any_enabled_harness`,
            `is_harness_enabled`), `user_workspaces.rs`
            (`get_cloud_conversation_storage_enablement_setting`),
            `cloud_agent_settings.rs` (`persist_harness_model_selection` with
            the `last_selected_harness_model` setting and
            `HarnessModelSelection`), `resolve_skill_spec.rs`
            (`ResolvedSkill::parsed_skill`), `view_tests.rs`
            (`has_pending_user_query_block`,
            `update_exchange_input_and_handle_event`, `TestTerminalManager`),
            `agent_view.rs::enter_agent_view_for_restored_cli_agent`, and the
            details panel's whole setup-commands vertical
            (`PLATFORM_ICON_SIZE`, `copy_setup_commands`,
            `CopyButtonKind::SetupCommands`, `CopySetupCommands`,
            `format_setup_commands_for_copy`,
            `render_setup_commands_section`). `llms.rs` fixed a real break:
            `CUSTOM_ENDPOINT_USAGE_FALLBACK_LABEL` is still read by
            `custom_endpoint_usage_display_label` for local agent footers, so
            it came back; only the cloud-run-only
            `is_cloud_runnable_oz_model_id` stayed deleted. The orphan file
            `terminal/input/cloud_mode_v2_history_menu.rs` (mod declaration
            already gone) is deleted, and with it `InlineMenuView`'s
            cloud-only `compact_layout` and `dismiss_on_row_click` builders,
            fields and branches.

            *Second tier — `clippy --all-targets` compiles the non-test lib,
            so it exposed ~30 more items whose only remaining callers were
            their own unit tests.* Deleted outright: `ambient_agents/task.rs`
            down to its live half (cloud `AmbientAgentTask`/`TaskState`/
            `RunExecution`/`LiveSessionState`/`RequestUsage`/
            `TaskPrincipalInfo`/`TaskStatusMessage`/`TaskStatusErrorCode`/
            `ExecutionLocation` + the source parser and session-id parsers,
            640 test+model lines; `AgentSource`, `AttachmentInput`,
            `normalize_orchestrator_agent_name`, the snapshot re-exports and
            the two `cancel_task_*` collapses stay), the Claude/Codex
            transcript upload modules (`codex_transcript.rs` whole,
            `claude_transcript.rs` down to `claude_config_dir` +
            `home_dir_for_claude_config`, `json_utils::entries_to_jsonl`),
            and the entire HTTP retry layer — `server/retry_strategies.rs`
            and `ai/agent_sdk/retry.rs` with their 13 tests, since
            `with_bounded_retry` and `HttpStatusError` lost every production
            caller. Also: `auth_check_command_for`, `deserialize_artifacts`,
            `exit_agent_view_without_confirmation`, `active_skill_by_reference`,
            `get_last_focused_terminal_id`, `from_conversation_metadata`, and
            the two items the `#[allow(dead_code)]` on
            `agent_conversations_model::entry` had been hiding
            (`SESSION_EXPIRATION_TIME`, `parse_session_id`) — that allow is
            now gone. Tests were re-pointed, not dropped, where the behavior
            is still local: `exit_agent_view`,
            `active_skill_by_reference_with_origin`, and the zero-state hint
            test's inactive control, which is now `/compact` with
            `FeatureFlag::AgentView` on (the old control was the cloud-only
            `/continue-locally`; with the cloud bits unsatisfiable every
            remaining hint command is active in a plain pane).
            `AgentRunDisplayStatus` lost its redundant `Conversation`
            prefix once the cloud-run variants fell.

            *Test-side repairs (all three were the WIP's own re-pins, not
            product regressions)*: the zero-state hint test had swapped its
            "inactive command" control from the deleted cloud-only
            `/continue-locally` to `/compact`, which fails twice over —
            `/compact` only enters `COMMAND_REGISTRY` under
            `FeatureFlag::SummarizationConversationCommand`, so its stale
            placeholder is never touched, and with the cloud availability bits
            unsatisfiable no hint-bearing command stays inactive in a plain
            pane. It now asserts the clear-and-re-register invariant on
            always-active `/rename-tab` (renamed
            `zero_state_hint_text_refreshes_active_slash_command_placeholders`).
            The two `enqueue_followup_prompt_*` tests re-pointed at
            `add_window_with_local_conversation` kept asserting through
            `queue_texts`, which reads the *selected* conversation — empty in
            that harness (the sibling test that passes asserts it is); they now
            read `QueuedQueryModel::queue(conversation_id)` directly. And with
            `exit_agent_view_without_confirmation` gone (its only production
            caller was the deleted ambient `view_impl.rs`), the transcript
            navigation test exits through the real path — two Escape presses,
            since the conversation is still in progress.
            `crates/warp_cli/src/lib_tests.rs` still referenced the removed
            `--task-id` / `--skip-initial-turn` args and did not compile: eight
            task-id tests deleted, three stray assertions dropped,
            `agent_run_rejects_without_prompt_or_task_id` renamed to
            `agent_run_requires_a_prompt`.

            Acceptance: `check -p warp --tests` clean in both feature sets —
            0 errors, 0 warnings; `--bin simplewarp` (simplewarp set) and
            `--bin warp-oss` (default set) surface only HEAD's three test-only
            `never used` items (`snapshots::Loaded`,
            `should_preserve_onboarding_profile`, `reset_unknown`), which any
            non-test build reports; `clippy -p warp --all-targets --tests` in
            both configs is a strict
            subset of the pre-round HEAD baseline captured by resetting to
            `aae51e938` (the five survivors are HEAD's: `input.rs`
            needless-returns — 0 added by this round, 17 removed —
            `should_preserve_onboarding_profile`, snapshots `Loaded`,
            `reset_unknown`, and the `mod_tests` single-element loop), and it
            drops HEAD's `notebook_tests` pair and the four `server_api/ai.rs`
            lints with their code. Format clean. Nextest: warp lib 4,658 passed
            default / 4,657 simplewarp, 0 failed (4 skipped both), down from
            4cg's 5,045/5,044 with the deleted surfaces' tests; `warp_cli` 76
            passed (was 84 before the task-id tests went). Built and launched
            `./target/debug/simplewarp`.

            *Cloud-run residue the compiler cannot see (next slice)*: `/cloud-agent`
            is still *active* in a local build while its spawn path is gone,
            and `/host` + `/harness` are permanently inactive because
            `Availability::CLOUD_MODE_V2_COMPOSER` can no longer be set —
            `CLOUD_AGENT` and `CLOUD_MODE_V2_COMPOSER` are both unsatisfiable
            bits now. `QueuedQueryOrigin::InitialCloudMode` has no producer but
            still drives five `queued_prompts_panel.rs` branches plus its
            telemetry mapping, and the ambient task-id plumbing is ~249
            references across `workspace/view.rs`, `lib.rs` (`OZ_RUN_ID_ENV` →
            `ServerApiProvider::set_ambient_agent_task_id`), `pane_group`, the
            terminal model and `load_ai_conversation`. After 4ch: this residue,
            then the telemetry scope decision (4ca item 7), then the fold
            (item 8).
      - [x] **Cloud-run residue, first slice (4ci) — DONE 2026-09-19.**
            The compiler-invisible half of 4ch's residue that deletes clean:
            the three dead slash commands plus the dead queue origin.
            11 files, +31/−269.

            *Slash commands*: `/cloud-agent` (active but misleading — its
            spawn path went with 4ch's `spawn_agent` wall, so it just opened
            the same local agent view as `/agent`), `/host` + `/harness`
            (permanently inactive — they require
            `Availability::CLOUD_MODE_V2_COMPOSER`, which has no producer
            since 4ch deleted `for_cloud_mode_v2`/`is_cloud_mode_v2`).
            Deleted the three `StaticCommand`s + registry pushes, the three
            `SlashCommandKind` variants, the `/cloud-agent` keybinding
            (`cmd-alt-enter`/`ctrl-alt-enter`), the shared
            `Agent|New|CloudAgent` handler arm's `CloudAgent` disjunct, the
            `Host|Harness => false` no-op arm, the `/host`-only
            `has_default_host` gate in `data_source/core.rs` (the underlying
            `default_host_slug`/`WARP_CLOUD_MODE_DEFAULT_HOST` orchestration
            machinery stays — still used by snapshots and cards), and the
            now-producer-less `Availability::CLOUD_AGENT` +
            `CLOUD_MODE_V2_COMPOSER` bits. `NOT_CLOUD_AGENT` stays (14 live
            commands gate on it; `gui.rs` still sets it). Tests: the 4
            availability-bit tests in `static_commands/mod_tests.rs` deleted,
            `not_cloud_agent_...` trimmed of its V2 lines,
            `cloud_mode_v2_commands_...` deleted (its subject was `HARNESS`).

            *Queue origin*: `QueuedQueryOrigin::InitialCloudMode` had zero
            producers (only a test fixture constructed it) but still drove
            the copy-instead-of-delete render swap, the force-disabled edit
            button, the disabled-grey drag handle with tooltip, the
            non-draggable guard, and the whole send-now cloud-setup disable
            path in `queued_prompts_panel.rs`, plus its telemetry mirror.
            Deleted the variant, collapsed `is_locked()` to
            `PendingLrcAutoQueue` alone, removed `CopyRow` action/handler/
            button (delete always shows now), the tooltip constants, the
            `drag_handle_tooltip_state` field, and the `origin` params that
            only existed for these branches (`build_row_state`,
            `seed_row_states_for`, the `Appended` handler). `ParentElement`
            (the trait behind `Flex::with_child`/`add_child`) stays — removing
            it broke the build and was restored. Tests: ICM lock test
            re-fixtured to `PendingLrcAutoQueue` (renamed
            `pending_lrc_head_rejects_user_mutations_and_autofire`),
            `pop_front_no_ops_when_head_is_locked` re-fixtured the same way.

            Deliberately left: `TerminalAction::EnterCloudAgentView` + the
            zero-state dispatches to it, `AgentViewEntryOrigin::CloudAgent`,
            and the whole ~249-ref ambient task-id plumbing (`OZ_RUN_ID_ENV`
            → `set_ambient_agent_task_id`, viewer status, transcript-viewer
            threading) — surveyed this round, all live-but-dormant header
            plumbing with no cloud producer left, but a multi-round job on its
            own, not a leaf deletion.

            Acceptance: `check` clean both feature sets (`--all-targets`,
            `-p integration`); clippy simplewarp 0 errors, no warnings in
            touched files (12 pre-existing elsewhere); format clean.
            Nextest: warp lib 4,652 passed simplewarp / 0 failed (4 skipped),
            down exactly 5 from 4ch's 4,657 (4 availability + 1 V2 test);
            default-feature slash/queue filter 55 passed; `warp_cli` 76
            passed. `--bin simplewarp` check clean.
      - [x] **Dead cloud-entry TerminalActions (4cj) — DONE 2026-09-19.**
            `EnterCloudAgentView` (handler was a `=> {}` no-op; its
            cmd-alt-enter/ctrl-alt-enter fixed binding and both zero-state
            "start a new cloud agent conversation" buttons dispatched into
            it) and `CancelAmbientAgentTask` (zero producers, same no-op
            handler). 5 files, +1/−68: the two `TerminalAction` variants +
            `Debug` arms, the empty-action listing + handler arm, the
            `init.rs` fixed-binding block, and both zero-state cloud rows
            with their `start_cloud_conversation` mouse handles + unused
            keystroke imports. Local agent entry
            (`StartNewAgentConversation` with `Input` origin, separate row
            above the deleted one) is untouched. Deliberately left:
            `ENTER_CLOUD_AGENT_VIEW_NEW_CONVERSATION_KEYSTROKE` (still used
            by the shortcuts display `is_cloud_agent` branch),
            `AgentViewEntryOrigin::CloudAgent` (zero producers but
            exhaustive matches across controller/telemetry/agent_view),
            `DefaultSessionMode::CloudAgent` (persisted settings value),
            the onboarding `submit_to_cloud_agent` hint text (display-only,
            no dispatch), and the whole ambient task-id plumbing —
            `AmbientAgentTaskId::new()` backs local-only orchestrator
            children (`prepare_local_oz_child_launch`, no server row) and
            `ViewingAmbientConversation`/transcript-viewer threading is
            shared with the local viewer, so that plumbing is not a leaf
            deletion (same lesson as Track A's `DrivePanel`).

            Acceptance: `check -p warp --lib --all-targets`,
            `--no-default-features --features simplewarp --bin simplewarp`,
            and `--bin warp-oss` clean; clippy no warnings in touched
            files, format clean; nextest warp lib 4,653 default / 4,652
            simplewarp (both exactly the 4ci baselines — zero tests added
            or removed), `warp_cli` 76 passed.
      - [x] **CloudAgent entry origin, default mode, and onboarding hint
            residue (4ck) — DONE 2026-09-19.** `AgentViewEntryOrigin::CloudAgent`
            had zero producers; all matches collapsed to the local branch, with
            the `AmbientAgent` telemetry mapping falling with its sole use.
            Zero-state header/body and shortcuts go local-only;
            `ENTER_CLOUD_AGENT_VIEW_NEW_CONVERSATION_KEYSTROKE` deleted.
            `DefaultSessionMode::CloudAgent` kept for persisted compat but
            degrades to Terminal in the getter and is filtered from the settings
            dropdown. `OnboardingKeybindings::submit_to_cloud_agent` was
            write-only; deleted. 12 files, +70/−252.
      - [x] **Dead SSE stream walls on `ServerApi` (4cl) — DONE 2026-09-19.**
            `stream_agent_events`, `stream_agent_events_for_ancestor`, and
            `stream_agent_events_for_task` (the warp-server-rtc agent event-push
            SSE endpoints, all `local_only_error()` walls since 3e) had zero
            callers anywhere in the workspace — no production caller, no test
            caller — so the three methods went with their doc comments and no
            companion edits. 1 file, +0/−35. Deliberately left: the neighboring
            `generate_ai_input_suggestions` / `get_relevant_files` /
            `generate_am_query_suggestions` / `transcribe` walls all have live
            local callers (voice transcription, prompt suggestions, file
            context) that handle the error locally, so deleting them would break
            reachable UI; `notify_login` has its live `auth_manager` caller;
            `set_ambient_agent_task_id` threads into live local-orchestrator
            paths. `cargo check --all-targets -p integration` surfaces two
            `step.rs` unused-import warnings that reproduce identically on a
            clean stash — pre-existing, not this round's.

            Acceptance: `check -p warp --lib --all-targets` clean in both
            feature sets; `--bin simplewarp`, `--bin warp-oss`, and
            `--all-targets -p integration` clean (0 errors); clippy 0 errors,
            no warnings in the touched file; format clean; nextest warp lib
            4,653 default / 4,652 simplewarp (both exactly the 4cj baselines —
            zero tests added or removed), `warp_cli` 76 passed. Built and
            launched `./target/debug/simplewarp` — alive past 45s, no panics,
            no outbound TCP connections (the one ERROR in the log is the
            127.0.0.1:9282 bind colliding with the already-running release
            `SimpleWarp.app`; the WARN is the benign secure-storage NotFound).
      - [x] **Dead ambient header decoration on `BaseClient`/`ServerApi`
            (4cm) — DONE 2026-09-19.** `ambient_headers` had zero production
            callers anywhere in the workspace — only its own unit test — so
            the whole request-decoration layer went with no reachable behavior
            change: the three header constants
            (`AMBIENT_WORKLOAD_TOKEN_HEADER`, `CLOUD_AGENT_ID_HEADER`,
            `AGENT_SOURCE_HEADER`), `HeaderOverride` + `AmbientHeaderPolicy`
            (+ `inherit_all`/`for_task`/`workload_only`/`omit_all`/`Default`),
            the `ambient_workload_token` / `ambient_agent_task_id` /
            `agent_source` fields, `get_or_create_ambient_workload_token`
            (sole caller was `ambient_headers`; the `warp_isolation_platform`
            workload-token issue path goes with it), and
            `BaseClient::set_ambient_agent_task_id` (only writers were the
            header init paths below). 6 files, +8/−305.

            *Upstream writers deleted with it*: `ServerApi::
            set_ambient_agent_task_id`, the `agent_source` params on
            `ServerApi::new`/`new_with_parts`/`ServerApiProvider::new` with
            `lib.rs::determine_agent_source` (its `AgentSource::as_str` mapping
            was the sole caller, so `as_str` went too — `display_name` and
            `is_user_initiated` stay for the live local conversation source),
            the `lib.rs` `OZ_RUN_ID` parse-and-set startup block, and the
            `AgentDriverRunner::set_ambient_agent_task_id` helper with its
            `set(None)` call at driver setup. `OZ_RUN_ID`/`OZ_PARENT_RUN_ID`/
            `OZ_CLI`/`OZ_HARNESS` env threading for local child launches
            (`driver/harness`, `local_harness_launch`) is untouched, as is the
            local task-id identity threading (`controller`/
            `action_model`/`executor` `set_ambient_agent_task_id`,
            `conversation.task_id`, child-agent `task_context`,
            `active_agent_views_model`, transcript-viewer threading) — that is
            the non-leaf remainder from the 4cl note. `GraphqlRoutingConfig`
            and `graphql_request_options_with_token` stay (live via
            `AuthClientImpl::fetch_user_properties`). Tests: the
            `ambient_policy_supports_inherit_override_and_omit` test deleted;
            the graphql-options test kept minus its `set_ambient` line.

            Acceptance: `check -p warp --lib --all-targets` clean in both
            feature sets; `--bin simplewarp`, `--bin warp-oss`, and
            `--all-targets -p integration` clean (0 errors, only the two
            pre-existing `step.rs` unused-import warnings that reproduce on a
            clean stash); clippy byte-identical to the stash baseline in both
            configs (11 needless-returns + 1 single-element loop, 0 added, none
            in touched files); format clean. Nextest: warp lib 4,653 default /
            4,652 simplewarp (both exactly the 4cl baselines — zero app tests
            added or removed), `warp_cli` 76 passed,
            `warp_server_client` 7 passed (was 8 before the ambient-policy
            test went). Built and launched `./target/debug/simplewarp` — alive
            past 45s, empty log, no panics, no outbound TCP connections.
      - [x] **Dead `BaseClient` wrappers (4cn) — DONE 2026-09-19.**
            `http_client()`, `access_token_ignoring_validity`,
            `allowed_to_refresh_token`, `is_auth_refresh_allowed`,
            `event_sender()`, `send_auth_event` had zero production callers
            anywhere in the workspace — verified with `rg
            "\.http_client\(\)|\.access_token_ignoring_validity\(\)|
            \.allowed_to_refresh_token\(\)|\.is_auth_refresh_allowed\(\)|
            \.event_sender\(\)|\.send_auth_event\("` (the one
            `http_client()` hit is a different type's `AuthContext`;
            `allowed_to_refresh_token`'s only other hit is
            `AuthSession`'s own method, which stays). All six went with no
            companion edits, plus the now-write-only `event_sender` field
            (still passed through to `AuthSession::new`, just no longer
            stored). 1 file, +1/−32. Deliberately left: every `ServerApi`
            method still has a live caller (the four AI/transcribe walls have
            local UI callers that handle the error; telemetry is the one live
            payload); `AuthSession::allowed_to_refresh_token`,
            `AuthState::get_access_token_ignoring_validity`, and
            `owned_http_client`/`auth_session`/`anonymous_id`/`user_id`/
            `get_or_refresh_access_token`/`graphql_request_options_with_token`
            all stay live. Local-only safety: zero callers means zero behavior
            change — terminal, tabs, panes, settings, themes, BYOK AI,
            personal-folder creation, and all other local features untouched.

            Acceptance: `check -p warp_server_client --all-targets`,
            `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors;
            only the two pre-existing `step.rs` unused-import warnings that
            reproduce on a clean stash); clippy 0 errors in touched crate,
            warp-lib warning count byte-identical to stash baseline (14
            pre-existing); format clean. Nextest: warp lib 4,653 default /
            4,652 simplewarp (both exactly the 4cm baselines — zero tests
            added or removed), `warp_server_client` 7 passed. Built and
            launched `./target/debug/simplewarp` — clean startup, terminal
            server spawned, no panics.
      - [x] **Dead `DeviceCodeRequestTimedOut` auth error (4co) — DONE 2026-09-19.**
            The device-code OAuth flow (`warp login`, `request_device_code`/
            `exchange_device_access_token`) went in 4bn, leaving its error
            variant with zero producers anywhere in the workspace — verified
            with `rg "DeviceCodeRequestTimedOut"` (definition + three match
            arms, no construction site). Deleted the variant, its
            `is_actionable` arm, and the two now-dead match arms in
            `auth_manager.rs` (`on_user_fetched` error handling) and
            `root_view.rs` (`AuthFailed` handling). Both matches stay
            exhaustive over the five remaining variants. 3 files, +0/−5.
            Deliberately left: `InvalidStateParameter`/`MissingStateParameter`
            (live producers in the auth-redirect handler, with passing tests),
            `DeniedAccessToken`/`UserAccountDisabled` (constructed via
            `From<FirebaseError>`, live through `fetch_auth_tokens`), and
            `Unexpected` (generic anyhow wrapper). Local-only safety: zero
            producers means zero behavior change — terminal, tabs, panes,
            settings, themes, BYOK AI, and all other local features untouched.

            Acceptance: `check -p warp_server_client --all-targets`,
            `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors;
            only the two pre-existing `step.rs` unused-import warnings that
            reproduce on a clean stash); clippy 0 errors, no warnings in
            touched files (11 needless-returns + 1 single-element loop are the
            pre-existing baseline); format clean. Nextest:
            `warp_server_client` 7 passed; warp lib auth-redirect tests
            (InvalidStateParameter producers) 3 passed; full warp lib
            4,652 run with 1 notebooks flake (`test_command_block_dispatches_
            event`, passes in isolation and in the notebooks-only filter —
            the same cross-test-interference family 4by noted, not this
            round's). Not re-run in the app — deleted code was unreachable
            (no producers).
      - [x] **Seven orphaned GraphQL query modules (4cp) — DONE 2026-09-19.**
            Every warp-server GraphQL operation deleted in 4bh–4ch left its
            query module standing in `crates/graphql/src/api/queries/` — the
            operation was never built again, but the module is `pub` so the
            compiler stays silent. Swept all 25 query modules for external
            `Variables` refs: 24 came back zero (the 25th,
            `ListAIConversations`, is test-only via `ai_tests.rs`). Deleted
            the seven whose *every* public type is unreferenced anywhere
            outside its own file (verified per-type with exact-name grep,
            not just the operation): `tui_onboarding_markers` (TUI is gone
            since Phase 4 step 1), `get_ai_conversation_format`,
            `get_integrations_using_environment` (the `warp integration`
            surface went in 4o), `get_oauth_connect_tx_status`,
            `get_scheduled_agent_history` (ambient CLI went in 4aw),
            `list_warp_dev_images`, `task_git_credentials` (harness chain
            went in 4bd/4az). 8 files, +0/−328 (7 deleted + `mod.rs`).
            Deliberately left: the other 17 zero-`Variables` modules all
            still lend a fragment type to live code (e.g. `Runner`,
            `CloudEnvironment`, `ReferralInfo`, `Workspace`) — operation-dead
            but type-live, so each needs its own type-level trace before
            deleting. Local-only safety: zero callers means zero behavior
            change — terminal, tabs, panes, settings, themes, BYOK AI,
            personal-folder creation, and all other local features untouched.

            Acceptance: `check -p warp_graphql --all-targets`,
            `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors;
            only the two pre-existing `step.rs` unused-import warnings that
            reproduce on a clean stash); clippy 0 errors, no warnings in
            touched crate (warp-lib 11 needless-returns + 1 single-element
            loop are the pre-existing baseline, byte-identical); format
            clean. Nextest: `warp_graphql` 7 passed; warp lib 4,652
            simplewarp / 4,653 default, 0 failed (4 skipped both — exactly
            the 4ci baselines, zero tests added or removed);
            `warp_cli`+`warp_server_client` 83 passed. Built
            `./target/debug/simplewarp` (`--help` runs).
      - [x] **Seven more orphaned GraphQL query modules (4cq) — DONE 2026-09-19.**
            The fragment-type trace 4cp asked for, done with qualified
            `queries::<mod>` imports as the signal instead of naive type-name
            grep. Naive grep collides (`Workspace`, `Block`, `Space`, `Runner`,
            `Task` all name unrelated live types) and counts `target/` build
            output (the only `ReferralInfo` hits left). Qualified search shows
            13 of the 17 zero-operation modules have zero importers anywhere
            in `crates/` + `app/`; this round deletes the 7 whose every type
            is unambiguous even under naive grep: `get_referral_info`
            (referrals went in 3d), `get_discoverable_teams` (3t/3u),
            `get_ai_credit_availability` (credit availability went in 4d),
            `get_simple_integrations` (the `warp integration` surface went in
            4o), `user_github_info` + `user_repo_auth_status` +
            `suggest_cloud_environment_image` (the GitHub/environment half
            went in 4o/4p — the three share `RepoInput`/`RepoResult` names
            with each other only, no external importer, so all three go
            together). 8 files, +0/−437 (7 deleted + `mod.rs`).
            Correction to 4cp's "deliberately left" examples: `ReferralInfo`
            (target-only), `CloudEnvironment` (a `JsonObjectType` variant, not
            the query fragment), and `Workspace` (unrelated live types) are
            collisions, not live fragment users — `get_runners::Runner`
            (via `upsert_runner.rs`) is the real live-fragment case, and it
            stays. Deliberately left: the 6 remaining zero-operation modules
            (`get_ai_overages_for_workspace`, `get_blocks_for_user`,
            `get_cloud_environments`, `get_cloud_object`,
            `get_workspaces_metadata_for_user`, `task_attachments`) — each
            needs the same collision-aware trace before deleting, since their
            generic names make naive grep useless; plus the live
            `get_user`/`get_conversation_usage`/`get_updated_cloud_objects`/
            `get_runners` and the test-only `list_ai_conversations`
            (its `ai_tests.rs` query-shape test guards the live `ai.rs`
            conversion). Local-only safety: zero qualified importers means
            zero behavior change — terminal, tabs, panes, settings, themes,
            BYOK AI, personal-folder creation, and all other local features
            untouched.

            Acceptance: `check -p warp_graphql --all-targets`,
            `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors;
            only the two pre-existing `step.rs` unused-import warnings that
            reproduce on a clean stash); clippy 0 errors, no warnings in the
            touched crate (warp-lib 11 needless-returns + 1 single-element
            loop are the pre-existing baseline, byte-identical); format
            clean. Nextest: `warp_graphql` 7 passed; warp lib 4,652
            simplewarp (4,651 passed, 1 timeout flake in
            `test_random_concurrent_operations` — passes in isolation — 4
            skipped) and 4,653 default, 0 failed (exactly the 4ci baselines,
            zero tests added or removed); `warp_cli`+`warp_server_client` 83
            passed. Built `./target/debug/simplewarp`: `--help` runs, and a
            50s launch stays alive with no outbound TCP connections and 0
            panics.
      - [x] **Six remaining orphaned GraphQL query modules (4cr) — DONE 2026-09-19.**
            The collision-aware trace 4cq asked for, using qualified
            `queries::<mod>` imports as the signal instead of naive type-name
            grep. All six zero-operation modules have zero qualified importers
            anywhere in `crates/` + `app/` (no `warp_graphql::queries::<mod>`
            and no `crate::queries::<mod>` hits; `mod.rs` + own file only),
            no `api::queries` glob imports exist, and the distinctive type
            names (`GetAiOveragesForWorkspaceVariables`,
            `GetBlocksForUserVariables`,
            `GetCloudEnvironmentsQueryVariables`, `GetCloudObjectVariables`,
            `GetWorkspacesMetadataForUserVariables`, `CloudObjectInput`,
            `TaskAttachment`) appear only in their own files plus the server
            `schema.graphql` definition (not a client use); `app/` has zero
            hits for all of them. `TaskVariables`/`TaskInput`/`TaskData` are
            module-local (the mutations' `CreateAgentTaskVariables`/
            `UpdateAgentTaskVariables` are distinct types). Deleted:
            `get_ai_overages_for_workspace` (billing overages went in 4d),
            `get_blocks_for_user` (server block history; local blocks stay in
            SQLite), `get_cloud_environments` (remote VMs went in 4o/4p),
            `get_cloud_object` (server fetch; local objects stay in
            `CloudModel`/SQLite), `get_workspaces_metadata_for_user`
            (server workspaces/billing metadata), `task_attachments` (Agent
            Mode VM presigned-S3 attachments; local attachments untouched).
            7 files, +0/−~480 (6 deleted + `mod.rs`). Deliberately left: the
            live `get_user`/`get_conversation_usage`/
            `get_updated_cloud_objects`/`get_runners` (fragment users via
            `AuthClientImpl`, `gql_convert`/`ai.rs`, `UpdatedObjectInput`,
            `upsert_runner.rs`) and the test-only `list_ai_conversations`
            (its `ai_tests.rs` query-shape test guards the live `ai.rs`
            conversion). Local-only safety: zero qualified importers means
            zero behavior change — terminal, tabs, panes, settings, themes,
            BYOK AI, personal-folder creation, and all other local features
            untouched.

            Acceptance: `check -p warp_graphql --all-targets`,
            `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors;
            only the two pre-existing `step.rs` unused-import warnings that
            reproduce on a clean stash); clippy 0 errors, no warnings in the
            touched crate (warp-lib 11 needless-returns + 1 single-element
            loop are the pre-existing baseline, byte-identical); format
            clean. Nextest: `warp_graphql` 7 passed; warp lib 4,652
            simplewarp / 4,653 default, 0 failed (4 skipped both — exactly
            the 4ci baselines, zero tests added or removed);
            `warp_cli`+`warp_server_client` 83 passed.
      - [x] **Seven orphaned GraphQL mutation modules, team management (4cs) — DONE 2026-09-20.**
            The mutation-side mirror of 4cp/4cq/4cr: every warp-server GraphQL
            operation deleted with its feature left its mutation module
            standing in `crates/graphql/src/api/mutations/` — the operation
            was never built again, but the module is `pub` so the compiler
            stays silent. A whole-repo `git grep` for `mutations::` outside
            the directory returns exactly one live module
            (`create_anonymous_user`, type-live via `queries/get_user.rs` +
            `warp_server_auth/src/user.rs`); no `api::mutations` glob import
            exists anywhere. Deleted the seven team-management modules whose
            every public type is unreferenced outside its own file (verified
            per-type with exact-name grep, not just the operation):
            `create_team` (TeamClient went in 4bc), `delete_team_invite`,
            `join_team_with_team_discovery` (discoverable-teams went in
            3t/3u), `remove_user_from_team`, `rename_team`,
            `send_team_invite_email`, `set_team_discoverability` — each
            `*Variables`/`*Input`/`*Output`/`*Result` family appears only in
            its own file. 8 files, +0/−552 (7 deleted + `mod.rs`).
            Deliberately left: `create_anonymous_user` (the one live
            fragment-type case) and the ~50 remaining zero-operation modules
            — each needs the same per-type trace before deleting, since
            generic names make naive grep useless. Local-only safety: zero
            qualified importers means zero behavior change — terminal, tabs,
            panes, settings, themes, BYOK AI, personal-folder creation, and
            all other local features untouched.

            Acceptance: `check -p warp_graphql --all-targets`,
            `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors;
            only the two pre-existing `step.rs` unused-import warnings that
            reproduce on a clean stash); clippy byte-identical to the stash
            baseline in both configs (11 needless-returns + 1 single-element
            loop, 0 added, none in touched files); format clean. Nextest:
            `warp_graphql` 7 passed; warp lib 4,652 simplewarp / 4,653
            default (`--no-fail-fast`), 0 failed (4 skipped both — exactly
            the 4ci baselines, zero tests added or removed; the one
            fail-fast default failure, `test_command_block_dispatches_event`,
            is the known cross-test-interference flake from 4co — passes in
            isolation and in the full `--no-fail-fast` run);
            `warp_cli`+`warp_server_client` 83 passed. Not re-run in the
            app — deleted code was unreachable (no importers), same call as
            4co.
      - [x] **Seven more orphaned GraphQL mutation modules (4ct) — DONE 2026-09-20.**
            Billing pair `purchase_addon_credits` / `stripe_billing_portal`
            (metering/billing went in 4d), artifact pair
            `create_file_artifact_upload_target` /
            `confirm_file_artifact_upload` (4cc; the former owns the shared
            `FileArtifact` fragment type, so both fall together —
            `api::message::artifact_event::FileArtifact` in the app is a
            different type), `delete_ai_conversation` (conversation sync
            went in 4cg; the `ModelEvent::DeleteAIConversation` /
            persistence `delete_ai_conversation` names are unrelated local
            code), and agent-task pair `create_agent_task` /
            `update_agent_task` (cloud-run lifecycle went in 4ch; the
            `AIClient::create_agent_task` / `update_agent_task` mentions
            are historical spec docs — the trait itself is gone).
            Verified per-type with exact-name grep: the only
            `mutations::` importer repo-wide stays `create_anonymous_user`
            (type-live via `queries/get_user.rs` +
            `warp_server_auth/src/user.rs`); no `api::mutations` glob
            import exists. 8 files, +0/−~700 (7 deleted + `mod.rs`).
            Deliberately left: `create_anonymous_user` (live) and the ~43
            remaining zero-operation modules — same per-type trace, batch
            by batch. Local-only safety: zero qualified importers means
            zero behavior change — terminal, tabs, panes, settings, themes,
            BYOK AI, local conversation history/persistence, and all other
            local features untouched.

            Acceptance: `check -p warp_graphql --all-targets`,
            `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0
            errors; only the two pre-existing `step.rs` unused-import
            warnings that reproduce on a clean stash); clippy
            `-p warp_graphql --all-targets` clean; format clean. Nextest:
            `warp_graphql` 7 passed; warp lib 4,652 simplewarp
            (`--no-fail-fast`), 0 failed (4 skipped — exactly the 4ci
            baselines, zero tests added or removed);
            `warp_cli`+`warp_server_client` 83 passed. Default-feature
            suite not re-run — deleted code is unreachable in every
            config (grep is config-independent); default config covered
            by the lib/all-targets checks. App not re-run — same
            unreachable-code call as 4co/4cs.
      - [x] **Seven more orphaned GraphQL mutation modules (4cu) — DONE 2026-09-20.**
            Object-lifecycle batch, the server side of the ObjectClient +
            sync_queue vertical deleted in 4bg: `bulk_create_objects`,
            `delete_object`, `move_object`, `trash_object`,
            `untrash_object`, `empty_trash`, `record_object_action`.
            Verified per-type with exact-name grep: zero qualified
            `mutations::<mod>` importers repo-wide for all seven (the only
            `mutations::` importer anywhere stays `create_anonymous_user`);
            no `api::mutations` glob import exists. The naive-grep hits are
            unrelated local names — `DriveIndexAction::DeleteObject` /
            `MoveObject` / `TrashObject` / `UntrashObject` / `EmptyTrash`
            drive UI actions, `update_manager.delete_object_*` /
            `trash_object` / `record_object_action` local SQLite-backed
            methods (local objects stay in `CloudModel`/SQLite), and the
            `schema.graphql` hits are server schema definitions, not client
            uses (same call as 4cr). 8 files, +0/−~370 (7 deleted +
            `mod.rs`). Deliberately left: `create_anonymous_user` (live)
            and the ~36 remaining zero-operation modules — same per-type
            trace, batch by batch. Local-only safety: zero qualified
            importers means zero behavior change — terminal, tabs, panes,
            drive UI, trash/empty-trash dialogs, workflow trash/untrash,
            settings, themes, BYOK AI, personal-folder creation, and all
            other local features untouched.

            Acceptance: `check -p warp_graphql --all-targets`,
            `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0
            errors; only the two pre-existing `step.rs` unused-import
            warnings that reproduce on a clean stash); clippy
            `-p warp_graphql --all-targets` clean, `-p warp --lib`
            byte-identical to the stash baseline (12, 0 added, none in
            touched files); format clean. Nextest: `warp_graphql` 7
            passed; warp lib 4,653 default 0 failed plus 4,652 simplewarp
            (`--no-fail-fast`) with the single known
            `test_command_block_dispatches_event` cross-test-interference
            flake from 4co (passes in isolation; 4 skipped both — exactly
            the 4ci baselines, zero tests added or removed);
            `warp_cli`+`warp_server_client` 83 passed. App not re-run —
            same unreachable-code call as 4co/4cs/4ct.
      - [x] **Seven more orphaned GraphQL mutation modules (4cv) — DONE 2026-09-20.**
            Sharing/guests batch, the server side of the ObjectClient +
            sync_queue sharing vertical deleted in 4bg: `add_object_guests`,
            `remove_object_guest`, `update_object_guests`, `leave_object`,
            `set_object_link_permissions`,
            `remove_object_link_permissions`, `set_is_invite_link_enabled`.
            Verified per-type with exact-name grep: zero qualified
            `mutations::<mod>` importers repo-wide for all seven (the only
            `mutations::` importer anywhere stays `create_anonymous_user`,
            type-live via `queries/get_user.rs` +
            `warp_server_auth/src/user.rs`); no `api::mutations` glob import
            exists. 8 files, +0/−329 (7 deleted + `mod.rs`).
            Local-only safety: zero qualified importers means zero behavior
            change — terminal, tabs, panes, drive UI, sharing dialogs,
            settings, themes, BYOK AI, and all other local features
            untouched. Each of the five 4cv–4cz batch commits was
            `check -p warp_graphql --all-targets` verified individually;
            full acceptance once on the final state (below, 4cz).
      - [x] **Seven more orphaned GraphQL mutation modules (4cw) — DONE 2026-09-20.**
            Invite-link/team batch: `add_invite_link_domain_restriction`,
            `delete_invite_link_domain_restriction`, `reset_invite_links`
            (invite-link management went with the sharing vertical),
            `send_referral_invite_emails` (referral program, remote-only),
            `set_team_member_role`, `transfer_team_ownership` (TeamClient
            went in 4bc), `mint_custom_token` (remote auth issuance).
            Same per-type trace — zero qualified importers, no glob import.
            8 files, +0/−~350 (7 deleted + `mod.rs`). Local-only safety as
            4cv — login/identity stays on the local `warp_server_auth` path
            plus the live `create_anonymous_user` fragment type.
      - [x] **Seven more orphaned GraphQL mutation modules (4cx) — DONE 2026-09-20.**
            Object-CRUD batch, the server side of personal-folder creation
            (local creation stays in `CloudModel`/SQLite):
            `create_folder`, `update_folder`, `create_notebook`,
            `update_notebook`, `give_up_notebook_edit_access`,
            `grab_notebook_edit_access` (notebook baton is local-only since
            4bg), `create_workflow`. Same per-type trace — zero qualified
            importers, no glob import. The naive-grep hits on
            `CreateFolder` / `CreateNotebook` / `CreateWorkflow` are the
            unrelated local `DriveIndexEvent` variants in
            `app/src/drive/index.rs` + `panel.rs` (no `warp_graphql` /
            `mutations` import in either file — same call as 4cu). 8 files,
            +0/−~400 (7 deleted + `mod.rs`). Local-only safety as 4cv —
            personal-folder/notebook/workflow creation, drive UI, and the
            notebook baton untouched.
      - [x] **Seven more orphaned GraphQL mutation modules (4cy) — DONE 2026-09-20.**
            Workflow/string-object batch: `update_workflow`,
            `transfer_workflow_owner`, `transfer_notebook_owner`,
            `create_generic_string_object`,
            `update_generic_string_object`,
            `transfer_generic_string_object_owner`,
            `create_simple_integration` (remote integrations). Same
            per-type trace — zero qualified importers, no glob import. 8
            files, +0/−~430 (7 deleted + `mod.rs`). Local-only safety as
            4cv — workflows, notebooks, and string objects stay local.
      - [x] **Seven more orphaned GraphQL mutation modules (4cz) — DONE 2026-09-20.**
            Final batch — `mutations/` is now just `mod.rs` +
            `create_anonymous_user` (live): `share_block`,
            `unshare_block` (cloud block sharing), `DisplaySetting` /
            `BlockInput` generic names verified zero external hits;
            `update_onboarding_survey_status` (remote survey write —
            `OnboardingSurveyStatus`, `SurveyResponsesInput` families zero
            external hits), `update_workspace_settings` (remote billing
            settings write), `create_managed_mcp_client_config`
            (`ManagedMcpTransportKind` zero external hits outside a specs
            doc mention), `delete_runner` / `upsert_runner` (cloud runners;
            `RunnerInput`, `MacOsConfigInput`, `LinuxConfigInput` zero
            external hits). 8 files, +0/−~540 (7 deleted + `mod.rs`).
            Total across 4cv–4cz: 36 files, +0/−2,053 — every orphaned
            mutation module is gone.

            Acceptance (final state): `check -p warp_graphql
            --all-targets`, `check -p warp --lib --all-targets`, `--bin
            simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
            clean (0 errors; only the two pre-existing `step.rs`
            unused-import warnings that reproduce on a clean stash); clippy
            `-p warp_graphql --all-targets` clean, `-p warp --lib` 12
            warnings byte-identical to the stash baseline (0 added, none in
            touched files); format clean. Nextest: `warp_graphql` 7 passed;
            warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`), 0
            failed (4 skipped both — exactly the 4ci baselines, zero tests
            added or removed, no flakes this round);
            `warp_cli`+`warp_server_client` 83 passed. App not re-run —
            same unreachable-code call as 4co/4cs/4ct/4cu.
      - [x] **Two remaining orphaned GraphQL query modules (4da) — DONE 2026-09-20.**
            `get_runners` (cloud runners; the `delete_runner` /
            `upsert_runner` mutations went in 4cz and `FactoryClient` itself
            is gone — only doc-comment mentions remain, and the local runner
            picker already degrades to its "environment default" empty state
            with no server to answer) and `list_ai_conversations` (cloud
            conversation listing; conversation sync went in 4cg — the only
            importer repo-wide was the crate's own `ai_tests.rs` guard test
            for the dead restore query, removed with the module). Verified
            per-type with exact-name grep: zero qualified
            `queries::<mod>` importers and zero operation call sites
            (`get_runners(`, `list_ai_conversations(`,
            `list_ai_conversation_metadata(`) repo-wide for both; no
            `api::queries` glob import exists. The naive-grep hits are
            unrelated local names — `RunnerArchArg` / `RunnerOsArg` CLI
            enums, the local blocklist `AIConversationMetadata` type, and
            the `MacosRunnersControl` experiment-flag variants. `ai.rs`
            keeps its own `AIConversation` / `AgentHarness` /
            `ConversationUsage` types (the deleted module imported them, not
            vice versa). 4 files, +0/−~260 (2 deleted + `mod.rs` +
            `ai_tests.rs`). Deliberately left: `get_conversation_usage`
            (fragment types live via `ai.rs` + `gql_convert.rs`),
            `get_updated_cloud_objects` (`UpdatedObjectInput` live via the
            `cloud_object` + `cloud_objects` crates), `get_user` (live auth
            path) — their operations may be dead but the modules are not
            orphaned, same per-type-trace rule as 4cs–4cz. Local-only
            safety: zero production importers means zero behavior change —
            terminal, tabs, panes, drive UI, local conversation
            history/persistence, BYOK AI, settings, themes, and all other
            local features untouched.

            Acceptance: `check -p warp_graphql --all-targets`,
            `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0
            errors); clippy `-p warp_graphql --all-targets` clean,
            `-p warp --lib` byte-identical to the stash baseline (0 added,
            none in touched files); format clean. Nextest: `warp_graphql`
            6 passed (was 7 — the removed restore-query guard test, zero
            other changes); warp lib 4,653 default / 4,652 simplewarp
            (`--no-fail-fast`), 0 failed (4 skipped both — exactly the 4ci
            baselines, zero tests added or removed, no flakes this round);
            `warp_cli`+`warp_server_client` 83 passed. App not re-run —
            same unreachable-code call as 4co/4cs/4ct/4cu/4cz.
      - [x] **Orphaned GraphQL subscription modules (4db) — DONE 2026-09-20.**
            The subscription-side mirror of 4cp–4da: `GetWarpDriveUpdates`
            (the warp-server Warp Drive live-update feed — object
            create/update/delete, permissions, team memberships, ambient
            task ticks) plus the generic `start_graphql_streaming_operation`
            websocket helper that was its only transport. Verified with
            exact-name grep: zero `warp_graphql::subscriptions` importers,
            zero `GetWarpDriveUpdates` / `WarpDriveUpdate` /
            `start_graphql_streaming_operation` call sites repo-wide — the
            naive-grep hits are unrelated local names (view event
            subscriptions, paid subscriptions). The SSE-stream walls on
            `ServerApi` went in 4cl, leaving these with no caller; nothing
            else ever subscribed. 3 files, +0/−~170 (2 deleted +
            `mod.rs`). Deliberately left: the `ObjectUpdateMessage` cloud
            variant in `cloud_objects` (4aw: its arm is an empty no-op, a
            separate layer) and the `websocket` crate itself (shared
            transport, not subscription-specific). Local-only safety: zero
            production importers means zero behavior change — terminal,
            tabs, panes, drive UI, local conversation history/persistence,
            BYOK AI, settings, themes, and all other local features
            untouched.

            Acceptance: `check -p warp_graphql --all-targets`,
            `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0
            errors; the 2 `integration_testing` unused-import warnings are
            pre-existing, unrelated files); clippy `-p warp_graphql
            --all-targets` clean, `-p warp --lib` 14 warnings —
            byte-identical count to the stash baseline (0 added, none in
            touched files); format clean. Nextest: `warp_graphql` 6
            passed; warp lib 4,653 default / 4,652 simplewarp
            (`--no-fail-fast`), 0 failed (4 skipped both — exactly the 4da
            baselines, zero tests added or removed, no flakes this round);
            `warp_cli`+`warp_server_client` 83 passed. App not re-run —
            same unreachable-code call as 4co/4cs/4ct/4cu/4cz/4da.
      - [x] **Zero-reader `FeatureFlag::HarnessSessionHeader` (4dc) — DONE 2026-09-20.**
            Definition-only delete, the 4aq precedent: `grep -rn
            "FeatureFlag::HarnessSessionHeader"` repo-wide returned only the
            declaration — no production reader, no test reader, no cargo
            feature, no `features.rs` mapping, no DOGFOOD/PREVIEW/RELEASE/
            RUNTIME membership. The remaining `HarnessSessionHeader` hits are
            the unrelated live `RichContentMetadata::HarnessSessionHeader`
            enum (terminal block header rendering, untouched). The flag's own
            doc ("styled header showing CLI name + status icon" for harness
            CLI commands) names the cloud-agent harness vertical whose client
            (`HarnessSupportClient`) went in 4bd — remote-only by design, and
            with zero readers deleting the variant is zero behavior change
            either way. 1 file, +0/−3. `FLAG_STATES`/`USER_PREFERENCE_MAP`
            are cardinality-sized, no count to update (per §5).

            Acceptance: `check -p warp_features --all-targets`, `check -p
            warp --lib --all-targets`, `--bin simplewarp`, `--bin warp-oss`
            clean (0 errors); clippy `-p warp_features --all-targets` clean;
            format clean; nextest `-p warp_features` 1 passed / 1 skipped.
            Local-only safety: zero readers means zero behavior change —
            terminal, tabs, panes, drive UI, local conversation
            history/persistence, BYOK AI, settings, themes, and all other
            local features untouched.
      - [x] **Dead `ServerApi::notify_login` no-op + single call site (4dd) — DONE 2026-09-20.**
            The `/client/login` server hello went local-only earlier, leaving
            `notify_login` as a `log::debug!` no-op with exactly one caller —
            the trailing line of the login-success telemetry-flush block in
            `auth_manager.rs`. Deleted the method (with its doc) and the call
            line; the surrounding flush block is unchanged, so the queued
            identify + Login events still flush the same way. Verified with
            `grep -rn "notify_login"`: zero hits remain. 2 files, +0/−7.
            Deliberately left: the neighboring `send/flush/persist_telemetry`
            methods (live telemetry payload, 4ca item 7 scope decision) and
            the four `local_only_error()` AI/transcribe walls (live local
            callers per 4cl). Local-only safety: no-op method means zero
            behavior change except one dropped debug line — login, terminal,
            tabs, panes, BYOK AI, settings, themes, and all other local
            features untouched.

            Acceptance: `check -p warp --lib --all-targets`, `--bin
            simplewarp`, `--bin warp-oss` clean (0 errors); clippy `-p warp
            --lib --all-targets` warning count byte-identical to the stash
            baseline (14 pre-existing, 0 added, none in touched files);
            format clean; nextest `-E 'test(auth)'` 44 passed / 4613
            skipped. App not re-run — deleted code was unreachable (no-op).
      - [x] **Three orphaned cloud-agent capacity-modal telemetry events (4de) — DONE 2026-09-20.**
            The `cloud_agent_capacity_modal` UI went in 4ch, leaving its three
            telemetry events with zero send sites anywhere in the workspace —
            verified with `grep -rn "CloudAgentCapacityModal"` (definition +
            five match-arm groups in `events.rs`, no constructor, no test
            reference) and `grep -rln "CapacityModal\|ConcurrencyModal"`
            (only `events.rs` — no modal UI, no string-name reference outside
            the event-name mapping). Deleted the three variants (with docs)
            and all five match arms (properties `None`, redaction-chain
            membership, `EnablementState::Always`, the
            `AmbientAgent.ConcurrencyModal.*` event names, descriptions). 1
            file, +0/−27. Deliberately left: the neighboring `ComputerUse`
            events (live local approval flow), the `CodexModal` pair (live
            modal), and the whole ambient task-id identity plumbing
            (`AmbientAgentTaskId` params on the `ComputerUse` events stay —
            `new()` backs local-only orchestrator children and the viewer
            threading is shared with the local viewer, so that plumbing is
            not a leaf deletion). Local-only safety: zero constructors means
            zero behavior change — terminal, tabs, panes, BYOK AI, settings,
            themes, and all other local features untouched; only three
            RudderStack event names that could never fire are gone.

            Acceptance: `check -p warp --lib --all-targets`, `--bin
            simplewarp`, `--bin warp-oss` clean (0 errors); clippy `-p warp
            --lib --all-targets` 14 warnings byte-identical to the baseline
            (0 added, none in the touched file); format clean. Nextest: warp
            lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`), 0 failed
            (4 skipped both — exactly the 4ci baselines, zero tests added or
            removed, no flakes this round). App not re-run — deleted code was
            unreachable (no constructors).
      - [x] **Ten orphaned team/sharing telemetry events (4df) — DONE 2026-09-20.**
            Commit `ee87b6f6e` (`app/src/server/telemetry/events.rs`,
            +0/−70): the team/sharing UI went with the 4bg ObjectClient
            vertical, leaving ten telemetry events with zero send sites.
            Same leaf class as 4de.
      - [x] **Zero-reader `FeatureFlag::NamedAgents` + cargo feature (4dg) — DONE 2026-09-20.**
            The 4dc precedent, second instance: `grep -rn "NamedAgents"`
            repo-wide returned only the definition
            (`crates/warp_features/src/lib.rs`) plus the write-only
            `features.rs` mapping — no `is_enabled` reader in production
            or tests, no DOGFOOD/PREVIEW/RELEASE membership. The flag gated
            API keys scoped to named agent identities in the cloud API-key
            management UI; the named-agent mgmt CLI went in 4bj and the
            account-gated Oz Cloud API Keys GUI page went in 4bz, so it is
            remote-only by design, and with zero readers deleting the
            variant is zero behavior change either way. Unlike 4dc the
            variant was wired to a cargo feature (`named_agents`, in BOTH
            the `default` and `simplewarp` sets), so the feature went with
            it: variant + doc (lib.rs), the two-line cfg mapping
            (features.rs), both set memberships plus the `named_agents = []`
            definition (app/Cargo.toml). `FLAG_STATES`/`USER_PREFERENCE_MAP`
            are cardinality-sized, no count to update (per §5). The
            remaining `specs/REMOTE-1696` hits are the historical Oz CLI
            spec for the already-deleted surface, untouched. 3 files,
            +0/−9. Local-only safety: nothing ever read the flag, so both
            feature sets resolve and behave exactly as before — terminal,
            tabs, panes, drive UI, local conversation history/persistence,
            BYOK AI, settings, themes, and all other local features
            untouched.

            Acceptance: `check -p warp_features --all-targets`, `check -p
            warp --lib --all-targets`, `--bin simplewarp`, `--bin warp-oss`,
            plus `--no-default-features --features simplewarp --bin
            simplewarp` (both sets touched) clean (0 errors); clippy `-p
            warp_features --all-targets` clean, `-p warp --lib` 14 warnings
            byte-identical to the stash baseline (0 added, none in touched
            files); format clean; nextest `-p warp_features` 1 passed / 1
            skipped. Nextest: warp lib 4,653 default / 4,652 simplewarp
            (`--no-fail-fast`), 0 failed (4 skipped both — exactly the 4ci
            baselines, zero tests added or removed, no flakes this round).
            App not re-run — deleted code was unreachable (no readers).
      - [x] **Eight orphaned auth/login/signup telemetry events + `LoginEventSource`
            (4dh) — DONE 2026-09-21.** Same leaf class as 4de/4df: `AuthView`
            went in 3o (login/signup buttons can't exist) and `initiate_user_signup`
            now just shows the local-only toast (sends no telemetry), leaving eight
            events with zero constructors repo-wide — verified with bare-name
            `git grep` (not just `TelemetryEvent::`) plus event-name string search,
            all zero outside `events.rs`: `SignUpButtonClicked`,
            `LoginButtonClicked`, `LoginLaterButtonClicked`,
            `LoginLaterConfirmationButtonClicked`, `AuthCommonQuestionClicked`,
            `AuthToggleFAQ`, `OpenAuthPrivacySettings`,
            `InitiateAnonymousUserSignup`. Deleted the eight variants with all five
            match-arm groups (properties, redaction chain, enablement, event names,
            descriptions). `LoginEventSource` (`OnboardingSlide`, `AuthModal`) had
            zero uses outside `events.rs` and lost its last four readers here, so
            the enum went too. Deliberately left: `AnonymousUserSignupEntrypoint`
            (live — `pane_group`, `terminal/input`, `terminal/view`,
            `workspace/view` still call `initiate_user_signup` with it) and the
            neighboring `AnonymousUser*`/`Login`/`InitiateReauth` events (live
            variants share two enablement/redaction chains — only the dead
            disjuncts were removed). 1 file, +0/−80. Local-only safety: zero
            constructors means zero behavior change — terminal, tabs, panes, BYOK
            AI, settings, themes, and all other local features untouched; only
            eight RudderStack event names that could never fire are gone.

            Acceptance: `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss` clean (0 errors); clippy `-p warp --lib` 11
            needless-returns + 1 single-element loop, byte-identical to the
            pre-round baseline (0 added, none in the touched file); format clean.
            Nextest: warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`),
            0 failed (4 skipped both — exactly the 4ci baselines, zero tests added
            or removed, no flakes); `warp_graphql`+`warp_server_client`+`warp_cli`
            89 passed. App not re-run — deleted code was unreachable (no
            constructors).
      - [x] **Three orphaned billing/revenue telemetry events (4di) — DONE 2026-09-21.**
            Same leaf class as 4de/4df/4dh: the metering/billing UI went in 4d
            (`purchase_addon_credits` / `stripe_billing_portal` went in 4ct,
            `get_ai_credit_availability` in 4cq, overages in 4cr), leaving three
            `revenue.*` events with zero constructors repo-wide — verified with
            bare-name `git grep` (not just `TelemetryEvent::`) plus event-name
            string search, all zero outside `events.rs`:
            `OutOfCreditsBannerClosed`, `AutoReloadModalClosed`,
            `AutoReloadToggledFromBillingSettings`. Deleted the three variants
            with all five match-arm groups (properties, `contains_ugc` chain,
            enablement, event names, descriptions). `AutoReloadModalAction` and
            `OutOfCreditsBannerAction` had zero uses outside the deleted
            variants, so both helper enums went too. Deliberately left:
            `TierLimitHit` (orphaned but its banner UI still renders locally —
            a separate call), the `OutOfCreditsResponse` quota-error parser and
            `AIApiError::QuotaLimit` in `server_api.rs` (live local error
            handling, not banner telemetry), and the whole ambient task-id
            identity plumbing (non-leaf — backs local-only orchestrator
            children and shares threading with the local viewer). 1 file,
            +0/−93. Local-only safety: zero constructors means zero behavior
            change — terminal, tabs, panes, BYOK AI, settings, themes, and all
            other local features untouched; only three RudderStack event names
            that could never fire are gone.

            Acceptance: `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss` clean (0 errors); clippy `-p warp --lib` warning
            list byte-identical to the pre-round baseline (11 needless-returns,
            0 added, none in the touched file); format clean.
            Nextest: warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`),
            0 failed (4 skipped both — exactly the 4ci baselines, zero tests added
            or removed, no flakes); `warp_graphql`+`warp_server_client`+`warp_cli`
            89 passed. App not re-run — deleted code was unreachable (no
            constructors).
      - [x] **Eight orphaned unit telemetry events (4dj) — DONE 2026-09-21.**
            Same leaf class as 4de/4df/4dh/4di: onboarding, drive-object,
            approvals-modal, anonymous-lockout, and secret-regex surfaces went
            in earlier rounds, leaving eight unit events with zero constructors
            repo-wide — verified with bare-name `git grep` (not just
            `TelemetryEvent::`) plus event-name string search, all zero outside
            `events.rs`: `SkipOnboardingSurvey`, `DismissWelcomeTips`,
            `DeletedWorkflow`, `DeletedNotebook`, `ToggleApprovalsModal`,
            `AnonymousUserExpirationLockout`, `CustomSecretRegexAdded`,
            `DriveSharingOnboardingBlockShown`. Deleted the eight variants with
            all five match-arm groups (properties, `contains_ugc` chain,
            enablement, event names, descriptions). No helper types fell — all
            eight are unit variants, so no payload structs/enums were attached.
            Deliberately left: the 12 remaining zero-outside-`events.rs`
            variants, all with payloads needing per-type traces
            (`ContextMenuToggleGitPromptDirtyIndicator`, `OpenChangelogLink`,
            `CompleteWelcomeTipFeature`, `ConversationListItemOpened`,
            `ConversationListLinkCopied`, `PromptSuggestionShown`,
            `TierLimitHit`, `OpenedSharingDialog`, `FileExceededContextLimit`,
            `MCPServerAdded`, `RecentMenuItemSelected`, `AgentViewExited`),
            plus the standing non-leaves (`TierLimitHit` banner UI,
            `OutOfCreditsResponse`/`AIApiError::QuotaLimit` local error
            handling, ambient task-id plumbing). 1 file, +1/−53 (the +1 is the
            collapsed `AnonymousUserLinkedFromBrowser` OR-chain line).
            Local-only safety: zero constructors means zero behavior change —
            terminal, tabs, panes, BYOK AI, settings, themes, and all other
            local features untouched; only eight RudderStack event names that
            could never fire are gone.

            Acceptance: `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss` clean (0 errors); clippy `-p warp --lib` warning
            list byte-identical to the pre-round baseline (11 needless-returns,
            0 added, none in the touched file); format clean.
            Nextest: warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`),
            0 failed (4 skipped both — exactly the 4ci baselines, zero tests added
            or removed, no flakes); `warp_graphql`+`warp_server_client`+`warp_cli`
            89 passed. App not re-run — deleted code was unreachable (no
            constructors).
      - [x] **One orphaned changelog telemetry event (4dk) — DONE 2026-09-21.**
            Same leaf class as 4de/4df/4dh/4di/4dj: the server-fed changelog went
            in 3g (`ChangelogModel` via `ServerApiProvider`, the `/changelog`
            slash command), leaving `OpenChangelogLink` — the first of 4dj's 12
            leftover payload-carrying variants — with zero constructors repo-wide
            (verified with bare-name `git grep` plus event-name string search, all
            zero outside `events.rs`). Deleted the variant with all five match-arm
            groups (properties, `contains_ugc` chain, enablement, event names,
            descriptions). Primitive `String` payload, so no helper types fell.
            1 file, +0/−8. Local-only safety: zero constructors means zero
            behavior change — terminal, tabs, panes, BYOK AI, settings, themes,
            and all other local features untouched; only one RudderStack event
            name that could never fire is gone.

            Acceptance: `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors; only
            the two pre-existing `step.rs` unused-import warnings that reproduce on
            a clean stash); clippy `-p warp --lib` 11 needless-returns + 1
            single-element loop, byte-identical to the baseline (0 added, none in
            the touched file); format clean. Nextest: warp lib 4,653 default /
            4,652 simplewarp (`--no-fail-fast`), 0 failed (4 skipped both — exactly
            the 4ci baselines, zero tests added or removed, no flakes);
            `warp_graphql`+`warp_server_client`+`warp_cli` 89 passed. App not
            re-run — deleted code was unreachable (no constructors).
      - [x] **Three orphaned context-menu/conversation-list telemetry events
            (4dl) — DONE 2026-09-21.** Same leaf class as 4de–4dk: the second
            slice of 4dj's 12 leftover payload-carrying variants —
            `ContextMenuToggleGitPromptDirtyIndicator` (primitive `bool`
            payload, same shape as 4dk's `OpenChangelogLink`),
            `ConversationListItemOpened` and `ConversationListLinkCopied`
            (primitive `bool` payloads; the cloud conversation-sync producers
            went in 4cg). All three verified with bare-name `git grep` over
            all files plus event-name/description string search — zero hits
            outside `events.rs` (only the plan.md ledger mentions). Deleted
            the three variants with all five match-arm groups (properties,
            `contains_ugc` chain, enablement, event names, descriptions). No
            helper types fell — all three payloads are primitive `bool`, and
            the two conversation-list enablement arms were removed from their
            shared `AgentViewConversationListView`-gated OR-chain with the
            live `ConversationListViewOpened`/`ConversationListItemDeleted`
            arms left intact (chain collapsed to one line, rustfmt-clean).
            Local-only safety: zero constructors means zero behavior change —
            the git-prompt dirty-indicator toggle itself, the local
            conversation list view/open/delete flows, terminal, tabs, panes,
            BYOK AI, settings, themes, and all other local features untouched;
            only three RudderStack event names that could never fire are gone.

            Acceptance: `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors; only
            the two pre-existing unused-import warnings that reproduce on
            a clean stash); clippy `-p warp --lib` 11 needless-returns, all in
            untouched `terminal/input.rs` (0 added, none in the touched file);
            format clean. Nextest: warp lib 4,653 default /
            4,652 simplewarp (`--no-fail-fast`), 0 failed (4 skipped both — exactly
            the 4ci baselines, zero tests added or removed, no flakes);
            `warp_graphql`+`warp_server_client`+`warp_cli` 89 passed. App not
            re-run — deleted code was unreachable (no constructors).
      - [x] **Seven orphaned payload-carrying telemetry events
            (4dm) — DONE 2026-09-21.** Same leaf class as 4de–4dl: the third
            slice of 4dj's 12 leftover payload-carrying variants —
            `CompleteWelcomeTipFeature` (welcome-tips completion; the tips UI
            itself stays on the live `Tip`/`TipAction` types),
            `PromptSuggestionShown` (server-driven suggestion banner; the
            legacy/static/accept siblings stay live in `terminal/view.rs`),
            `OpenedSharingDialog` (sharing dialog; the dialog went in 4bg),
            `FileExceededContextLimit` (AI context-limit; the `AgentModeError`
            sibling stays live), `MCPServerAdded` (MCP collection;
            `MCPServerSpawned`/`MCPTemplate*` stay live in
            `templatable_manager/native.rs`), `RecentMenuItemSelected`
            (zero-state recents; primitive `&'static str` payload), and
            `AgentViewExited` (sibling `AgentViewEntered` stays live in
            `agent_view.rs`). All seven verified with bare-name `git grep` over
            all files plus event-name/description string search — zero hits
            outside `events.rs` (only the plan.md ledger mentions). Deleted
            the seven variants with all five match-arm groups (properties,
            `contains_ugc` chain, enablement, event names, descriptions), plus
            the two helper types left with zero users (`OpenedSharingDialogEvent`,
            `MCPServerTelemetryMetadata`) and the now-unused
            `WelcomeTipFeature` enum + impl in `tips/mod.rs` (a `TipAction`
            duplicate whose only reader was the deleted variant;
            `WELCOME_TIP_FEATURE_LENGTH` stays — `tip_view.rs` still reads it).
            The three shared enablement OR-chains (`AgentView`,
            `McpServer`, prompt-suggestion group) collapsed to their live arms;
            the four single-line arms went outright. Deliberately kept:
            `SharingDialogSource` (live via `WorkspaceAction::
            OpenObjectSharingSettings`), `PromptSuggestionViewType` (live via
            the accepted/static siblings), `TelemetryAgentViewEntryOrigin`
            (live via `AgentViewEntered`), `AIIdentifiers`/`CloudObjectTelemetry
            Metadata` (live via many siblings), and `TierLimitHit` (orphaned
            but its tier-limit banner UI still renders locally — a separate
            call). Local-only safety: zero constructors means zero behavior
            change — welcome tips, prompt-suggestion banners, sharing flows,
            MCP servers, recents, agent view enter/exit, terminal, tabs, panes,
            BYOK AI, settings, themes, and all other local features untouched;
            only seven RudderStack event names that could never fire are gone.

            Acceptance: `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors; only
            the two pre-existing `step.rs` unused-import warnings that reproduce on
            a clean stash); clippy `-p warp --lib` 14 warnings byte-identical to
            the stash baseline (0 added, none in touched files); format clean.
            Nextest: warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`),
            0 failed (4 skipped both — exactly the 4ci baselines, zero tests added
            or removed; the one simplewarp fail-fast failure,
            `test_command_block_dispatches_event`, is the known
            cross-test-interference flake from 4co — passes in isolation);
            `warp_graphql`+`warp_server_client`+`warp_cli` 89 passed. App not
            re-run — deleted code was unreachable (no constructors).
      - [x] **One orphaned tier-limit telemetry event
            (4dn) — DONE 2026-09-22.** Same leaf class as 4de–4dm: the last
            of 4dj's 12 leftover payload-carrying variants —
            `TierLimitHit` (struct `TierLimitHitEvent` with `team_uid` +
            `feature`). Verified with bare-name `git grep` over all files
            plus event-name/description string search — zero hits outside
            `events.rs` (only the plan.md ledger mentions). Deleted the
            variant with all five match-arm groups (properties,
            `contains_ugc` chain, enablement, event names, descriptions)
            plus the now-unused struct. Deliberately kept:
            `SharedObjectLimitHitBannerViewPlansButtonClicked` (live — the
            tier-limit banner's View Plans button still sends it),
            `is_at_tier_limit_for_object_type` /
            `has_capacity_for_shared_notebooks` /
            `has_capacity_for_shared_workflows` /
            `render_shared_object_limit_hit_banner` (live local banner UI —
            the standing non-leaf the plan flagged; this round touches only
            the orphaned RudderStack event, not the banner), and the whole
            ambient task-id identity plumbing (non-leaf — backs local-only
            orchestrator children and shares threading with the local
            viewer). 1 file, +0/−13. Local-only safety: zero constructors
            means zero behavior change — tier-limit banner rendering,
            terminal, tabs, panes, BYOK AI, settings, themes, and all other
            local features untouched; only one RudderStack event name that
            could never fire is gone.

            Acceptance: `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors; only
            the two pre-existing `step.rs` unused-import warnings that reproduce on
            a clean stash); clippy `-p warp --lib` 14 warnings byte-identical to
            the stash baseline (0 added, none in touched files); format clean.
            Nextest: warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`),
            0 failed (4 skipped both — exactly the 4ci baselines, zero tests added
            or removed, no flakes);
            `warp_graphql`+`warp_server_client`+`warp_cli` 89 passed. App not
            re-run — deleted code was unreachable (no constructors).
      - [x] **One orphaned launch-config telemetry event
            (4do) — DONE 2026-09-22.** Same leaf class as 4de–4dn, found by
            scripting the check rather than working off 4dj's leftover list:
            extracted all 346 `TelemetryEvent` variant names and grepped each
            for `TelemetryEvent::<Name>` outside `events.rs` — exactly one
            zero-hit variant, `OpenLaunchConfigSaveModal` (unit, no payload).
            Verified with bare-name `git grep` plus event-name/description
            string search — zero hits outside `events.rs` (only the plan.md
            ledger mentions). The similarly-named
            `WorkspaceAction::OpenLaunchConfigSaveModal` is live local UI (the
            binding, the `view.rs` dispatch arm, the modal itself) and is
            untouched; the sibling `TelemetryEvent::SaveLaunchConfig` is still
            sent from `save_modal.rs` on actual save and stays. Deleted the
            variant with all six match-arm groups (properties, `contains_ugc`
            chain, enablement, event names, descriptions — no payload struct
            since the variant is unit). 1 file, +0/−6. Local-only safety:
            zero constructors means zero behavior change — launch-config save
            modal, terminal, tabs, panes, BYOK AI, settings, themes, and all
            other local features untouched; only one RudderStack event name
            that could never fire is gone.

            Acceptance: `check -p warp --lib --all-targets`, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors);
            clippy `-p warp --lib` byte-identical to the stash baseline
            (0 added, none in touched files); format clean.
            Nextest: warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`),
            0 failed (4 skipped both — exactly the 4ci baselines, zero tests added
            or removed, no flakes);
            `warp_graphql`+`warp_server_client`+`warp_cli` 89 passed. App not
            re-run — deleted code was unreachable (no constructors).
      - [x] **Dead server-sync version plumbing
            (4dp) — DONE 2026-09-22.** Same zero-caller leaf class as 4de–4do,
            found by grepping the sync layer rather than the telemetry enum:
            `CloudModel::get_versions_for_all_objects` had zero callers
            repo-wide (definition only), and `CloudObject::versions` (whose own
            doc says "timestamps are sent to the server for comparison") had
            exactly one caller — that dead method. Deleted the method, the
            trait declaration + impl, and the two now-unused imports
            (`ObjectsToUpdate` in `persistence.rs`, `UpdatedObjectInput` +
            `ObjectActions` in `cloud_object/mod.rs`). The compiler's own
            cascade named the rest: `ObjectActions::
            get_latest_processed_at_ts` (whose doc says "most recent
            server-synced action ... whether or not we should accept some
            update from the server") lost its only caller here, so it went in
            the same pass. 3 files, +2/−71. Deliberately left:
            `ObjectsToUpdate` (the struct in `crates/cloud_objects`, now
            write-only — crate-level cleanup is its own round),
            `UpdatedObjectInput` (the GraphQL input type, still referenced by
            that struct + the `get_updated_cloud_objects` query module), and
            `update_objects_from_initial_load` (live local-SQLite path, a
            different function). Local-only safety: zero callers means zero
            behavior change — local persistence, local objects, terminal,
            tabs, panes, BYOK AI, settings, themes, and all other local
            features untouched; only the "which objects need server updates"
            payload that could never be sent is gone.

            Acceptance: `check -p warp --lib --all-targets` both feature sets,
            `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
            clean (0 errors; only the two pre-existing `step.rs` unused-import
            warnings that reproduce on a clean stash); clippy `-p warp --lib`
            14 warnings byte-identical to the stash baseline (0 added, none in
            touched files); format clean.
            Nextest: warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`),
            0 failed (4 skipped both — exactly the 4do baselines, zero tests added
            or removed, no flakes). App not
            re-run — deleted code was unreachable (no callers).
      - [x] **Dead token-refresh gate
            (4dq) — DONE 2026-09-22.** Same zero-caller leaf class as 4de–4dp,
            found by re-sweeping 4ca's BaseClient zero-caller list rather than
            the telemetry enum: `AuthSession::allowed_to_refresh_token` had
            zero production callers repo-wide (definition + one assert in its
            own `session_tests.rs` only), and `Credentials::
            is_externally_managed` in `warp_server_auth` had exactly one
            caller — that dead method. Deleted the method, the predicate, and
            the assert. 3 files, +0/−20. Deliberately left:
            `get_or_refresh_access_token` (live — bearer/Firebase/ApiKey arms
            still serve the daemon token, cached Firebase tokens, and local
            auth flows; the bearer test keeps asserting the
            no-refresh-event shape through it), `exchange_credentials`
            (live login-token wrap), and all four `AuthEvent` variants (each
            still constructed and matched: `NeedsReauth` fires from the
            Firebase refresh arm, `AccessTokenRefreshed` from the same arm to
            the remote-server bearer forwarder + builtin-MCP re-sync,
            `UserAccountDisabled`/`StagingAccessBlocked` matched in the
            server-api event loop + MCP manager). Local-only safety: zero
            callers means zero behavior change — login/logout, daemon bearer,
            cached tokens, terminal, tabs, panes, BYOK AI, settings, themes,
            and all other local features untouched; only the "may this
            credential hit Firebase/warp-server for a fresh token" gate that
            could never be consulted is gone.

            Acceptance: `check -p warp_server_auth -p warp_server_client
            --all-targets` clean; clippy both crates 0 warnings; nextest both
            crates 11 passed; `check -p warp --lib --all-targets` both
            feature sets, `--bin simplewarp`, `--bin warp-oss`,
            `--all-targets -p integration` clean (0 errors; only the two
            pre-existing `step.rs` unused-import warnings from 4dp);
            format clean.
            Nextest: warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`),
            0 failed (4 skipped both — exactly the 4do baselines, zero tests added
            or removed, no flakes). App not
            re-run — deleted code was unreachable (no callers).
      - [x] **Dead server-sync update shaping + its last query module
            (4dr) — DONE 2026-09-22.** The crate-level cleanup 4dp deferred
            (`ObjectsToUpdate`, then write-only) plus the 4bg orphan it never
            swept back for. `crates/cloud_objects/src/cloud_object/update.rs`
            held exactly two items, both with zero code users repo-wide
            (verified with exact-name `git grep`, excluding only the plan
            ledger, the server `schema.graphql` definition, and binary test
            fixtures): `ObjectsToUpdate` ("all the info needed to fetch
            changed objects from the server" — the sync-queue input shaping
            whose producers died in 4bg and whose last reader 4dp removed)
            and `UpdateCloudObjectResult` (revision-based cloud-update result;
            `git log -S` shows its users went with 4bg's sync-queue
            deletion). Deleted the file with `mod update;` + `pub use
            update::*;`. That left `UpdatedObjectInput` with zero external
            readers, which per the 4cp–4da per-type rule orphans the whole
            `get_updated_cloud_objects` query module (the operation itself was
            never built anywhere — zero builders repo-wide; the module survived
            only as `UpdatedObjectInput`'s supplier, per 4da): deleted the
            module + its `mod.rs` line. `queries/` now holds two modules
            (`get_user`, `get_conversation_usage` — both still type-live via
            `AuthClientImpl`/`gql_convert`/`ai.rs`). Deliberately left: the
            `Folder`/`Notebook`/`Workflow`/`GenericStringObject` fragment types
            the deleted module imported (defined in sibling modules, still
            live), the server `schema.graphql` definitions (wire boundary, not
            client code), and the binary sqlite fixtures that merely contain
            the type names as data. 4 files, +1/−121 (2 deleted). Local-only
            safety: zero code users means zero behavior change — local
            persistence (`CloudModel`/SQLite), local objects, terminal, tabs,
            panes, BYOK AI, settings, themes, and all other local features
            untouched; only the "which objects need server updates" payload
            shaping and the unbuilt server query are gone.

            Acceptance: `check -p cloud_objects --all-targets`,
            `check -p warp_graphql --all-targets`, `check -p warp --lib
            --all-targets` both feature sets, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors;
            only the two pre-existing `step.rs` unused-import warnings);
            clippy `-p warp --lib` 11 needless-returns + 1 single-element loop,
            byte-identical to the baseline (0 added, none in touched files);
            clippy on both touched crates 0 warnings; format clean.
            Nextest: `cloud_objects`+`warp_graphql` 6 passed; warp lib 4,653
            default / 4,652 simplewarp (`--no-fail-fast`), 0 failed (4 skipped
            both — exactly the 4ci baselines, zero tests added or removed, no
            flakes). App not re-run — deleted code was unreachable (no users).
      - [x] **Zero-reader `FeatureFlag::AgentManagementView` + cargo feature
            (4ds) — DONE 2026-09-22.** The 4dc/4dg precedent, third instance:
            `git grep "FeatureFlag::AgentManagementView"` repo-wide returns only
            the definition (`crates/warp_features/src/lib.rs`) — the
            `features.rs` cfg mapping is write-only, no `is_enabled` reader in
            production or tests, no DOGFOOD/PREVIEW/RELEASE membership, and the
            `agent_management_view` cargo feature has no `cfg(feature = ...)`
            reader outside that mapping (verified; the 190-variant sweep finds
            no other zero-reader flag). The flag gated the cloud agent
            management dashboard; the dashboard view went in 4al (server-token
            cloud conversation metadata, server-API task-list calls), leaving
            the variant orphaned. Unlike 4dg the feature was only in the
            `default` set (simplewarp already excludes it), so this round
            deletes the variant + doc (lib.rs), the two-line cfg mapping
            (features.rs), and the default-set membership plus the
            `agent_management_view = []` definition (app/Cargo.toml).
            `FLAG_STATES`/`USER_PREFERENCE_MAP` are cardinality-sized, no count
            to update (per §5). Deliberately left:
            `AgentModeEntrypoint::AgentManagementView` (telemetry entry-origin
            taxonomy — needs its own per-type constructor trace, same class as
            4dm's payload leftovers), `WorkspaceAction::
            OpenAgentManagementView` (4al's kept no-op stub, a cataloged
            local_control/CLI surface), `FeatureFlag::
            InteractiveConversationManagementView` (separate flag with live
            readers in `agent_conversations_model`), the `vertical_tabs.rs`
            comment mentioning the deleted view, and the historical
            `specs/REMOTE-*` hits for the already-deleted surface. 3 files,
            +0/−7. Local-only safety: nothing ever read the flag, so both
            feature sets resolve and behave exactly as before — terminal,
            tabs, panes, local conversation history/persistence, BYOK AI,
            settings, themes, and all other local features untouched.

            Acceptance: `check -p warp_features --all-targets`, `check -p warp
            --lib --all-targets` both feature sets, `--bin simplewarp`, `--bin
            warp-oss` clean (0 errors); clippy `-p warp_features --all-targets`
            clean, `-p warp --lib` 11 needless-returns byte-identical to the
            stash baseline (0 added, none in touched files); format clean.
            Nextest: `-p warp_features` 1 passed / 1 skipped; warp lib 4,653
            default / 4,652 simplewarp (`--no-fail-fast`) — default shows the
            one known 4co cross-test-interference flake
            (`test_command_block_dispatches_event`, passes in isolation),
            simplewarp 0 failed (4 skipped both — exactly the 4ci baselines,
            zero tests added or removed). App not re-run — deleted code was
            unreachable (no readers).
      - [ ] **Next per 4ca's order after 4dt**: orphaned query/mutation/
            subscription modules are done (`mutations/` holds only the live
            `create_anonymous_user`, `queries/` holds only the two
            type-live modules, `subscriptions/` is gone) and the one
            zero-reader flag is gone (4dc), the dead login-notify no-op
            is gone (4dd), the orphaned capacity-modal telemetry events
            are gone (4de), the ten orphaned team/sharing telemetry events
            are gone (4df), the zero-reader NamedAgents flag + cargo
            feature are gone (4dg), the eight orphaned auth/login/signup
            telemetry events + `LoginEventSource` are gone (4dh), and the three
            orphaned billing/revenue telemetry events + two helper enums are
            gone (4di), the eight orphaned unit telemetry events are
            gone (4dj), the orphaned changelog telemetry event is
            gone (4dk), the three orphaned context-menu/conversation-list
            telemetry events are gone (4dl), and the seven orphaned
            payload-carrying telemetry events are gone (4dm), and the
            orphaned tier-limit telemetry event is gone (4dn), and the
            orphaned launch-config telemetry event is gone (4do — the scripted
            346-variant sweep shows zero remaining zero-constructor variants),
            and the dead server-sync version plumbing is gone (4dp —
            `get_versions_for_all_objects` + `CloudObject::versions` +
            `get_latest_processed_at_ts`), and the dead token-refresh gate is
            gone (4dq — `allowed_to_refresh_token` +
            `is_externally_managed`), the dead server-sync update shaping
            is gone (4dr — `ObjectsToUpdate` + `UpdateCloudObjectResult` +
            the `get_updated_cloud_objects` query module they kept alive),
            and the zero-reader AgentManagementView flag + cargo
            feature is gone (4ds), and the orphaned
            `AgentModeEntrypoint::AgentManagementView` telemetry taxonomy
            variant is gone (4dt — 4ds's deferred per-type trace, same class
            as 4dm's payload leftovers), and the three zero-caller
            `AuthState` getters are gone (4du — `user_email_domain` +
            `anonymous_user_renotification_block_expired` +
            `is_api_key_authenticated`, with the orphaned 7-day
            renotification const + `chrono` import).
            Next is the remaining ambient
            task-id identity plumbing as its own multi-round job (local children +
            transcript viewer first — `AmbientAgentTaskId::new()` backs
            local-only orchestrator children and the viewer threading is
            shared with the local viewer, so it is not a leaf deletion),
            then the telemetry scope decision (4ca item 7), then the fold
            (item 8).
      - [x] **Orphaned `AgentModeEntrypoint::AgentManagementView` telemetry
            taxonomy variant (4dt) — DONE 2026-09-22.** The per-type
            constructor trace 4ds deferred: `git grep
            "AgentModeEntrypoint::AgentManagementView"` repo-wide returns zero
            constructors — only the plan.md ledger mentions. The cloud agent
            management dashboard went in 4al (server-token cloud conversation
            metadata, server-API task-list calls), so entering Agent Mode
            from that dashboard can never happen; the variant is remote-only
            by design. Deleted the variant with its doc + serde rename (1
            file, +0/−4). No companion edits: the value is only ever
            serde-serialized into `AgentModeClickedEntrypoint` properties —
            no `match`/`if` on any `AgentModeEntrypoint` value exists
            anywhere, so no arm could name it. Deliberately left:
            `AgentModeEntrypoint::{AICommandSearch, PromptChip,
            AgentManagementPopup}` — the same trace shows zero
            `AgentModeEntrypoint::` constructors for all three as well, but
            they are entry points from local surfaces (command search,
            prompt chip, management popup), not remote-dependent code, so
            each needs its own surface trace before deleting; plus the
            standing non-leaves (`WorkspaceAction::OpenAgentManagementView`
            no-op stub — its local_control reason went in 4bp, needs its own
            action-surface trace — tier-limit banner UI, ambient task-id
            plumbing). Local-only safety: zero constructors means zero
            behavior change — terminal, tabs, panes, BYOK AI, settings,
            themes, and all other local features untouched; only one
            RudderStack entrypoint string that could never be emitted is
            gone.

            Acceptance: `check -p warp --lib --all-targets`, `--bin
            simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
            clean (0 errors; only the two pre-existing `step.rs`
            unused-import warnings that reproduce on a clean stash); clippy
            `-p warp --lib` 11 needless-returns + 1 single-element loop,
            all in untouched files, byte-identical to the baseline (0 added,
            none in the touched file); format clean. Nextest: warp lib 4,653
            default / 4,652 simplewarp (`--no-fail-fast`), 0 failed (4
            skipped both — exactly the 4ci baselines, zero tests added or
            removed, no flakes). App not re-run — deleted code was
            unreachable (no constructors).
      - [x] **Three zero-caller `AuthState` getters (4du) — DONE 2026-09-22.**
            Same leaf class as 4dq (token-refresh gate) and 3v (pub-fn
            sweep): grepped every `pub fn` in
            `crates/warp_server_auth/src/auth_state.rs` repo-wide, flagged
            the three with exactly one hit (the definition, nothing else —
            verified with bare-name `git grep` plus `.method(` call-syntax
            search, all zero outside the definition):
            `user_email_domain` (Firebase email domain from the
            server-fetched `User` metadata — `user` never populates here per
            3n, so always `None`), `anonymous_user_renotification_block_expired`
            (anonymous-signup nudge timer gated on
            `is_anonymous_user_feature_gated`, whose type comes from the
            server's `AnonymousUserType`), and `is_api_key_authenticated`
            (the 4cd `warp api-key` round's leftover — an API key buys
            nothing per 4cd, so the predicate it reported on is dead).
            Deleted the three methods plus the orphaned cascade:
            `ANONYMOUS_USER_NOTIFICATION_BLOCK_TIMER` (7-day const, sole
            user was the renotification method) and the `chrono::{DateTime,
            Duration, Utc}` import (sole users were the const + method —
            `chrono` stays in `Cargo.toml`, still live via `user.rs`).
            Deliberately left: `is_anonymous_user_feature_gated` (live via
            `drive_helpers.rs`), `api_key`/`api_key_owner_type`
            (live getters on the same credentials), and `Credentials::ApiKey`
            itself (still constructed in tests/evals). 1 file, +0/−38.
            Local-only safety: zero callers means zero behavior change —
            login/logout, daemon bearer, cached tokens, terminal, tabs,
            panes, BYOK AI, settings, themes, and all other local features
            untouched; only three predicates that could never be consulted
            are gone.

            Acceptance: `check -p warp_server_auth --all-targets`, `check -p
            warp --lib --all-targets` both feature sets, `--bin simplewarp`,
            `--bin warp-oss`, `--all-targets -p integration` clean (0 errors;
            only the two pre-existing `step.rs` unused-import warnings that
            reproduce on a clean stash); clippy `-p warp_server_auth
            --all-targets` 0 warnings, `-p warp --lib` 11 needless-returns
            byte-identical to the stash baseline (0 added, none in the
            touched file); format clean. Nextest: `warp_server_auth` 4
            passed; `warp_server_client`+`warp_graphql`+`warp_cli` 89 passed;
            warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`), 0
            failed (4 skipped both — exactly the 4dt baselines, zero tests
            added or removed, no flakes). App not re-run — deleted code was
            unreachable (no callers).
      - [x] **Dead server-conflict setter on `GenericCloudObject`
            (4dv) — DONE 2026-09-23.** Same zero-caller leaf class as
            4dp/4dq/4du, found by sweeping every `pub fn` in
            `warp_server_client` / `warp_server_auth` / `cloud_objects` /
            `warp_graphql` repo-wide (candidate 3, the 3v method — 170 `pub fn`
            lines, 132 unique names, 8 single-file hits):
            `GenericCloudObject::set_conflicting_object` had zero callers
            (definition only — verified with bare-name `git grep` plus
            `.set_conflicting_object(` / `::set_conflicting_object(`
            call-syntax search, all zero outside the definition). It marked the
            object as conflicting with a server-provided
            `GenericServerObject`; server objects can never arrive in this fork
            (sync queue gone in 4bg, version/update shaping gone in 4dp/4dr),
            so no producer could ever call it. Deleted the method (1 file,
            +0/−5). Candidates 1 and 2 came back clean first: the re-run
            346-variant `TelemetryEvent` sweep (now 345 variants) and the
            190-variant `FeatureFlag` sweep (now 189) both show zero remaining
            orphans. Deliberately left: `update_from_server_object` (live via
            the persistence ingest path) and everything it constructs
            (`ConflictStatus::ConflictingChanges`, `GenericServerObject` —
            still read in `app/src/cloud_object/mod.rs`); `into_upsert_params`
            (consuming variant of the live `upsert_params` used by the local
            SQLite persistence path — local plumbing, needs its own scope call,
            same class as 4du's deferred `is_anonymous_user_feature_gated`);
            `SharingAccessLevel::{can_trash, can_edit_access,
            to_serializable_value}` + `SyncId::from_object_id` (same
            dual-confirmed zero-caller shape, different files — the next
            slice, not this one); `sqlite_hash` / `new_anonymous_for_test`
            (single-file hits but live — an in-file caller and a wrapper
            caller respectively, caught only by reading the grep output, the
            3z lesson). Local-only safety: zero callers means zero behavior
            change — local persistence (`CloudModel`/SQLite), conflict
            tracking, terminal, tabs, panes, BYOK AI, settings, themes, and all
            other local features untouched; only the setter that could never be
            called is gone.

            Acceptance: `check -p warp --lib --all-targets` both feature sets,
            `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p
            integration`, `check -p cloud_objects --all-targets` clean (0
            errors; only the two pre-existing `step.rs` unused-import
            warnings); clippy `-p warp --lib --all-targets` byte-identical to
            the stash baseline in both configs (11 needless-returns + 1
            single-element loop, 0 added, none in touched files); clippy `-p
            cloud_objects --all-targets` 0 warnings; format clean. Nextest:
            warp lib 4,653 default / 4,652 simplewarp (`--no-fail-fast`), 0
            failed (4 skipped both — exactly the 4ci baselines, zero tests
            added or removed, no flakes). Built `./target/debug/simplewarp`
            and launched it — alive past 50s, no outbound TCP (`lsof -nP
            -iTCP` empty), 0 panics, clean shutdown.
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

No `DEVELOPER_DIR` override is needed (`xcode-select -p` returns
`/Applications/Xcode.app/Contents/Developer`). If the toolchain ever moves, prefix
with `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`.

```sh
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

## Phase 4 cleanup ledger (continued)

- [x] **Three zero-caller `SharingAccessLevel` methods + orphaned `FromStr`
      (4dw) — DONE 2026-09-23.** Slice A from the 4dv handoff (B,
      `SyncId::from_object_id`, untouched). Same zero-caller leaf class as
      4dp/4dq/4du/4dv: `can_trash`, `can_edit_access`, and
      `to_serializable_value` each had exactly one hit repo-wide (the
      definition in `crates/cloud_objects/src/drive/sharing.rs` — verified
      with bare-name `git grep` plus `.method(` call-syntax search, all zero
      outside the definition, plan.md ledger and schema.graphql excluded).
      Single-column trace against the survivor: `can_move_drive` stays live
      via `app/src/drive/index.rs:2304`, confirming the sweep can tell live
      from dead in this impl block. `FromStr` got its separate judgment
      (not a casual scope add): zero `from_str`/`parse::<SharingAccessLevel>`
      callers anywhere, no `.parse()` in `cloud_objects` at all, warp_cli's
      share parsing uses its own `ShareSubject::from_str`, and the impl's
      only purpose was parsing `to_serializable_value` output per its own
      doc-link — real serialization goes through the serde derives. Deleted
      the three methods plus the `FromStr` impl and its now-unused
      `use std::str::FromStr;` import (1 file, +0/−34). Deliberately left:
      `can_delete` (same zero-caller shape, needs its own slice — candidate
      next), `label`/`name` (live UI strings), the `From<AccessLevel>` /
      `From<Role>` conversions (live server/session-mapping paths), and all
      of B (`SyncId::from_object_id` — the other half of the handoff).
      Local-only safety: zero callers means zero behavior change — drive
      sharing levels, trash/move permission gates, terminal, tabs, panes,
      BYOK AI, settings, themes, and all other local features untouched;
      only predicates that could never be consulted and a parser that could
      never run are gone.

      Acceptance: `check -p cloud_objects --all-targets`, `check -p warp
      --lib --all-targets` both feature sets, `--bin simplewarp`, `--bin
      warp-oss`, `--all-targets -p integration` clean (0 errors; only the
      two pre-existing `step.rs` unused-import warnings); clippy `-p warp
      --lib --all-targets` byte-identical to the stash baseline (182 lines
      both, only the build-time trailer differs — 0 added, none in the
      touched file); format clean. Nextest: warp lib 4,652 simplewarp
      (`--no-fail-fast`), 0 failed (4 skipped — exactly the baseline, zero
      tests added or removed); `cloud_objects` has no tests (0 run). Built
      `./target/debug/simplewarp` and launched it — alive past 50s, zero TCP
      sockets (`lsof -nP -a -p <pid> -iTCP` empty), 0 panics, clean
      shutdown.

- [x] **`SharingAccessLevel::can_delete` (4dx) — DONE 2026-09-23.** Slice A
      from the 4dw handoff (B, `SyncId::from_object_id`, untouched). Same
      zero-caller leaf class as 4dp/4dq/4du/4dv/4dw: bare-name `git grep`
      for `can_delete` hit only the definition in
      `crates/cloud_objects/src/drive/sharing.rs` plus unrelated shapes
      (the `agent_conversations_model` `can_delete: bool` capability field
      and its readers, old workflow-migration `user_can_delete` columns,
      `plan.md` ledger mentions), and call-syntax `.can_delete(` search hit
      only the definition — zero callers repo-wide (`plan.md` ledger and
      `schema.graphql` excluded from both). Single-column trace against the
      survivor: `can_move_drive` stays live via
      `app/src/drive/index.rs:2304`, untouched. Deleted the method alone
      (1 file, +0/−5). Deliberately left: `can_move_drive` (live),
      `label`/`name` (live UI strings), the `From<AccessLevel>` /
      `From<Role>` conversions (live server/session-mapping paths), and all
      of B (`SyncId::from_object_id` — the other half of the handoff, next
      candidate). Local-only safety: zero callers means zero behavior
      change — drive sharing levels, trash/move permission gates, terminal,
      tabs, panes, BYOK AI, settings, themes, and all other local features
      untouched; only a predicate that could never be consulted is gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both feature
      sets (default + `--no-default-features --features simplewarp`),
      `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      clean (0 errors); clippy `-p warp --lib --all-targets`
      warning-identical to the stash baseline (same 12 pre-existing
      warnings, none in the touched file — raw outputs differ only in
      `Checking` line order and the build-time trailer); format clean.
      Nextest `-p warp --lib --no-fail-fast`: 4,652 simplewarp / 4,653
      default, 0 failed (4 skipped — exactly the baseline, zero tests added
      or removed). Built `./target/debug/simplewarp` and launched it —
      alive past 50s, zero TCP sockets (`lsof -nP -a -p <pid> -iTCP`
      empty), 0 panics, clean shutdown.

- [x] **`SyncId::from_object_id` (4dy) — DONE 2026-09-23.** Slice B
      from the 4dv/4dw handoff (A, `SharingAccessLevel::can_delete`,
      done in 4dx). Same zero-caller leaf class as
      4dp/4dq/4du/4dv/4dw/4dx: bare-name `git grep` for `from_object_id`
      hit only the definition in `crates/cloud_objects/src/ids.rs:74`
      plus `plan.md` ledger mentions, and call-syntax `.from_object_id(`
      search hit zero while `::from_object_id` hit only the ledger —
      zero live callers repo-wide (`plan.md` ledger, `schema.graphql`,
      binary fixtures excluded from both). Deleted the method alone
      (1 file, +0/−7). Deliberately left: the `ToServerId` trait and
      `to_server_id()` (live — generic bounds in
      `app/src/cloud_object/mod.rs`, `model/persistence.rs`,
      `server/cloud_objects/update_manager.rs`,
      `notebooks/editor/embedded_item.rs`, and
      `crates/cloud_object_persistence/src/objects.rs:528`),
      `SyncId::uid` / `sqlite_uid_hash` / `into_server` / `into_client`
      (live), the `From<ServerId>` / `From<FolderId>` /
      `From<GenericStringObjectId>` conversions (live), and all
      SharingAccessLevel survivors. Local-only safety: zero callers
      means zero behavior change — sync IDs, drive, terminal, tabs,
      panes, BYOK AI, settings, themes, and all other local features
      untouched; only a constructor that could never be called is gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both feature
      sets (default + `--no-default-features --features simplewarp`),
      `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      clean (0 errors; only the two pre-existing `step.rs`
      unused-import warnings); clippy `-p warp --lib --all-targets`
      warning-identical to the stash baseline (same 12 pre-existing
      warnings — 11 unneeded-return + 1 single-element-loop, none in the
      touched file — raw outputs differ only in the `mcp` `Checking`
      line order and the build-time trailer); format clean. Nextest
      `-p warp --lib --no-fail-fast`: 4,652 simplewarp / 4,653 default,
      0 failed (4 skipped — exactly the baseline, zero tests added or
      removed). Built `./target/debug/simplewarp` and launched it —
      alive 67s, zero TCP sockets (`lsof -nP -a -p <pid> -iTCP` empty),
      0 panics, clean shutdown.

- [x] **`Subject::from_owner` (4dz) — DONE 2026-09-23.** Next
      zero-caller leaf in `crates/cloud_objects` after 4dy
      (`SyncId::from_object_id`). Same zero-caller leaf class as
      4dp/4dq/4du/4dv/4dw/4dx/4dy: bare-name `git grep -n "from_owner"`
      hit only the definition in
      `crates/cloud_objects/src/drive/sharing.rs:133` plus two unrelated
      `pane_group` variable lines (`from_owner_lookup` substring, no call),
      and call-syntax `from_owner(` hit only the definition while
      `Subject::from_owner` / `::from_owner` hit zero — zero live callers
      repo-wide (`plan.md` ledger zero hits, `schema.graphql` zero,
      fixtures zero; `.rs` scope for both searches). Single-column trace
      against the survivors: `can_move_drive` stays live via
      `app/src/drive/index.rs:2304`, `is_user` stays live via
      `crates/cloud_objects/src/cloud_object/mod.rs:483` plus
      `app/src/cloud_object/model/view.rs:180`, confirming the sweep can
      tell live from dead in this module. Deleted the method plus its
      now-unused `use crate::cloud_object::Owner;` import (1 file,
      +0/−9). Deliberately left: `Subject::user_uid` /
      `Subject::team_uid` (same zero-caller shape — `.user_uid(` zero,
      `.team_uid()` only the internal `TeamKind::team_uid` call — each
      needs its own slice, candidates next), `label` / `name` (generic
      names, receiver-typed judgment deferred — no `access_level.label()`
      / `access_level.name()` callers found but bare-name noise is high),
      `can_move_drive` / `is_user` (live), `into_upsert_params`
      (zero callers but consuming-variant scope judgment per handoff —
      trace-only, not deleted), and all `ids.rs` / `drive/mod.rs`
      survivors (`into_server` / `into_client` / `sqlite_hash` via
      `sqlite_uid_hash` / `from_string_lossy` /
      `sqlite_type_and_uid_hash` / `from_id_and_type` / `as_folder_id` /
      `as_notebook_id` / `has_server_id` etc. all live). Local-only
      safety: zero callers means zero behavior change — drive sharing
      subjects, permission gates, sync IDs, terminal, tabs, panes, BYOK
      AI, settings, themes, and all other local features untouched; only
      an `Owner`-to-`Subject` constructor that could never be called is
      gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both feature
      sets (default + `--no-default-features --features simplewarp`),
      `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      clean (0 errors; only the two pre-existing `step.rs`
      unused-import warnings); clippy `-p warp --lib --all-targets`
      warning-identical to the stash baseline (182 lines both, 14
      `^warning` lines both, same 12 pre-existing warnings — none in the
      touched file — raw outputs differ only in the build-time trailer);
      format clean. Nextest `-p warp --lib --no-fail-fast`: 4,652
      simplewarp / 4,653 default, 0 failed (4 skipped — exactly the
      baseline, zero tests added or removed). Built
      `./target/debug/simplewarp` and launched it — alive 76s, zero TCP
      sockets (`lsof -nP -a -p <pid> -iTCP` empty), 0 panics, clean
      shutdown.

- [x] **`Subject::user_uid` (4ea) — DONE 2026-09-23.** Next
      zero-caller leaf in `crates/cloud_objects` after 4dz
      (`Subject::from_owner`). Same zero-caller leaf class as
      4dp/4dq/4du/4dv/4dw/4dx/4dy/4dz: call-syntax `.user_uid(`
      zero in `*.rs` (only hit repo-wide is the `plan.md:6787`
      ledger mention), `user_uid()` zero in `*.rs`,
      `Subject::user_uid` zero except the `plan.md:6786` ledger
      line, and `::user_uid` only the three module re-exports
      (`crates/cloud_objects/src/auth/mod.rs:2`,
      `crates/warp_server_auth/src/user.rs:8`,
      `crates/warp_server_client/src/auth/mod.rs:19`) plus the
      plan ledger — no method path. Bare `.user_uid` in `*.rs`
      only four field hits, none with a `Subject` receiver:
      `app/src/persistence/sqlite.rs:2741` (`row.user_uid` DB
      row field) plus `crates/ai/src/api_keys_tests.rs:581,650,673`
      (`gate`/`mismatched.user_uid` struct-field assignments).
      Bare-name `git grep -n "user_uid"` (98 hits) ledger-excluded
      class by class, none a caller: `Owner::User { user_uid }`
      destructures (`app/src/drive/items/item.rs:427`,
      `app/src/workspace/view.rs:3738`,
      `app/src/workspaces/user_workspaces.rs:646`,
      `crates/cloud_objects/src/cloud_object/mod.rs:1008`,
      `crates/cloud_object_persistence/src/objects.rs:155,604`),
      `Owner`/`GeapMintBinding`/profile/persistence struct field
      decls and inits (`crates/ai/src/geap_credentials.rs:104`,
      `app/src/ai/geap_credentials.rs:38`,
      `app/src/auth/auth_view_modal.rs:20`,
      `crates/cloud_object_models/src/user_profile.rs:38`,
      `crates/cloud_objects/src/cloud_object/mod.rs:320,332,997`,
      `crates/persistence/src/model.rs:128,137`,
      `crates/persistence/src/schema.rs:384`,
      `crates/persistence/migrations/.../up.sql:4`,
      `app/src/persistence/sqlite.rs:1947,2091`),
      `let user_uid = ...user_id()` locals and their closures
      (`app/src/cloud_object/model/view.rs:91,95,177-191`,
      `app/src/drive/index.rs:664,669,1776,1785`,
      `app/src/persistence/sqlite.rs:190-191`,
      `app/src/ai/agent_sdk/admin.rs:99-128`,
      `app/src/workspace/view.rs:3733-3735`,
      `app/src/ai/agent_conversations_model/entry.rs:267`),
      auth-redirect payload plumbing (`app/src/auth/auth_manager.rs`,
      `auth_manager_tests.rs`, `auth_view_modal.rs:12,37,43`,
      `app/src/auth/mod.rs:10,14`, `crates/warp_server_auth/src/user_uid.rs`,
      `crates/cloud_objects/src/auth/mod.rs:1`), `user_uid` module
      re-exports, skill/TECH docs
      (`.agents/.../SKILL.md:232,242`, `specs/REV-1599/TECH.md:65`),
      fixture data (`windows_tests.rs:15` JSON `test_user_uid`
      string), and the `plan.md:6786-6787` ledger lines;
      `schema.graphql` zero, `*.json` fixtures zero. In-module
      `UserKind::Account(user_uid)` patterns at
      `sharing.rs:135` (inside the deleted method) and `:149`
      (inside the live `is_user`) are destructures, not callers.
      Single-column trace against the survivors: `is_user` stays
      live via `crates/cloud_objects/src/cloud_object/mod.rs:483`
      plus `app/src/cloud_object/model/view.rs:180`, confirming
      the sweep can tell live from dead in this module. Deleted
      the method only (`use crate::auth::UserUid;` stays — still
      used by `is_user` and `UserKind::Account`) (1 file, +0/−15).
      Deliberately left: `Subject::team_uid` (same zero-caller
      shape — needs its own slice, candidate next per handoff),
      `label` / `name` (generic names, receiver-typed judgment
      deferred), `can_move_drive` / `is_user` (live),
      `into_upsert_params` (zero callers but consuming-variant
      scope judgment per handoff — trace-only, not deleted), and
      all `ids.rs` / `drive/mod.rs` survivors. Local-only safety:
      zero callers means zero behavior change — drive sharing
      subjects, permission gates, terminal, tabs, panes, BYOK AI,
      settings, themes, and all other local features untouched;
      only a `Subject`-to-`Option<UserUid>` getter that could never
      be called is gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both feature
      sets (default + `--no-default-features --features simplewarp`),
      `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      clean (0 errors; only the two pre-existing `step.rs`
      unused-import warnings); clippy `-p warp --lib --all-targets`
      warning-identical to the stash baseline (182 lines both, 14
      `^warning` lines both, same 12 pre-existing warnings — none in the
      touched file — raw outputs differ only in the build-time trailer);
      format clean. Nextest `-p warp --lib --no-fail-fast`: 4,652
      simplewarp / 4,653 default, 0 failed (4 skipped — exactly the
      baseline, zero tests added or removed). Built
      `./target/debug/simplewarp` and launched it — alive 70s, zero TCP
      sockets (`lsof -nP -a -p <pid> -iTCP` empty), 0 panics, clean
      shutdown.

- [x] **`Subject::team_uid` (4eb) — DONE 2026-09-23.** Next
      zero-caller leaf in `crates/cloud_objects` after 4ea
      (`Subject::user_uid`). Same zero-caller leaf class as
      4dp/4dq/4du/4dv/4dw/4dx/4dy/4dz/4ea: dual-confirm (bare-name +
      call-syntax, ledger / `schema.graphql` / fixture / field-decl
      excluded). Call-syntax `.team_uid()` zero-arg form exactly one
      hit repo-wide in `*.rs` — the internal `team_kind.team_uid()`
      at `sharing.rs:145` (`TeamKind` receiver, inside the deleted
      method itself); zero `Subject`-receiver callers.
      `Subject::team_uid` path zero in `*.rs` (only the
      `plan.md:6787,6879` ledger lines); `::team_uid` zero in `*.rs`
      (no re-export confusion, unlike `user_uid`). `.team_uid(` with
      args only the unrelated `self.team_uid(ctx/app)` view methods
      (`app/src/root_view.rs:1415`,
      `app/src/workspace/view.rs:20567,20627,20647,20676,20707,20736,21676,22274`)
      and `workspace_setting.team_uid(ctx)` — all take `ctx`/`app`,
      different methods. Bare `.team_uid` field hits
      (`window.team_uid`, `metadata.team_uid`, `invite.team_uid`,
      `team_uid_for_window`) none with a `Subject` receiver
      (`Subject` is an enum — no field access possible).
      `TeamKind::Team { team_uid }` / `SharedSessionTeam { team_uid,
      .. }` destructures at `sharing.rs:124-125` sit inside the live
      `TeamKind::team_uid`, not callers. Bare-name `git grep team_uid`
      (267 hits) class by class, none a caller: `Owner::Team` /
      `Space::Team` destructures and inits, `UserWorkspaces`
      (`team_uid_for_window`, `sole_team_uid`, `team_from_uid`,
      `inherited_or_default_team_uid`), window/telemetry/persistence
      struct fields, GraphQL/QA types, migrations/schema, tests, docs,
      and the plan ledger; `schema.graphql` zero, `*.json` fixtures
      zero. Single-column trace against the survivors: `is_user` stays
      live via `crates/cloud_objects/src/cloud_object/mod.rs:483` plus
      `app/src/cloud_object/model/view.rs:180`, confirming the sweep
      can tell live from dead in this module. Deleted the method only
      (both imports stay — `ServerId` still used by `TeamKind` at
      `sharing.rs:111,115,122`, `UserUid` still used by `is_user`) (1
      file, +0/−8). Deliberately left: `TeamKind::team_uid` (its only
      caller was the deleted method, so newly callerless with
      `TeamKind` used nowhere outside `sharing.rs` — same zero-caller
      shape, needs its own dual-confirm slice, candidate next),
      `label` / `name` (generic names, receiver-typed judgment
      deferred), `can_move_drive` / `is_user` (live),
      `into_upsert_params` (zero callers but consuming-variant scope
      judgment per handoff — trace-only, not deleted), and all
      `ids.rs` / `drive/mod.rs` survivors. Local-only safety: zero
      callers means zero behavior change — drive sharing subjects,
      permission gates, terminal, tabs, panes, BYOK AI, settings,
      themes, and all other local features untouched; only a
      `Subject`-to-`Option<ServerId>` getter that could never be called
      is gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both feature
      sets (default + `--no-default-features --features simplewarp`),
      `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      clean (0 errors; only the two pre-existing `step.rs`
      unused-import warnings); clippy `-p warp --lib --all-targets`
      warning-identical to the stash baseline (182 lines both, 14
      `^warning` lines both, same 12 pre-existing warnings — none in the
      touched file — raw outputs differ only in build nondeterminism:
      `Finished` time trailer plus one parallel `Checking mcp` line
      reorder); format clean. Nextest `-p warp --lib --no-fail-fast`:
      4,652 simplewarp / 4,653 default, 0 failed (4 skipped — exactly the
      baseline, zero tests added or removed). Built
      `./target/debug/simplewarp` and launched it — alive 60s+, zero TCP
      sockets (`lsof -nP -a -p <pid> -iTCP` empty), 0 panics, clean
      shutdown.

- [x] **`TeamKind::team_uid` (4ec) — DONE 2026-09-23.** Next
      zero-caller leaf in `crates/cloud_objects` after 4eb
      (`Subject::team_uid`, whose sole internal caller was this
      method). Same zero-caller leaf class as
      4dp/4dq/4du/4dv/4dw/4dx/4dy/4dz/4ea/4eb: independent
      dual-confirm (bare-name + call-syntax, ledger /
      `schema.graphql` / fixture / field-decl excluded). Call-syntax
      `.team_uid()` zero-arg form zero hits repo-wide in `*.rs`
      (the 4eb-era single hit at `sharing.rs:145` is gone with the
      deleted method); `::team_uid` zero in `*.rs` (no re-export
      confusion); `TeamKind::team_uid` / `Subject::team_uid` paths
      zero in `*.rs` (only `plan.md` ledger lines). `.team_uid(`
      with args only the unrelated `self.team_uid(ctx/app)` view
      methods (`app/src/root_view.rs:1415`,
      `app/src/workspace/view.rs:20567,20627,20647,20676,20707,20736,21676,22274`)
      — all take `ctx`/`app`, different methods. `TeamKind` refs
      repo-wide only `sharing.rs:91` (`Subject::Team` payload),
      `:109` (enum def), and the deleted `:120-128` impl (whose
      `:124-125` destructures sit inside the deleted method itself,
      not callers) — used nowhere outside `sharing.rs`. Bare-name
      `team_uid` hits class by class, none a caller: `Owner::Team`
      / `Space::Team` destructures and inits, `UserWorkspaces`
      (`team_uid_for_window`, `sole_team_uid`, `team_from_uid`,
      `inherited_or_default_team_uid`), window/telemetry/persistence
      struct fields, GraphQL/QA types, migrations/schema, tests, docs,
      and the plan ledger; `schema.graphql` zero, `*.json` fixtures
      zero. Deleted the `impl TeamKind` block only (both imports
      stay — `ServerId` still used by the `TeamKind` fields at
      `sharing.rs:111,115`, `UserUid` still used by `is_user`) (1
      file, +0/−10). Deliberately left: `TeamKind` enum itself (now
      newly candidate — payload of `Subject::Team` only — needs its
      own `Subject::Team` construction-site trace slice, candidate
      next, never in this round), `label` / `name` (generic names,
      receiver-typed judgment deferred), `can_move_drive` / `is_user`
      (live), `into_upsert_params` (zero callers but
      consuming-variant scope judgment per handoff — trace-only, not
      deleted), and all `ids.rs` / `drive/mod.rs` survivors.
      Local-only safety: zero callers means zero behavior change —
      drive sharing subjects, permission gates, terminal, tabs, panes,
      BYOK AI, settings, themes, and all other local features
      untouched; only a `TeamKind`-to-`ServerId` getter that could
      never be called is gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both feature
      sets (default + `--no-default-features --features simplewarp`),
      `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      clean (0 errors; only the two pre-existing `step.rs`
      unused-import warnings); clippy `-p warp --lib --all-targets`
      warning-identical to the stash baseline (182 lines both, 14
      `^warning` lines both, same 12 pre-existing warnings — none in the
      touched file — raw outputs differ only in the build-time
      `Finished` trailer); format clean. Nextest `-p warp --lib
      --no-fail-fast`: 4,652 simplewarp / 4,653 default, 0 failed (4
      skipped — exactly the baseline, zero tests added or removed).
      Built `./target/debug/simplewarp` and launched it — alive 60s+,
      zero TCP sockets (`lsof -nP -a -p <pid> -iTCP` empty), 0 panics,
      clean shutdown.

- [x] **`TeamKind` enum itself (4ed) — DONE 2026-09-23.** The
      4ec-designated next slice: `TeamKind` was payload of
      `Subject::Team` only. Independent dual-confirm
      `Subject::Team` construction-site trace (bare-name +
      call-syntax, ledger / `schema.graphql` / fixture / field-decl
      excluded). Exact `git grep -P '\bSubject::Team\b' -- '*.rs'`
      zero hits; call-syntax `\bSubject::Team\s*\(` zero hits.
      Naive `Subject::Team` substring hits are only different enums
      (`GuestSubject::TeamGuest` at
      `cloud_object/mod.rs:945-946` + `ServerGuestSubject::Team`,
      `ShareSubject::Team` in `warp_cli/share.rs:38,143` and
      `share_tests.rs:16,23,30`) — excluded by word boundary, no
      confusion. Exact `\bTeamKind\b` only `sharing.rs:91`
      (`Team(TeamKind)` payload field-decl) and `:109` (enum def);
      `\bTeamKind::` zero; `SharedSessionTeam` only `:114` inside
      the def itself. Bare `\bSubject\b` only the import + field
      type (`cloud_object/mod.rs:23,503`), the def/impl
      (`sharing.rs:82,85,120`), and the two `Subject::User`
      destructures inside live `is_user` (`:124-125`) — plus
      unrelated English ("Subject:" / "Subject to"). No
      `use Subject::*` glob. `schema.graphql` zero, `*.json`
      fixtures zero. So zero external construction — deletable.
      Deleted the `TeamKind` enum + its doc comment and the
      `Subject::Team(TeamKind)` variant (the sole user), plus the
      now-unused `ServerId` import (`UserUid` stays — still used by
      `UserKind::Account` + `is_user`) (1 file, +0/−16).
      Deliberately left: `Subject` itself (`User` / `PendingUser` /
      `AnyoneWithLink` — `is_user` live via
      `app/src/cloud_object/model/view.rs:180` and
      `cloud_object/mod.rs:483`, `has_direct_user_access` live via
      `app/src/drive/index.rs:670,1786`), `label` / `name`
      (generic names, deferred), `can_move_drive` / `is_user`
      (live), `into_upsert_params` (trace-only per handoff),
      `Owner::Team` / `Space::Team` / `ServerGuestSubject::Team` /
      `ShareSubject::Team` (different enums, out of scope), all
      `ids.rs` / `drive/mod.rs` survivors, ambient plumbing,
      telemetry scope (4ca item7), fold (item8), redesigns, and all
      local features. Removal-safe: the only `Subject` match is
      `is_user` with `_ => false` wildcard — no exhaustive match to
      fix; `CloudObjectGuest.guests` stays `Vec` (always empty in
      `new_from_server` / mocks, never constructed — not in scope).
      Local-only safety: zero constructions means zero behavior
      change — drive sharing subjects, permission gates, terminal,
      tabs, panes, BYOK AI, settings, themes untouched; only an
      unconstructible team-subject variant and its payload type are
      gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both feature
      sets (default + `--no-default-features --features simplewarp`),
      `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      clean (0 errors; only the two pre-existing `step.rs`
      unused-import warnings); clippy `-p warp --lib --all-targets`
      warning-identical to the stash baseline (182 lines both, 14
      `^warning` lines both, same 12 pre-existing warnings — none in the
      touched file — raw outputs differ only in the build-time
      `Finished` trailer); format clean (`./script/format` no diff).
      Nextest `-p warp --lib --no-fail-fast`: 4,652 simplewarp passed /
      4,653 default passed, 4 skipped each (zero tests added or
      removed; first default run showed 1 flaky FAIL
      `notebooks::notebook::tests::test_command_block_dispatches_event`
      which passes in isolation and on full rerun — unrelated to this
      change). Built `./target/debug/simplewarp` and launched it —
      alive 56s, zero TCP sockets (`lsof -nP -a -p <pid> -iTCP` empty),
      0 panics, clean shutdown (SIGTERM). Did not `cargo clean`.

- [x] **`Subject::PendingUser` (4ee) — DONE 2026-09-23.** The
      4ed-designated next slice: `Subject::PendingUser` carried the
      same `#[allow(dead_code)]` the 4ed round named as the
      zero-construction candidate marker. Independent dual-confirm
      `Subject::PendingUser` construction-site trace (bare-name +
      call-syntax, ledger / `schema.graphql` / fixture / field-decl /
      different-enum excluded). Exact `git grep -P
      '\bSubject::PendingUser\b' -- '*.rs'` zero hits; call-syntax
      `\bSubject::PendingUser\s*\(` zero hits. Fixed-string
      `Subject::PendingUser` hits in `*.rs` are only
      `crates/cloud_objects/src/cloud_object/mod.rs:942-943`
      (`GuestSubject::PendingUserGuest` match arm +
      `ServerGuestSubject::PendingUser { email }` construction) —
      both different enums, excluded by word boundary: `git grep -P
      '(?<![A-Za-z0-9_])Subject::PendingUser(?![A-Za-z0-9_])' --
      '*.rs'` zero hits, proving the substring hits are
      `Guest`/`ServerGuest` prefixes, not this type. Bare
      `\bPendingUser\s*\{` hits are only the definition in
      `sharing.rs:87`, the `ServerGuestSubject::PendingUser` def at
      `cloud_object/mod.rs:364`, and its live construction at
      `:943` — no `Subject` construction. All-files
      `Subject::PendingUser` (incl. `plan.md`) is only those two
      `mod.rs` lines; `schema.graphql` holds only the wire
      `PendingUserGuest` union member
      (`warp_graphql_schema/api/schema.graphql:1827,2599`), `*.json`
      fixtures zero. `Subject::` match arms repo-wide are only the
      live `is_user` (`Subject::User(..)` + `_ => false` wildcard)
      plus unrelated enums (`AgentConversationNavigationSubject`,
      `ShareSubject`, `GuestSubject`/`ServerGuestSubject`) — the
      same removal-safe wildcard shape as 4ed, no exhaustive match
      to fix. No `use Subject::*` glob. Single-column trace
      against the survivors: `is_user` stays live via
      `crates/cloud_objects/src/cloud_object/mod.rs:483` plus
      `app/src/cloud_object/model/view.rs:180`, `can_move_drive`
      stays live via `app/src/drive/index.rs:2304`, confirming the
      sweep can tell live from dead in this module. Deleted the
      `#[allow(dead_code)] PendingUser { email: Option<String> }`
      variant only (both imports stay — `UserUid` still used by
      `UserKind::Account` + `is_user`) (1 file, +0/−4).
      Deliberately left: `Subject` itself (`User` /
      `AnyoneWithLink` — both live via `is_user` /
      `has_direct_user_access`), `ServerGuestSubject::PendingUser`
      (live, different enum — constructed at `mod.rs:943`),
      `GuestSubject::PendingUserGuest` (wire), `label` / `name`
      (generic names, deferred), `can_move_drive` / `is_user`
      (live), `into_upsert_params` (trace-only per handoff —
      consuming-variant scope judgment, never in this round),
      `Owner::Team` / `Space::Team` / `ServerGuestSubject::Team` /
      `ShareSubject::Team` (different enums, out of scope), all
      `ids.rs` / `drive/mod.rs` survivors, ambient plumbing,
      telemetry scope (4ca item7), fold (item8), redesigns, and all
      local features. Local-only safety: zero constructions means
      zero behavior change — drive sharing subjects, permission
      gates, terminal, tabs, panes, BYOK AI, settings, themes
      untouched; only an unconstructible pending-user variant is
      gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both feature
      sets (default + `--no-default-features --features simplewarp`),
      `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      clean (0 errors; only the two pre-existing `step.rs`
      unused-import warnings); clippy `-p warp --lib --all-targets`
      byte-identical to the stash baseline in both configs (182 lines
      both, 14 `^warning` lines both, same 12 pre-existing warnings —
      11 unneeded-return + 1 single-element-loop, none in the
      touched file — raw outputs differ only in the build-time
      `Finished` trailer); format clean (`./script/format` no diff).
      Nextest `-p warp --lib --no-fail-fast`: 4,652 simplewarp passed /
      4,653 default passed, 4 skipped each (zero tests added or
      removed, no flakes). Built `./target/debug/simplewarp` and
      launched it — alive 71s, zero TCP sockets (`lsof -nP -a -p <pid>
      -iTCP` empty), 0 panics, clean shutdown (SIGTERM). Did not
      `cargo clean`.

- [x] **`SharingAccessLevel::label` (4ef) — DONE 2026-09-23.** The
      4ee-designated next slice: `label` / `name` share
      `crates/cloud_objects/src/drive/sharing.rs:14` but bare-name
      noise is high, so receiver-typed dual-confirm (call-syntax +
      receiver + fully-qualified, ledger / `schema.graphql` /
      fixture / different-type excluded). Fully-qualified
      `SharingAccessLevel::label` zero in `*.rs`,
      `SharingAccessLevel::name` zero; `::label` only the three
      unrelated paths (`PhenomenonStyle::label_text`,
      `ReviewTerminalUnavailableReason::label`), `::name` only
      experiments / style / doc paths — none this type.
      Call-syntax `.label(` (69 hits) triaged class by class, none
      with a `SharingAccessLevel` receiver: `MenuItemFields`,
      `BlockType` / `block_type`, `header_size`, dropdown /
      `DropdownItem`, `AskUserQuestionPermission`
      (`permission.label()` at
      `ask_user_question_view.rs:330,354` — different type),
      `prompt_suggestion`, breadcrumb, `element` / `self_fields` /
      `item.fields()`, `ui_builder.label("...")` builders, and
      `terminal/view_tests` + `workspace/view_tests` assertions —
      zero `access` / `sharing` receivers. Call-syntax `.name(`
      (218 hits) likewise none with an access/sharing receiver
      (`grep -i access|shar` zero); variant-name
      `access_level|sharing_level|level|perm|access` + `.name(`
      zero. Receiver-typed `access_level.label(`/`.name(` zero,
      `SharingAccessLevel` var + `.label`/`.name` zero,
      `level.label`/`.name` zero. In-`cloud_objects` `.label(` /
      `.name(` zero; each `SharingAccessLevel`-importing file
      (`env_vars/active_env_var_collection_data.rs`,
      `env_vars/view/fixed_view_components.rs`,
      `active_notebook_data.rs`, `cloud_object/model/view.rs`,
      `cloud_objects/cloud_object/mod.rs`, `sharing/mod.rs`)
      zero `.label(`/`.name(`. Literal `"Can view"` / `"Can
      edit"` / `"Full access"` only the definitions themselves —
      no hardcoded UI copy. `fn label(` / `fn name(` defs
      elsewhere are all different types. Single-column trace
      against the survivors: `can_move_drive` stays live via
      `app/src/drive/index.rs:2304`, confirming the sweep can tell
      live from dead in this module. Deleted `label` alone (1
      file, +0/−8) — never both at once per handoff. Deliberately
      left: `name` (same zero-caller shape — needs its own slice,
      candidate next), `can_move_drive` / `is_user` (live),
      `into_upsert_params` (trace-only per handoff —
      consuming-variant scope judgment, never in this round), all
      `ids.rs` / `drive/mod.rs` survivors, ambient plumbing,
      telemetry scope (4ca item7), fold (item8), redesigns, and all
      local features. Local-only safety: zero callers means zero
      behavior change — drive sharing levels, permission gates,
      terminal, tabs, panes, BYOK AI, settings, themes untouched;
      only a UI string getter that could never be called is gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both feature
      sets (default + `--no-default-features --features simplewarp`),
      `--bin simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      clean (0 errors; only the two pre-existing `step.rs`
      unused-import warnings); clippy `-p warp --lib --all-targets`
      byte-identical to the stash baseline (182 lines both, 14
      `^warning` lines both, same 12 pre-existing warnings — 11
      unneeded-return + 1 single-element-loop, none in the touched
      file — raw outputs differ only in the build-time `Finished`
      trailer); format clean (`./script/format` no diff). Nextest
      `-p warp --lib --no-fail-fast`: 4,652 simplewarp passed /
      4,653 default passed, 4 skipped each (zero tests added or
      removed, no flakes). Built `./target/debug/simplewarp` and
      launched it — alive 66s+, zero TCP sockets (`lsof -nP -a -p
      <pid> -iTCP` empty), 0 panics, clean shutdown (SIGTERM). Did
      not `cargo clean`.

- [x] **`SharingAccessLevel::name` (4eg) — DONE 2026-09-23.** The
      4ef-designated next slice: `label` / `name` share
      `crates/cloud_objects/src/drive/sharing.rs`, and `name` got
      its own receiver-typed dual-confirm (call-syntax + receiver +
      fully-qualified, ledger / `schema.graphql` / fixture /
      different-type excluded). Fully-qualified
      `SharingAccessLevel::name` zero in `*.rs` (worktree and
      HEAD); only repo-wide hit is the `plan.md:7182` ledger
      mention (the 4ef entry itself); `schema.graphql` zero,
      `*.json` fixtures zero. `::name` path-form (28 hits) all
      different types: experiments `Self::name()` /
      `FooExperiment::name()` (`app/src/experiments/mod.rs`,
      `mod_tests.rs`), `styles::name_font_size` (external-secrets /
      notebook-embedding search items), doc-links
      (`ReadMCPResource::name`, `Signature::name`), and
      `sysinfo::System::name()`. Call-syntax `.name(` (218 hits)
      triaged class by class, none with a `SharingAccessLevel` /
      access / sharing / level-typed receiver: MCP server/tool
      names, shell-type names, workflow-data names, space names
      (`space.name(app)`), menu-item names, experiment-layer
      names, theme names, regex-capture names, thread-builder
      `.name("...")` setters, integration-driver/test names, and
      telemetry event-name fields — `grep -i access|shar|level|perm`
      over the 218 zero. Receiver-typed
      `(access_level|access|sharing|sharing_level|level|perm)`
      + `.name(` zero in `*.rs`; each `SharingAccessLevel`-importing
      file zero `.name(` (`app/src/cloud_object/model/view.rs`,
      `env_vars/active_env_var_collection_data.rs`,
      `env_vars/view/fixed_view_components.rs`,
      `active_notebook_data.rs`, `app/src/sharing/mod.rs`,
      `cloud_objects/cloud_object/mod.rs`, and the `sharing.rs`
      module itself — the in-module `access_level`
      locals/params/fields at `view.rs:170,193`, `sharing.rs:52`,
      `cloud_object/mod.rs:495,504` never call it). Variant-literal
      receivers `SharingAccessLevel::(View|Edit|Full).` zero — no
      inline `SharingAccessLevel::View.name()` forms. Serde note:
      the enum's derives serialize variant names directly
      (`View`/`Edit`/`Full`) and never consult `name`, whose
      lowercase output differs — no wire path touched.
      Single-column trace against the survivor: `can_move_drive`
      stays live via `app/src/drive/index.rs:2304`, confirming the
      sweep can tell live from dead in this module. Deleted `name`
      alone (1 file, +0/−8) — never both at once per handoff.
      Deliberately left: `can_move_drive` / `is_user` (live),
      `into_upsert_params` (designated next, 4eh — the deferred
      consuming-variant scope judgment is tractable: bare-name grep
      hits only the definition at
      `generic_cloud_object.rs:177`, it is an inherent
      `GenericCloudObject` method in no trait, and the borrowing
      sibling `upsert_params` is the live path via
      `app/src/cloud_object/mod.rs:677` +
      `model/persistence.rs:1752`, so the consuming variant is a
      dead `Arc::try_unwrap`-or-clone optimization that could
      never run), all `ids.rs` / `drive/mod.rs` survivors, ambient
      plumbing, telemetry scope (4ca item7), fold (item8),
      redesigns, and all local features. Local-only safety: zero
      callers means zero behavior change — drive sharing levels,
      permission gates, terminal, tabs, panes, BYOK AI, settings,
      themes untouched; only a name-string getter that could never
      be called is gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both
      feature sets (default + `--no-default-features --features
      simplewarp`), `--bin simplewarp`, `--bin warp-oss`,
      `--all-targets -p integration` clean (0 errors; only the two
      pre-existing `step.rs` unused-import warnings at
      `app/src/integration_testing/input/step.rs:11,13`, observed
      in the integration check); clippy `-p warp --lib
      --all-targets` warning-identical to the stash baseline in
      BOTH configs (182 lines both, 14 `^warning` lines both, same
      12 pre-existing warnings — 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`, none in the
      touched file — sorted warning+location pairs identical, raw
      outputs differ only in one `Checking warp_server_client`
      line reorder and the build-time `Finished` trailer); format
      clean (`./script/format` no diff). Nextest `-p warp --lib
      --no-fail-fast`: 4,652 simplewarp passed / 4,653 default
      passed, 4 skipped each (zero tests added or removed, no
      flakes). Built `./target/debug/simplewarp` and launched it —
      alive 80s, zero TCP sockets (`lsof -nP -a -p <pid> -iTCP`
      empty), 0 panics, clean shutdown (SIGTERM). Did not
      `cargo clean`.

- [x] **`GenericCloudObject::into_upsert_params` (4eh) — DONE
      2026-09-23.** The 4eg-designated next slice: the deferred
      consuming-variant scope judgment, now done. Dual-confirm
      (bare-name + call-syntax + path-form, ledger /
      `schema.graphql` / fixture excluded): bare-name
      `git grep -n "into_upsert_params" -- '*.rs'` exactly one
      hit repo-wide — the definition at
      `generic_cloud_object.rs:177`; call-syntax
      `.into_upsert_params(` zero; path-form
      `::into_upsert_params` zero; all-files repo-wide grep only
      that definition plus `plan.md` ledger mentions;
      `schema.graphql` zero, `*.json` fixtures zero. It is an
      inherent method in `impl<K, M> GenericCloudObject<K, M>`
      (declared in no trait), so no generic dispatch can reach
      it. Consumer-shape trace: the borrowing sibling
      `upsert_params` is the live path at exactly two sites —
      `app/src/cloud_object/mod.rs:677`
      (`M::upsert_event(self.upsert_params(self.object_type()))`)
      and `app/src/cloud_object/model/persistence.rs:1752`
      (`.map(|object| object.upsert_params(object.object_type()))`)
      — feeding the `CloudObject` trait's
      `upsert_event`/`bulk_upsert_event` impls through
      `CloudObjectUpsertParams`; the consuming variant took
      `self` by value and its
      `Arc::try_unwrap(...).unwrap_or_else(|m| m.clone())` fast
      path could never execute because the function is never
      entered. Deleted the method + its doc comment (the
      formatter absorbed the leftover blank line) (1 file,
      +0/−22); `use std::sync::Arc;` stays — still used by the
      `model: Arc<M>` field, `Arc::new` in
      `update_from_server_object`, and `shared_model`.
      Deliberately left: the
      `From<CloudObjectUpsertParams<M>> for
      GenericCloudObject<K, M>` impl in the same file (designated
      next — zero `GenericCloudObject::from(` hits, and every
      repo-wide `CloudObjectUpsertParams` binding is an
      `upsert_event(params)` trait-fn parameter consumed by
      model-event constructors, none converted back into a
      `GenericCloudObject`; but trait-impl deadness needs its own
      `.into()`-target trace slice to dual-confirm), the live
      `upsert_params` sibling and `model` / `shared_model` /
      `set_model` / `new` / `new_local` / `new_from_server` /
      `update_from_server_object` (live via
      `search/command_search/notebooks/notebooks_data_source.rs:40`
      et al., `persistence.rs:818` et al.,
      `CloudNotebook::new_local(` at
      `notebooks/active_notebook_data.rs:176` et al.,
      `GenericCloudObject::<K, M>::new_from_server` at
      `persistence.rs:443,472`, `update_from_server_object` at
      `persistence.rs:415`), all `ids.rs` / `drive/mod.rs`
      survivors (re-surveyed: `as_generic_string_object_id`,
      `drive_row_position_id`, `from_generic_string_object` all
      live; `SharingAccessLevel::can_move_drive` / `is_user`
      live), ambient plumbing, telemetry scope (4ca item7), fold
      (item8), redesigns, and all local features. Local-only
      safety: zero callers means zero behavior change — cloud
      object persistence/sync upserts run exclusively through the
      borrowing sibling, and drive, terminal, tabs, panes, BYOK
      AI, settings, themes, and all other local features are
      untouched; only an owning-conversion optimizer that could
      never run is gone.

      Acceptance: `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets`
      both feature sets (default + `--no-default-features
      --features simplewarp`), `--bin simplewarp`, `--bin
      warp-oss`, `--all-targets -p integration` clean (all 7
      exit 0; 0 errors; only the two pre-existing `step.rs`
      unused-import warnings at
      `app/src/integration_testing/input/step.rs:11,13`);
      clippy `-p warp --lib --all-targets` warning-identical to
      the HEAD baseline in BOTH configs (182 lines each, 14
      `^warning` lines each, 13 sorted warning+location pairs
      identical — same 12 pre-existing warnings: 11
      unneeded-return in `app/src/terminal/input.rs` +
      1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`, none in the
      touched file); format stable (`./script/format` idempotent,
      it absorbed the leftover blank line, no residual diff).
      Nextest `-p warp --lib --no-fail-fast`: 4,652 simplewarp
      passed / 4,653 default passed, 4 skipped each, 0 failed
      (zero tests added or removed, no flakes). Built
      `./target/debug/simplewarp` and launched it — alive 74s,
      zero TCP sockets (`lsof -nP -a -p <pid> -iTCP` empty),
      0 panics, clean shutdown (SIGTERM). Did not
      `cargo clean`.

- [x] **`Subject::AnyoneWithLink` (4ei) — DONE 2026-09-23.** Next
      zero-caller leaf in `crates/cloud_objects` after 4eh
      (`GenericCloudObject::into_upsert_params`), picked from a
      full-crate survey. Handoff correction first: the 4eh-designated
      next candidate, the `From<CloudObjectUpsertParams<M>> for
      GenericCloudObject<K, M>` impl in `generic_cloud_object.rs`,
      is LIVE and permanently retired — trace-only, found live, never
      deletable while the `CloudObject` trait's
      `upsert_event`/`bulk_upsert_event` constructors build events
      from params. The 4eh handoff's bare-path grep
      (`GenericCloudObject::from(`) missed 8 call sites in
      alias-qualified/turbofish spellings: `CloudFolder::from(params)`
      / `CloudNotebook::from(params)` at
      `app/src/cloud_object/folders.rs:39,44` and
      `app/src/notebooks/mod.rs:68,73`, `CloudWorkflow::from(params)`
      at `app/src/workflows/mod.rs:218,223`, and
      `GenericCloudObject::<GenericStringObjectId, Self>::from(params)`
      at `app/src/cloud_object/model/generic_string_model.rs:176,204`
      — the alias/turbofish lesson for every future trait-impl trace.
      No commit exists for that aborted round, so this round is 4ei.
      Survey scope: every module of `crates/cloud_objects`
      (sharing.rs, ids.rs, drive/mod.rs, generic_cloud_object.rs,
      server_object.rs, creation.rs, generic_string_model.rs, auth,
      cloud_object/mod.rs). Survivors re-verified live:
      `can_move_drive` via `app/src/drive/index.rs:2304`, `is_user`
      via `cloud_object/mod.rs:483` +
      `app/src/cloud_object/model/view.rs:180`,
      `parse_sqlite_id_to_uid` via
      `app/src/cloud_object/model/actions.rs:146`,
      `ServerIdAndType` + its `sqlite_type_and_uid_hash` via
      `cloud_object_persistence/src/objects.rs:474,485`, all
      `CloudObjectTypeAndId` methods (`sync_id` via
      `persistence.rs:329`, `object_type` via
      `settings_view/agent_profiles_page.rs:355`, `server_id` via
      `update_manager.rs:758`, `uid` via `drive/index.rs:2299` et
      al.), the generic_cloud_object survivors per 4eh, and
      `From<Owner> for Option<ServerId>` via the `owner.into()`
      sites. Leaf evidence (exact-name dual-confirm, ledger /
      `schema.graphql` / fixtures excluded): `git grep -n
      "AnyoneWithLink" -- '*.rs'` exactly one hit — the definition
      at `sharing.rs:70`; `\bSubject::AnyoneWithLink\b` zero
      (no construction, no pattern); all-files hits only
      `schema.graphql`'s unrelated `AnyoneWithLinkSharingPolicy`
      GraphQL type plus the plan ledger; `*.json` fixtures zero.
      The only `Subject` match is live `is_user`'s two
      `Subject::User(..)` arms. Deleted the variant; clippy then
      flagged `is_user`'s `_ => false` arm as newly unreachable
      (Subject down to one variant, both `UserKind` arms cover it),
      so that already-dead arm went too — zero behavior change,
      match now exhaustive (1 file, +0/−2); imports unchanged
      (`LinkSharingSubjectType` stays pub, its derive imports still
      used). Deliberately left: `LinkSharingSubjectType` enum
      (newly callerless payload after this deletion — same shape as
      `TeamKind` in 4ec/4ed, designated next), 
      `UserKind::SharedSessionParticipant` (zero construction sites
      repo-wide, TODO CLD-2283 — but removal edits the live
      `is_user` match, its own slice after), `as_concrete_type`
      (`ServerObject` default trait method, zero call forms in any
      spelling repo-wide — next-next candidate), the four
      `SharingAccessLevel` `From` conversions (inference-based
      `.into()` conversions invisible to name greps; `Role` is
      imported only by sharing.rs so `From<Role>` and
      `From<SharingAccessLevel> for Role` are likely dead — needs a
      dedicated inference-trace slice; prior LIVE verdict kept),
      creation.rs's `CreateObjectRequest` /
      `BulkCreateGenericStringObjectsRequest` / `CreatedCloudObject`
      / `CreateCloudObjectResult` /
      `BulkCreateCloudObjectResult` (zero repo hits outside
      creation.rs — each needs its own slice;
      `ServerCreationInfo` / `RevisionAndLastEditor` in that file
      are live), `Subject::User` / `UserKind::Account`
      (structurally live via `is_user` and the
      `CloudObjectGuest.subject` field type), and all `ids.rs` /
      `drive/mod.rs` survivors. Local-only safety: zero
      constructions and zero match arms means zero behavior change
      — drive sharing subjects, permission gates, terminal, tabs,
      panes, BYOK AI, settings, themes, and all other local
      features untouched; only an unconstructible link-sharing
      variant and an already-unreachable wildcard arm are gone.

      Acceptance: clippy baseline captured at HEAD FIRST in both
      configs (26 captured warning+location lines, 14 `^warning`
      lines, the 12 pre-existing warnings each); after the edit
      plus the wildcard-arm removal, `-p warp --lib --all-targets`
      is byte-identical to the baseline in BOTH configs (default
      and `--no-default-features --features simplewarp`) — the
      intermediate run caught the newly-unreachable arm, resolved,
      re-run identical; all 7 checks exit 0 (`check -p
      cloud_objects --all-targets` ± `--all-features`, `check -p
      warp --lib --all-targets` both feature sets, `--bin
      simplewarp`, `--bin warp-oss`, `--all-targets -p
      integration`) with only the two pre-existing `step.rs`
      unused-import warnings (`single_terminal_view_for_tab`,
      `crate::terminal::CLIAgent`, observed in the integration
      check); format clean (`./script/format`, no diff beyond the
      deletion). Nextest `-p warp --lib --no-fail-fast`: 4,652
      simplewarp passed / 4,653 default passed, 4 skipped each, 0
      failed (exactly the baseline, zero tests added or removed,
      no flakes). Built `./target/debug/simplewarp` and launched
      it — alive 65s, zero TCP sockets (`lsof -nP -a -p <pid>
      -iTCP` empty), 0 panics, clean shutdown (SIGTERM). Did not
      `cargo clean`.

- [x] **`LinkSharingSubjectType` enum (4ej) — DONE 2026-09-23.**
      The 4ei-designated next slice: the enum was the payload of
      the `Subject::AnyoneWithLink` variant deleted in 4ei, making
      it newly callerless — same shape as `TeamKind` in 4ed, where
      the enum was deleted after its payload variant went.
      Independent dual-confirm (bare-name + path-form + type-
      position, ledger / `schema.graphql` / fixtures excluded):
      `git grep -n "LinkSharingSubjectType" -- '*.rs'` exactly one
      hit repo-wide — the definition at
      `crates/cloud_objects/src/drive/sharing.rs:61`;
      `LinkSharingSubjectType::` path-form zero (so zero match
      arms and zero associated-const/variant references);
      `: LinkSharingSubjectType` type-position zero; all-files
      repo-wide grep only that definition plus the `plan.md` ledger
      mentions; `schema.graphql` zero, `*.json` fixtures zero. No
      re-export anywhere (`drive/mod.rs` only declares
      `pub mod sharing;`), no `use ... ::*` glob import of it. Not
      a persisted serde enum: its derives are `Copy, Clone, Debug,
      Eq, PartialEq` only — no `Serialize`/`Deserialize`, no
      `#[allow(dead_code)]`, and no GraphQL/persistence type ever
      carried it, so the "persisted serde enums keep variants"
      convention does not apply (variants `None`/`Anyone` could
      never be constructed and never reached a wire format).
      Imports unchanged — `Serialize`/`Deserialize` stay for
      `SharingAccessLevel`'s derives. Deleted the enum + its derive
      attribute (1 file, +0/−5). Deliberately left:
      `UserKind::SharedSessionParticipant` (designated next — zero
      construction sites repo-wide, re-verified: only the variant
      def at `sharing.rs:75` and the live `is_user` match arm at
      `:83`; TODO CLD-2283 marks it intended for removal, but the
      removal edits the live `is_user` match and the manual
      `PartialEq for UserKind` wildcard, its own slice per 4ei
      handoff — behavior-preserving because an unconstructible
      variant's arm can never run, mirroring 4ei's unreachable-arm
      removal), `as_concrete_type` (`ServerObject` default trait
      method, zero call forms in any spelling — next-next), the
      `From<Role>` / `From<SharingAccessLevel> for Role` pair
      (needs a dedicated `.into()`-target inference trace — apply
      the 4ei alias/turbofish lesson), the creation.rs dead-type
      cluster (`CreateObjectRequest` /
      `BulkCreateGenericStringObjectsRequest` / `CreatedCloudObject`
      / `CreateCloudObjectResult` / `BulkCreateCloudObjectResult`,
      zero repo hits outside creation.rs — each its own slice;
      `ServerCreationInfo` / `RevisionAndLastEditor` there are
      LIVE), `Subject::User` / `UserKind::Account` (structurally
      live via `is_user` and the `CloudObjectGuest.subject` field
      type), and all `ids.rs` / `drive/mod.rs` survivors.
      Local-only safety: zero references means zero behavior
      change — drive sharing subjects, permission gates, terminal,
      tabs, panes, BYOK AI, settings, themes, and all other local
      features untouched; only an unconstructible, unmentionable
      payload enum that no code path could ever name or build is
      gone.

      Acceptance: clippy baseline captured at HEAD FIRST in both
      configs (26 captured warning+location lines, 14 `^warning`
      lines, 12 sorted warning+location pairs each — the 12
      pre-existing warnings: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the
      edit, `-p warp --lib --all-targets` is warning-identical to
      the baseline in BOTH configs (default: raw sorted output
      byte-identical; simplewarp: 12 sorted pairs identical, 14
      `^warning` lines each — raw outputs differ only in
      `Checking`-order and `Finished`-trailer noise); all 7 checks
      exit 0 (`check -p cloud_objects --all-targets` ±
      `--all-features`, `check -p warp --lib --all-targets` both
      feature sets, `--bin simplewarp`, `--bin warp-oss`,
      `--all-targets -p integration`) with only the two
      pre-existing `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`,
      observed in the integration check); format clean
      (`./script/format`, diff still exactly +0/−5). Nextest
      `-p warp --lib --no-fail-fast`: 4,652 simplewarp passed /
      4,653 default passed, 4 skipped each, 0 failed (exactly the
      baseline, zero tests added or removed, no flakes). Built
      `./target/debug/simplewarp` and launched it — alive 81s,
      zero TCP sockets (`lsof -nP -a -p <pid> -iTCP` empty),
      0 panics, clean shutdown (SIGTERM). Did not `cargo clean`.

- [x] **`UserKind::SharedSessionParticipant` (4ek) — DONE 2026-09-23.**
      The 4ej-designated next slice: the unconstructible variant behind
      the TODO CLD-2283, whose removal — unlike prior variant rounds —
      edits the LIVE `is_user` match and the manual `PartialEq for
      UserKind` impl, mirroring 4ei's unreachable-arm removal
      (behavior-preserving: an unconstructible variant's arm can never
      run). Independent verification: construction-site search
      `git grep -n "UserKind::SharedSessionParticipant" -- '*.rs'`
      exactly one hit — the `is_user` destructure pattern at
      `sharing.rs:83` (a pattern, not a construction);
      `SharedSessionParticipant::` path-form zero in all files; bare
      `SharedSessionParticipant` only the variant def at `:75`, that
      `:83` pattern, and the `plan.md` ledger mentions — `schema.graphql`
      zero, `*.json` fixtures zero, no `use UserKind::*` glob, no
      re-export. Match-arm mapping: `UserKind` is referenced nowhere
      outside `sharing.rs` — the only mentions are the `Subject::User`
      payload field type (`:64`), the `is_user` arms (`:82-85`), and the
      manual `PartialEq` impl (`:90-99`); after deleting the variant's
      own `is_user` arm, the match's remaining
      `Subject::User(UserKind::Account(user_uid))` arm is exhaustive
      (Subject has one variant, UserKind one), and `PartialEq`'s
      `_ => false` wildcard plus its participant-specific comment became
      newly unreachable and went with it (single
      `(Self::Account, Self::Account)` arm now exhaustive), per the 4ei
      precedent. Serde-rule check: `UserKind` derives only
      `Debug, Clone` and `Subject` only `Debug, Clone, PartialEq` — no
      `Serialize`/`Deserialize` on either, no persistence or GraphQL type
      carries `UserKind`, so the "persisted serde enums keep variants"
      convention does not apply and deletion (not `is_available() =>
      false`) is correct. TODO CLD-2283 sat directly on the variant
      ("Remove this once we have Firebase UIDs for shared session
      participants") and the `/// A session-sharing participant.` doc
      comment was variant-specific — both deleted with it; the
      `UserKind` container doc left untouched. Import consequence
      resolved within the round: `SessionSharingProfileData` (alias of
      `session_sharing_protocol::common::ProfileData`) was used only by
      the deleted variant, so the import shrank to
      `use session_sharing_protocol::common::Role;` (`Role` stays —
      used by the live `From<Role>` / `From<SharingAccessLevel> for
      Role` impls). Deleted the variant + its doc + TODO, its `is_user`
      arm, and the `PartialEq` wildcard arm + comment (1 file,
      +1/−10). Deliberately left: `UserKind` itself and `Subject::User`
      (structurally live via `is_user` — callers
      `crates/cloud_objects/src/cloud_object/mod.rs:483` and
      `app/src/cloud_object/model/view.rs:180` — and the
      `CloudObjectGuest.subject` field type at
      `cloud_object/mod.rs:503`), `as_concrete_type`
      (`ServerObject` default trait method, re-verified zero call forms
      in any spelling repo-wide — only the definition at
      `server_object.rs:43` plus ledger mentions; designated next),
      the `From<Role>` / `From<SharingAccessLevel> for Role` pair
      (needs a dedicated `.into()`-target inference trace — apply the
      4ei alias/turbofish lesson), the creation.rs dead-type cluster
      (`CreateObjectRequest` / `BulkCreateGenericStringObjectsRequest`
      / `CreatedCloudObject` / `CreateCloudObjectResult` /
      `BulkCreateCloudObjectResult`, zero repo hits outside
      creation.rs — each its own slice; `ServerCreationInfo` /
      `RevisionAndLastEditor` there are LIVE), and all `ids.rs` /
      `drive/mod.rs` survivors. Local-only safety: zero constructions
      means zero behavior change — the deleted match arm and wildcard
      could never execute (the variant could never be built), so
      `is_user` and `UserKind` equality behave identically for every
      value that can exist; drive sharing subjects, permission gates,
      terminal, tabs, panes, BYOK AI, settings, themes, and all other
      local features untouched; only an unconstructible
      session-participant variant and its already-dead arm are gone.

      Acceptance: clippy baseline captured at HEAD FIRST in both
      configs (12 sorted warning+location pairs each — the 12
      pre-existing warnings: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the edit,
      `-p warp --lib --all-targets` is warning-identical to the
      baseline in BOTH configs (default and `--no-default-features
      --features simplewarp` — diff empty each, no
      newly-unreachable-arm or unused-import warnings). All 7 checks
      exit 0 (`check -p cloud_objects --all-targets` ±
      `--all-features`, `check -p warp --lib --all-targets` both
      feature sets, `--bin simplewarp`, `--bin warp-oss`,
      `--all-targets -p integration`) with only the two pre-existing
      `step.rs` unused-import warnings (`single_terminal_view_for_tab`,
      `crate::terminal::CLIAgent`, observed in the integration check);
      format clean (`./script/format`, diff still exactly +1/−10).
      Nextest `-p warp --lib --no-fail-fast`: 4,652 simplewarp passed /
      4,653 default passed, 4 skipped each, 0 failed (exactly the
      baseline, zero tests added or removed, no flakes). Built
      `./target/debug/simplewarp` and launched it — alive 65s, zero
      TCP sockets (`lsof -nP -a -p <pid> -iTCP` empty), 0 panics,
      clean shutdown (SIGTERM). Did not `cargo clean`.

- [x] **`ServerObject::as_concrete_type` (4el) — DONE 2026-09-23.**
      The 4ek-designated next slice: the `ServerObject` default trait
      method at `server_object.rs:41-54`, the sweep's first dead
      DEFAULT trait method — prior rounds deleted inherent items, so
      verification added the two trait-only proofs (override search,
      dynamic-dispatch trace) on top of the usual exhaustive
      call-form search. Evidence (ledger / `schema.graphql` /
      fixtures excluded): bare-name `git grep -n "as_concrete_type"
      -- '*.rs'` exactly one hit repo-wide — the definition;
      all-files grep only that definition plus the three `plan.md`
      ledger mentions; `schema.graphql` zero, `*.json` fixtures
      zero. Exhaustive call-form search: `.as_concrete_type(` zero,
      `::as_concrete_type` zero (covers
      `<T as ServerObject>::as_concrete_type::<K, M>(...)` turbofish
      and `SomeType::as_concrete_type(...)` path spellings),
      `as_concrete_type;` use-declaration form zero. Override
      search: `fn as_concrete_type` only the definition — zero
      impl-block overrides anywhere including test code and other
      crates; the trait's only `impl ServerObject for` block
      repo-wide is `GenericServerObject` at `server_object.rs:139`,
      which overrides only `object_type` / `as_any` / `clone_box`.
      Dynamic-dispatch reach: structurally impossible — the method
      is an associated function (no `self` receiver) with
      `Self: Sized`, so it has no vtable slot and can only be
      invoked by spelling a concrete type, i.e. a path form, all
      zero; `T: ServerObject` generic-bounded calls would surface
      as `.as_concrete_type(` — zero; every `dyn ServerObject`
      mention repo-wide sits inside `server_object.rs` itself, and
      the two `From<&dyn ServerObject> for
      Option<&GenericServerObject<K, M>>` impls downcast via
      `as_any().downcast_ref()` directly, never through
      `as_concrete_type`. No use-declaration imports the name, and
      macro bodies are `*.rs` text, so the bare-name hit count
      already covers generated code. Deleted the method + its doc
      comment (1 file, +0/−15); imports unchanged —
      `std::any::Any` stays (`as_any` is a required trait method
      and the `From` impls downcast through it). Live-from-dead
      trace in the same file: `as_any` / `clone_box` (required
      methods, implemented by `GenericServerObject`),
      `GenericServerObject` + `ServerObjectModel` (used across
      `app/src/cloud_object/model/persistence.rs` and the
      `cloud_object_models` type aliases), the `From<&dyn
      ServerObject>` downcast impls (separate items, untouched).
      Deliberately left: the `From<Role> for SharingAccessLevel` /
      `From<SharingAccessLevel> for Role` pair in sharing.rs
      (designated next — inference-based `.into()` conversions
      invisible to name greps; `Role` is imported at
      `sharing.rs:2` and used only by these two impls, so a
      dead-pair verdict would also drop the import; the trace must
      be conclusive either way — alias-qualified `.into()`,
      `From::from` turbofish, `let x: T = ...` inference targets,
      applying the 4ei alias/turbofish lesson to call sites in
      `app/` too), the creation.rs dead-type cluster
      (`CreateObjectRequest` /
      `BulkCreateGenericStringObjectsRequest` /
      `CreatedCloudObject` / `CreateCloudObjectResult` /
      `BulkCreateCloudObjectResult`, zero repo hits outside
      creation.rs — each its own slice; `ServerCreationInfo` /
      `RevisionAndLastEditor` there are LIVE), the `From<&dyn
      ServerObject>` downcast impls (each needs its own
      `.into()`-target trace slice before any judgment), all other
      `ServerObject` / `GenericServerObject` members, and all
      `ids.rs` / `drive/mod.rs` survivors. Local-only safety: zero
      call forms in any spelling means the default body could
      never execute and no override existed to lose — cloud-object
      sync, drive, terminal, tabs, panes, BYOK AI, settings,
      themes, and all other local features untouched; only a
      downcast helper that could never be named or called is gone.

      Acceptance: clippy baseline captured at HEAD FIRST in both
      configs (12 sorted warning+location pairs each — the 12
      pre-existing warnings: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the
      edit, `-p warp --lib --all-targets` is warning-identical to
      the baseline in BOTH configs (default and
      `--no-default-features --features simplewarp` — 12 sorted
      pairs identical each, no unused-import or dead-code
      warnings). All 7 checks exit 0 (`check -p cloud_objects
      --all-targets` ± `--all-features`, `check -p warp --lib
      --all-targets` both feature sets, `--bin simplewarp`, `--bin
      warp-oss`, `--all-targets -p integration`) with only the two
      pre-existing `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`,
      observed in the integration check); format clean
      (`./script/format`, diff still exactly +0/−15). Nextest
      `-p warp --lib --no-fail-fast`: 4,652 simplewarp passed /
      4,653 default passed, 4 skipped each, 0 failed (exactly the
      baseline, zero tests added or removed, no flakes). Built
      `./target/debug/simplewarp` and launched it — alive 65s,
      zero TCP sockets on the smoke pid (`lsof -nP -a -p <pid>
      -iTCP` empty; an unrelated long-running
      `/Applications/SimpleWarp.app` instance holds the only
      listener), 0 panics, clean shutdown (SIGTERM, its
      `terminal-server` helper child gone too). Did not
      `cargo clean`.

- [x] **`From<Role>`/`From<SharingAccessLevel>` pair (4em) — DONE
      2026-09-23.** The 4el-designated next slice: the sweep's first
      INFERENCE-TRACE deletion — both `From` impls in
      `crates/cloud_objects/src/drive/sharing.rs:41-58` are reachable
      only through `.into()`/`from` resolution, invisible to name
      greps, so the trace enumerated every way either conversion could
      instantiate, applying the 4ei alias/turbofish lesson to call
      sites. `Role` provenance: it lives in the external git
      dependency `session-sharing-protocol` (`common::roles.rs`,
      serde `Reader`/`Executor`/`Full`), not a workspace member —
      what matters is workspace reachability. Direction A
      (`From<Role> for SharingAccessLevel`) needs a `Role`-typed
      VALUE: construction-site finding — the bare `\bRole\b` name
      appears in workspace `*.rs` ONLY at the `sharing.rs:2` import
      and inside the two impls themselves; zero `enum Role` /
      `struct Role` / `type Role` workspace definitions; zero
      `::Role` path or `Role =` alias forms; no glob
      `use session_sharing_protocol::common::*` (all five protocol
      imports are explicit and enumerated: `ProfileData`
      (`cloud_object_models/src/user_profile.rs:3`), `SessionId`
      (`app/src/ai/agent_conversations_model/entry.rs:2`),
      `InputMode` + `InputType as ProtocolInputType`
      (`app/src/ai/blocklist/input_model.rs:17`), fully-qualified
      `ServerConversationToken`
      (`app/src/ai/agent/api.rs:105-112`)); every Role-carrying
      protocol type is unreferenced in the workspace — `Viewer.role`
      and `AccessLevels.max_acl/direct_acl`
      (`common/participant.rs:8,70,80,94,102`), `TeamAclData.acl`
      (`team.rs:7`), the sharer/viewer control-message `role: Role`
      payloads (`viewer.rs`, `sharer.rs`) — the naive grep hits are
      unrelated local shapes (GraphQL `Viewer` enum,
      `ArgumentEditorMode::Viewer`, `AccessLevel::Viewer` variants);
      and the five workspace-used protocol types are all Role-free
      (`ProfileData` = strings + `InputReplicaId`; `SessionId` and
      `ServerConversationToken` = `Uuid` newtypes; `InputType` /
      `InputMode` = plain enums). Zero `Role` construction sites
      means no `Role` value can ever exist in workspace code, so the
      conversion can never fire. Direction B
      (`From<SharingAccessLevel> for Role`) needs `Role` as an
      inference TARGET — annotation, binding, fn
      param/return, struct field, turbofish, or bound — all of which
      require naming `Role` outside `sharing.rs`, impossible per the
      above. Every SharingAccessLevel-typed value was enumerated and
      its consumers read: `CloudViewModel::access_level` /
      `object_access_level` (`app/src/cloud_object/model/view.rs:157,164`
      — locals only `.max()`, `<`, returns), `drive/index.rs:2301`
      (`.can_move_drive()` — the live trace column) and `:3874`
      (`_access_level`, unused), `env_var_collection.rs:1229` →
      `render_trash_banner(_access_level: SharingAccessLevel)`
      (param ignored, `fixed_view_components.rs:55`),
      `notebook.rs:1230` and `menus.rs:363` (`_access_level`,
      unused), the `CloudLinkSharing`/`CloudObjectGuest.access_level`
      field reads (`view.rs:169,185` — `.max()` only), and the
      persistence layer, which keeps the ACL columns NULL
      (`(None, None)`, `cloud_object_persistence/src/objects.rs:162-166`)
      — zero `.into()` on any SharingAccessLevel value anywhere, and
      the `ServerLinkSharing`/`ServerObjectGuest` AccessLevel-typed
      sinks (`cloud_object/mod.rs:371,379`) are built only from
      GraphQL `TryFrom`. Exhaustive form search: `Role::from(` zero,
      `SharingAccessLevel::from(` zero, `Into::<Role>` /
      `Into::<SharingAccessLevel>` turbofish zero, the only
      `From<Role>`/`Into<Role>`/`From<SharingAccessLevel>`/
      `Into<SharingAccessLevel>` bound shapes are the impls
      themselves plus the remaining `From<SharingAccessLevel> for
      AccessLevel` (different target), the generic `From::from` /
      `map(Into::into)` sites (~100) triaged to unrelated types (GQL
      tier policy fields, sync ids, paths, colors, LLM/conversation
      types), `impl Into<...>` params zero. Macro-generated code
      cannot reach the impls (macro bodies are `*.rs` text, already
      in the greps); `schema.graphql` and `*.json` fixtures cannot
      invoke Rust `From` impls; test code included in all searches.
      Deleted both impls plus the now-unused
      `use session_sharing_protocol::common::Role;` import (1 file,
      +0/−20); `AccessLevel` import stays (used by the two remaining
      impls); cloud_objects' `session-sharing-protocol` Cargo
      dependency left in place (used by other workspace crates;
      unused deps warn-free). Deliberately left: the
      `From<AccessLevel> for SharingAccessLevel` /
      `From<SharingAccessLevel> for AccessLevel` pair (same file,
      also inference-based — this trace observed no caller for
      either either, but prior handoffs recorded them as live
      server/session-mapping paths, so they need their own dedicated
      inference-trace slice before any judgment — same class as this
      round), the creation.rs dead-type cluster (DESIGNATED NEXT:
      `CreateObjectRequest` /
      `BulkCreateGenericStringObjectsRequest` / `CreatedCloudObject`
      / `CreateCloudObjectResult` / `BulkCreateCloudObjectResult`,
      zero repo hits outside creation.rs per the 4ei survey — each
      its own slice; `ServerCreationInfo` / `RevisionAndLastEditor`
      in that file are LIVE and must not be touched), the `From<&dyn
      ServerObject>` downcast impls (each needs its own
      `.into()`-target trace slice), `Subject` / `UserKind` /
      `is_user` (live via `cloud_object/mod.rs:483` +
      `app/src/cloud_object/model/view.rs:180`), `can_move_drive`
      (live), and all `ids.rs` / `drive/mod.rs` survivors.
      Local-only safety: unreachable-conversion deletion means zero
      behavior change — no code path could name or construct a
      `Role`, and no SharingAccessLevel value ever flowed anywhere
      but comparisons/`max`/`can_move_drive`/ignored params, so the
      deleted match arms could never execute; drive sharing levels,
      permission gates, terminal, tabs, panes, BYOK AI, settings,
      themes, and all other local features untouched; only two
      never-instantiable trait impls and their import are gone.

      Acceptance: clippy baseline captured at HEAD FIRST in both
      configs (14 sorted warning+location lines each — the 12
      pre-existing warnings: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the edit,
      `-p warp --lib --all-targets` is warning-identical to the
      baseline in BOTH configs (default and `--no-default-features
      --features simplewarp` — sorted pair diffs empty, no
      unused-import or dead-code warnings). All 7 checks exit 0
      (`check -p cloud_objects --all-targets` ± `--all-features`,
      `check -p warp --lib --all-targets` both feature sets, `--bin
      simplewarp`, `--bin warp-oss`, `--all-targets -p integration`)
      with 0 errors and only the two pre-existing `step.rs`
      unused-import warnings (`single_terminal_view_for_tab`,
      `crate::terminal::CLIAgent`, observed in the integration
      check); format clean (`./script/format`, diff still exactly
      +0/−20). Nextest `-p warp --lib --no-fail-fast`: 4,652
      simplewarp passed / 4,653 default passed, 4 skipped each, 0
      failed (exactly the baseline, zero tests added or removed, no
      flakes). Built `./target/debug/simplewarp` and launched it —
      alive 79s, zero TCP sockets on the smoke pid (`lsof -nP -a -p
      55718 -iTCP` empty; the unrelated long-running
      `/Applications/SimpleWarp.app` instance still holds the only
      listener), 0 panics (empty log), clean shutdown (SIGTERM). Did
      not `cargo clean`.

- [x] **creation.rs dead-type cluster (4en) — DONE 2026-09-23.** The
      4em-designated next slice, taken as one cluster: the five
      remote-creation request/response types in
      `crates/cloud_objects/src/cloud_object/creation.rs` —
      `CreateObjectRequest`, `BulkCreateGenericStringObjectsRequest`,
      `CreatedCloudObject`, `CreateCloudObjectResult`,
      `BulkCreateCloudObjectResult`. Independent per-type
      verification (the 4ei survey was a lead, not proof): for EACH
      type, bare-name `git grep` over all files hits ONLY the
      definition in creation.rs (plus the two in-file uses of
      `CreatedCloudObject` as payload of the two deleted enums),
      `plan.md` ledger mentions, and one stale prose doc mention of
      `CreateObjectRequest` at
      `cloud_object_models/src/notebook.rs:13` ("avoid polluting the
      generic CreateObjectRequest type" — a comment, not a use);
      path-form `T::` zero, type-position `[<:,]\s*T` zero, snake_case
      bindings zero — for all five. No inference-trace caveat applies
      (unlike 4em's `From` impls): a struct/enum can only be used by
      naming it — construction, field/fn-signature/trait-impl/alias
      all spell the name — so bare+path+type-position coverage is
      conclusive. Exposure: the module is wired via
      `cloud_object/mod.rs:26 mod creation;` + `:32 pub use
      creation::*;` glob (any external use would still spell the bare
      name — zero); `creation::` path usage zero outside that glob;
      none of the five derives `Serialize`/`Deserialize` (derives are
      `PartialEq, Eq, Debug` once and `Debug` thrice, plus
      `allow(clippy::large_enum_variant)`); zero hits in
      `crates/persistence`, `crates/cloud_object_persistence`,
      `app/src/persistence`, `app/src/cloud_object`, and
      `crates/warp_graphql*`; `schema.graphql` zero — the wire's
      `BulkCreateObjectsInput` / `BulkCreateObjectsOutput` /
      `BulkCreateObjectsResult` and the `bulkCreateObjects` mutation
      are schema-side GraphQL types with different names, untouched;
      `*.json` / `*.toml` fixtures zero. These are the client-side
      types of the remote creation API that no longer runs — exactly
      the remote-dependent class this effort removes. Deleted the five
      types with their doc comments, plus the nine imports they
      orphaned in creation.rs (`ServerTimestamp`,
      `CloudObjectEventEntrypoint`, `GenericStringObjectFormat`,
      `GenericStringObjectUniqueKey`, `Owner`,
      `RevisionAndLastEditor`, `SerializedModel`, `ClientId`,
      `FolderId` — `ServerPermissions` and `ServerIdAndType` stay for
      `ServerCreationInfo`), plus the two stale doc lines in
      `notebook.rs` (2 files, +2/−67; creation.rs 76 → 11 lines,
      keeping only `ServerCreationInfo`). One-hop orphan check: every
      field type of the deleted types re-verified heavily used
      elsewhere — `CloudObjectEventEntrypoint` (`cloud_object/mod.rs:745`
      def + `:827` GraphQL `From` impl), `GenericStringObjectFormat` /
      `GenericStringObjectUniqueKey` (`app/src/ai/cloud_agent_config`,
      `cloud_environments`, `execution_profiles`, `facts`,
      `blocklist`), `SerializedModel` (`app/src/cloud_object/folders.rs:51`,
      `app/src/cloud_object/mod.rs:449`,
      `model/generic_string_model.rs:27`), `ServerPermissions`
      (`app/src/ai/conversation.rs:4503` et al.) — nothing else
      orphaned. Deliberately left: `ServerCreationInfo` (LIVE —
      `app/src/cloud_object/model/persistence.rs:17` +
      `crates/cloud_object_persistence/src/objects.rs:7`),
      `RevisionAndLastEditor` (LIVE — defined at
      `cloud_object/mod.rs:782`, not in creation.rs as the handoff
      loosely said; live via `app/src/cloud_object/model/persistence.rs:249`,
      `app/src/persistence/mod.rs:342`,
      `cloud_object_persistence/src/objects.rs:423`; only
      creation.rs's now-unused import of it went), and everything
      else per prior rounds. Designated NEXT: the
      `From<AccessLevel> for SharingAccessLevel` /
      `From<SharingAccessLevel> for AccessLevel` pair in sharing.rs —
      same inference-trace class as 4em (the 4em trace observed no
      caller for either, but prior handoffs recorded them as live
      server/session-mapping paths, so they need their own dedicated
      slice enumerating alias-qualified `.into()`, `From::from`
      turbofish, and inference targets — apply the 4ei lesson); after
      that, a fresh survey of `crates/cloud_objects` — if the crate
      is clean, the effort pivots to the next major item (the
      cloud-run lifecycle walls per the 4ca plan). Local-only safety:
      zero references means zero behavior change — the deleted types
      are the request/response shapes of a remote creation API that
      no longer runs, so no code path could construct, receive, or
      even name them; cloud-object persistence/sync runs exclusively
      through the live `ServerCreationInfo` path, and drive, terminal,
      tabs, panes, BYOK AI, settings, themes, and all other local
      features are untouched; only five never-instantiated wire types
      are gone.

      Acceptance: clippy baseline captured at HEAD FIRST in both
      configs (12 sorted warning+location pairs each — the 12
      pre-existing warnings: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the edit,
      `-p warp --lib --all-targets` is warning-identical to the
      baseline in BOTH configs (default and `--no-default-features
      --features simplewarp` — sorted-pair diffs empty, 14 `^warning`
      lines each, no unused-import or dead-code warnings). All 7
      checks exit 0 (`check -p cloud_objects --all-targets` ±
      `--all-features`, `check -p warp --lib --all-targets` both
      feature sets, `--no-default-features --features simplewarp
      --bin simplewarp`, `--bin warp-oss`, `--all-targets -p
      integration`) with 0 errors and only the two pre-existing
      `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`,
      observed in the integration check); format clean
      (`./script/format`, diff still exactly +2/−67). Nextest
      `-p warp --lib --no-fail-fast`: 4,652 simplewarp passed /
      4,653 default passed, 4 skipped each, 0 failed (first
      simplewarp run hit the known flake
      `notebooks::notebook::tests::test_command_block_dispatches_event`
      — passes in isolation and on the full rerun, per the 4ed
      precedent; exactly the baseline, zero tests added or removed).
      `cargo nextest run -p cloud_objects --no-fail-fast`: 0 tests
      (the crate has no test attributes). Runtime smoke test
      SKIPPED: the user is away and nobody can answer the macOS
      password prompt, so per the 2026-09-23 convention change the
      GUI binary was not built or launched — unit tests plus checks
      are the acceptance for this round. Did not `cargo clean`.

- [x] **`From<AccessLevel>`/`From<SharingAccessLevel> for AccessLevel`
      pair (4eo) — DONE 2026-09-23.** The 4en-designated next slice:
      the sweep's second INFERENCE-TRACE deletion, same class as 4em —
      both impls (sharing.rs:20-38 pre-edit) were reachable only
      through `.into()`/`from` resolution, invisible to name greps.
      `AccessLevel` re-verified from scratch as a LIVE workspace type:
      the cynic GraphQL enum at
      `crates/graphql/src/api/object_permissions.rs:72` (package
      `crates/graphql`, lib name `warp_graphql`), deserialized from
      server responses for the wire fields `ObjectGuest.access_level`
      (`:17`) and `LinkSharing.access_level` (`:83`) — the type and
      all its other users untouched. Direction A
      (`From<AccessLevel> for SharingAccessLevel`) needs an
      `AccessLevel`-typed VALUE flowing into a `SharingAccessLevel`
      inference target: every AccessLevel-typed value was enumerated
      and read — the two GraphQL fields flow by plain field-move
      (`access_level: value.access_level`) into
      `ServerObjectGuest.access_level` (`cloud_object/mod.rs:379`,
      built at `:917-919`) and `ServerLinkSharing.access_level`
      (`:371`, built at `:961-962`), and those server ACL values are
      then dropped UNREAD: the only readers of
      `ServerPermissions.guests` / `.anyone_link_sharing` are
      `CloudObjectPermissions::new_from_server` (`mod.rs:458`,
      "Guest and link-sharing ACLs from the server are ignored" —
      `guests: Vec::new()` + `anyone_with_link: None` at `:463-464`)
      and the DB path `to_cloud_object_permissions`
      (`cloud_object_persistence/src/objects.rs:576`, same
      `(Vec::new(), None)` at `:588-589`), while
      `ServerPermissions::mock_personal` (`:412`) and test fixtures
      set them empty/`None` — zero `.access_level` reads on any
      `Server*` type exist anywhere, so no AccessLevel value can ever
      reach a conversion site; the prior rounds' "live
      server/session-mapping paths" verdict is overturned by this
      from-scratch re-trace (the mapping code that would have used it
      no longer exists). Direction B
      (`From<SharingAccessLevel> for AccessLevel`) needs an
      AccessLevel inference TARGET — annotation, fn param/return,
      struct field, turbofish, or bound — all of which require naming
      `AccessLevel` outside sharing.rs: the bare name appears in
      exactly three files (the graphql definition,
      `cloud_object/mod.rs`, sharing.rs), no import renames
      (`as AccessLevel` zero), and every SharingAccessLevel-typed
      value re-enumerated post-4en (files shifted) flows only into
      comparisons, returns, or ignored params:
      `CloudViewModel::access_level` / `object_access_level`
      (`app/src/cloud_object/model/view.rs:157,164` — locals only
      `.max()`, `<`, returns; the `link_settings.access_level` /
      `guest.access_level` reads at `:174,181` are
      `CloudLinkSharing`/`CloudObjectGuest` fields,
      SharingAccessLevel→SharingAccessLevel),
      `drive/index.rs:2299` → `.can_move_drive()` at `:2304` (the
      live trace column), `:3520` and `:3874` (`_access_level`,
      unused), `env_var_collection.rs:1229` →
      `render_trash_banner(_access_level: SharingAccessLevel)`
      (param ignored, `fixed_view_components.rs:55`),
      `active_env_var_collection_data.rs:174`,
      `active_notebook_data.rs:307`, `menus.rs:363`,
      `notebook.rs:1230` (returns and `_access_level`, unused), and
      the `CloudLinkSharing`/`CloudObjectGuest.access_level` fields
      (`cloud_object/mod.rs:495,504`) whose containers are
      constructed ONLY empty — zero `.into()` receivers, zero
      `AccessLevel::from(` / `SharingAccessLevel::from(`, zero
      `Into::<T>` / `From::<T>` turbofish, and the only
      `From<AccessLevel>` / `Into<AccessLevel>` /
      `From<SharingAccessLevel>` / `Into<SharingAccessLevel>` bound
      shapes repo-wide are the two impls themselves (ref/nested
      `From<&AccessLevel>`-style forms zero, `impl Into<...>` params
      zero); the generic `From::from` / `map(Into::into)` /
      `map_into()` sites (~100) triaged to unrelated types
      (AI/tool-usage metadata, theme colors, revisions, sync ids,
      lsp/completer/shell types — zero in any cloud/drive/sharing
      file). Macro-generated code cannot reach the impls (macro
      bodies are `*.rs` text, already in the greps);
      `schema.graphql` and `*.json` fixtures cannot invoke Rust
      `From` impls; test code included in all searches. Deleted both
      impls plus the now-unused
      `use warp_graphql::object_permissions::AccessLevel;` import
      (1 file, +0/−21); the serde derives on `SharingAccessLevel`
      serialize variant names directly and never consulted these
      impls — no wire path touched.
      Deliberately left: `AccessLevel` and all its users (the graphql
      wire fields, `ServerLinkSharing`/`ServerObjectGuest` and their
      TryFrom constructors — live code that merely drops the server
      ACL payloads, a redesign-class item, not dead code),
      `SharingAccessLevel::can_move_drive` (live via
      `app/src/drive/index.rs:2304`) / `is_user` (live via
      `cloud_object/mod.rs:483` +
      `app/src/cloud_object/model/view.rs:180`), `Subject` /
      `UserKind`, all `ids.rs` / `drive/mod.rs` /
      `generic_cloud_object.rs` / `server_object.rs` /
      `creation.rs` survivors, ambient plumbing, telemetry scope
      (4ca item7), fold (item8), redesigns, and all local features.
      Local-only safety: unreachable-conversion deletion means zero
      behavior change — no `SharingAccessLevel` value ever flowed
      anywhere but comparisons/`max`/`can_move_drive`/ignored
      params, and no server `AccessLevel` value was ever read after
      construction, so both impls could never execute; drive sharing
      levels, permission gates, cloud-object persistence/sync,
      terminal, tabs, panes, BYOK AI, settings, themes, and all
      other local features untouched; only two never-instantiable
      trait impls and their import are gone. Designated NEXT, from a
      fresh whole-crate survey of `crates/cloud_objects` (sharing.rs,
      ids.rs, drive/mod.rs, generic_cloud_object.rs, server_object.rs,
      creation.rs, generic_string_model.rs, auth/, cloud_object/mod.rs
      — all re-verified live per prior rounds, `ConflictStatus` live
      via `generic_cloud_object.rs:40` +
      `app/src/cloud_object/mod.rs:599-625`, TEST_USER re-exports
      live, `models/mod.rs` an empty declared module at
      `cloud_object/mod.rs:29` — trivial cleanup): the
      `From<&'a dyn ServerObject>` / `From<&'a Box<dyn ServerObject>>
      for Option<&'a GenericServerObject<K, M>>` downcast-impl pair in
      `server_object.rs` — `dyn ServerObject` and
      `Box<dyn ServerObject>` have ZERO mentions in any form outside
      `server_object.rs`, so no value of either type can exist at any
      call site and the impls' only invocation forms
      (`.into()`/`From::from` on such references) are unreachable;
      behind them the entire `ServerObject` trait shows zero external
      mentions in any spelling (bound, path, `dyn`, `impl ... for`
      outside the file) while `ServerObjectModel` /
      `GenericServerObject` stay heavily live via the
      `cloud_object_models` type aliases — each its own slice, trait
      deadness needs its own conclusive trace. If that cluster
      exhausts, the crate is clean and the effort pivots to the next
      major item (the cloud-run lifecycle walls per the 4ca plan) —
      not started this round.

      Acceptance: clippy baseline captured at HEAD FIRST in both
      configs (16 sorted warning+location pair-lines each, 14
      `^warning` lines each — the 12 pre-existing warnings: 11
      unneeded-return in `app/src/terminal/input.rs` +
      1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the edit,
      `-p warp --lib --all-targets` is warning-identical to the
      baseline in BOTH configs (sorted-pair diffs empty, 14
      `^warning` lines each, no unused-import or dead-code
      warnings). All 7 checks exit 0 (`check -p cloud_objects
      --all-targets` ± `--all-features`, `check -p warp --lib
      --all-targets` both feature sets, `--no-default-features
      --features simplewarp --bin simplewarp`, `--bin warp-oss`,
      `--all-targets -p integration`) with 0 errors and only the two
      pre-existing `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`,
      observed in the integration check); format clean
      (`./script/format`, diff still exactly +0/−21). Nextest
      `-p warp --lib --no-fail-fast`: 4,652 simplewarp passed /
      4,653 default passed, 4 skipped each, 0 failed (exactly the
      baseline, zero tests added or removed, no flakes).
      `cargo check -p cloud_objects` emits zero warnings. Runtime
      smoke test SKIPPED: the user is away and nobody can answer the
      macOS password prompt, so per the 2026-09-23 convention change
      the GUI binary was not built or launched — unit tests plus
      checks are the acceptance for this round. Did not
      `cargo clean`.

- [x] **`ServerObject` downcast `From` impl pair (4ep) — DONE 2026-09-23.**
      The 4eo-designated next slice: the
      `From<&'a dyn ServerObject>` / `From<&'a Box<dyn ServerObject>> for
      Option<&'a GenericServerObject<K, M>>` downcast impls at
      `server_object.rs:104-122` (pre-edit), verified by construction-site
      unreachability. Exhaustive `dyn ServerObject` trace (ledger /
      `schema.graphql` / fixtures excluded): `git grep -n "dyn
      ServerObject"` over all files hits exactly six, all inside
      `server_object.rs` itself — the `clone_box` return type (`:45`), the
      two impls being deleted (`:104,109,114,119`), and the
      `GenericServerObject` override (`:137`); zero mentions in
      `Box<>`/`Arc<>`/`Vec<>`/`Rc<>` wrappers outside the file, zero in
      generic bounds, zero in use statements. Word-boundary `\bServerObject\b`
      (digit-guarded) zero outside the file — all external substring hits are
      `ServerObjectModel` / `GenericServerObject` / `ServerObjectContainer` /
      `ServerObjectGuest`, different items. The 37 `pub type` aliases in
      `cloud_object_models` enumerated with multi-line expansions read: every
      one expands to `GenericCloudObject<...>`, `GenericStringModel<...>`, or
      `GenericServerObject<...>` — none to the trait object.
      `impl ... ServerObject for` only `server_object.rs:124`;
      `ServerObject::` path form zero; `as ServerObject` renames zero; no
      non-Rust mention outside the ledger. So no `&dyn ServerObject` or
      `&Box<dyn ServerObject>` value can exist at any call site — the impls'
      only invocation forms (`.into()`/`From::from` on such references) are
      unreachable; the live `.into()`-target downcasts that exist
      (`Option<&mut GenericCloudObject<K, M>> = boxed.into()` at
      `app/src/cloud_object/model/persistence.rs:424`, `Option<&CloudFolder> =
      object.into()` at `:1695`) run through the APP-SIDE `CloudObject` trait
      pair (`app/src/cloud_object/mod.rs:340`), a different, live impl set.
      Deleted BOTH impls (1 file, +0/−20); imports unchanged —
      `std::any::Any` stays (`as_any` is a required trait method, def `:39` +
      `GenericServerObject` override `:113`).
      Deliberately left: the `ServerObject` trait itself (DESIGNATED NEXT —
      zero external mentions per this round's trace: word-boundary bare-name
      zero outside `server_object.rs`, the only `impl ServerObject for` is
      `GenericServerObject`, no bound/use/path/rename form anywhere; no call
      resolves through the trait — `generic_cloud_object.rs:136-160`
      `new_from_server`/`update_from_server_object` and the
      `persistence.rs` consumers (`:404,432,463,1662,1714`) only read
      `GenericServerObject` fields (`.id`/`.model`/`.metadata`/
      `.permissions`), the crate's only `.object_type()` call is
      `server_object.rs:110` resolving on `M: ServerObjectModel` (live), and
      `persistence.rs:370`'s `.object_type()` receiver is
      `Box<dyn CloudObject>`; the `TeamKind` precedent — trait deadness needs
      its own conclusive slice, with `GenericServerObject` staying live via
      the aliases and degenerating once the trait goes),
      `ServerObject::as_any` (newly callerless after this deletion — its only
      caller was the deleted `From<&dyn ServerObject>` body; every other
      `.as_any(` repo-wide is a different trait, receivers verified: app-level
      `dyn CloudObject` (`app/src/cloud_object/mod.rs:649`,
      `drive_object_type.rs:158`, `embedded_item.rs:214`,
      `embedding_model.rs:187`, `input_context.rs:280`), `CommandExecutor`
      (`remote_server.rs:219`), plus warpui `Model`/`View`/`Action`,
      `DropTargetData`, `PaneContent` — next-round candidate),
      `ServerObject::clone_box` (already callerless BEFORE this round — zero
      `.clone_box()` / `::clone_box` on `ServerObject` receivers anywhere; all
      call sites are `CloudObject` (`app/src/cloud_object/mod.rs:782`),
      `CloudStringObject` (`app/src/cloud_object/model/
      generic_string_model.rs:179`), `WarpDriveItem`
      (`app/src/cloud_object/warp_drive_item.rs:75`), `DropdownItemAction`
      (`app/src/view_components/dropdown.rs:51,221`), `ChildModelHandle` —
      next-round candidate), the empty `cloud_object/models/mod.rs` module
      (1-byte file, `pub mod models;` at `cloud_object/mod.rs:29`, zero
      `cloud_object::models` path mentions — trivial cleanup from the 4eo
      note), and all other survivors per prior rounds.
      Local-only safety: unreachable-conversion deletion means zero behavior
      change — no `dyn ServerObject` value can exist anywhere (the only
      would-be producers are `clone_box`, itself callerless, and coercion,
      which requires naming the type — zero sites), so both impls could never
      execute; cloud-object persistence/sync (the live `GenericServerObject`
      field-plumbing path), drive, terminal, tabs, panes, BYOK AI, settings,
      themes, and all other local features untouched; only two
      never-instantiable trait impls are gone.

      Acceptance: clippy baseline captured at HEAD FIRST in both configs
      (12 sorted warning+location pairs each, 14 `^warning` lines each — the
      12 pre-existing warnings: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the edit,
      `-p warp --lib --all-targets` is warning-identical to the baseline in
      BOTH configs (default and `--no-default-features --features simplewarp`
      — sorted-pair diffs empty, 14 `^warning` lines each, no unused-import
      or dead-code warnings). All 7 checks exit 0 (`check -p cloud_objects
      --all-targets` ± `--all-features`, `check -p warp --lib --all-targets`
      both feature sets, `--no-default-features --features simplewarp --bin
      simplewarp`, `--bin warp-oss`, `--all-targets -p integration`) with 0
      errors and only the two pre-existing `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`, observed
      in the integration check); format clean (`./script/format`, diff still
      exactly +0/−20). Nextest `-p warp --lib --no-fail-fast`: 4,652
      simplewarp passed / 4,653 default passed, 4 skipped each, 0 failed
      (exactly the baseline, zero tests added or removed, no flakes).
      Runtime smoke test SKIPPED: the user is away and nobody can answer the
      macOS password prompt, so per the 2026-09-23 convention change the GUI
      binary was not built or launched — unit tests plus checks are the
      acceptance for this round. Did not `cargo clean`.

- [x] **`ServerObject` trait (4eq) — DONE 2026-09-23.** The
      4ep-designated next slice: the `ServerObject` trait itself at
      `server_object.rs:32-46` (pre-edit) plus its only impl, mirroring
      the `TeamKind` precedent (4ed) — payload/support item deleted after
      its consumers went, with the survivor degenerating. Fresh
      re-verification trace (files shift between rounds; ledger /
      `schema.graphql` / fixtures excluded): digit-guarded word-boundary
      `git grep -P '(?<![A-Za-z0-9_])ServerObject(?![A-Za-z0-9_])' --
      '*.rs'` zero hits outside `server_object.rs` — every external
      substring hit is a different item (`ServerObjectModel`,
      `GenericServerObject`, `ServerObjectGuest`,
      `ServerObjectContainer`); `dyn ServerObject` only inside the file
      (`:45` trait-def `clone_box` return, `:117` impl override — both in
      deletion scope) plus `plan.md` ledger mentions; `impl ServerObject
      for` only the `GenericServerObject` block at `:104` (pre-edit),
      bounded impl-form grep zero elsewhere; `ServerObject::` path form
      zero in `*.rs` (hits all `GenericServerObject::` substrings);
      `as ServerObject` renames zero; `use` statements only
      `ServerObjectModel`; generic bounds `: ServerObject` (non-Model)
      zero; non-Rust files (`schema.graphql`, `*.json`, `*.toml`,
      `*.wgsl`) zero. No method call resolves through the trait: the
      consumers (`generic_cloud_object.rs:136-160`
      `new_from_server`/`update_from_server_object`, the `persistence.rs`
      readers) only read `GenericServerObject` fields
      (`.id`/`.model`/`.metadata`/`.permissions`), and no
      `dyn ServerObject` value can exist (4ep). `GenericServerObject`
      itself stays live via the `cloud_object_models` type aliases
      (`server_cloud_object.rs:278+` turbofish sites,
      `app/src/search/ai_context_menu/rules/data_source_tests.rs:32`).
      Deleted the trait definition with its doc comment (including the
      callerless `as_any` / `clone_box` methods — 4ep's designated
      next-round candidates, subsumed here), the `impl ServerObject for
      GenericServerObject` block, and the now-unused
      `use std::any::Any;` (`as_any` was its only user) (1 file, +0/−35;
      `server_object.rs` 120 → 85 lines). `ObjectType` / `Debug` /
      `PhantomData` / `Arc` / `SyncId` imports all stay (used by the
      surviving `ServerObjectModel`, `ConflictStatus`, and
      `GenericServerObject` items). Deliberately left:
      `GenericServerObject` (LIVE — the 37 `cloud_object_models` aliases
      all expand to it or its siblings, field-plumbed by persistence),
      the `cloud_object_models` aliases (LIVE), `ServerObjectModel` and
      its four impls (`CloudFolderModel` / `CloudNotebookModel` /
      `CloudWorkflowModel` / `GenericStringModel` — LIVE per handoff;
      note for the survey: the trait method `object_type`'s only in-crate
      caller was the deleted impl block, so its four impl bodies are now
      caller-facing zero — needs its own dual-confirm slice, never in
      this round), `ConflictStatus` (live via `generic_cloud_object.rs:40`
      + `app/src/cloud_object/mod.rs:599-625`), and everything else per
      prior rounds. Designated NEXT: the empty
      `cloud_object/models/mod.rs` module (re-verified this round: 1-byte
      file, `pub mod models;` at `cloud_object/mod.rs:29`, zero
      `cloud_object::models` path mentions repo-wide — trivial cleanup);
      after that a FRESH full survey of `crates/cloud_objects` (ids.rs,
      drive/, cloud_object/mod.rs, auth/, env_vars-related remnants, plus
      the `ServerObjectModel::object_type` caller question above); if the
      survey finds the crate clean, the ledger says so and the effort's
      next major item is the cloud-run lifecycle walls per the 4ca plan —
      NOT started this round. Local-only safety: zero external mentions
      means zero behavior change — no code outside the deleted file could
      name the trait, produce a `dyn ServerObject`, or dispatch through
      it, so cloud-object persistence/sync (the live
      `GenericServerObject` field-plumbing path), drive, terminal, tabs,
      panes, BYOK AI, settings, themes, and all other local features are
      untouched; only an unnameable trait and its single degenerate impl
      are gone.

      Acceptance: clippy baseline captured at HEAD FIRST in both configs
      (16 sorted warning+location pair-lines each, 14 `^warning` lines
      each — the 12 pre-existing warnings: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the edit,
      `-p warp --lib --all-targets` is warning-identical to the baseline
      in BOTH configs (default and `--no-default-features --features
      simplewarp` — sorted-pair diffs empty, 14 `^warning` lines each, no
      unused-import or dead-code warnings). All 7 checks exit 0 (`check
      -p cloud_objects --all-targets` ± `--all-features`, `check -p warp
      --lib --all-targets` both feature sets, `--no-default-features
      --features simplewarp --bin simplewarp`, `--bin warp-oss`,
      `--all-targets -p integration`) with 0 errors and only the two
      pre-existing `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`,
      observed in the integration check); format clean
      (`./script/format`, diff still exactly +0/−35). Nextest `-p warp
      --lib --no-fail-fast`: 4,652 simplewarp passed / 4,653 default
      passed, 4 skipped each, 0 failed (exactly the baseline, zero tests
      added or removed, no flakes). Runtime smoke test SKIPPED: the user
      is away and nobody can answer the macOS password prompt, so per the
      2026-09-23 convention change the GUI binary was not built or
      launched — unit tests plus checks are the acceptance for this
      round. Did not `cargo clean`.

- [x] **empty `cloud_object::models` module (4er) — DONE 2026-09-23.**
      The 4eq-designated next slice: the declared-but-empty module at
      `crates/cloud_objects/src/cloud_object/models/mod.rs`.
      Verification (ledger excluded): the file at HEAD is exactly one
      byte (blob `8b1378917` = a single newline — zero items), and
      `git ls-tree -r HEAD` on the directory shows `mod.rs` was its only
      file; exact-path searches all zero in `*.rs` —
      `cloud_object::models`, `crate::cloud_object::models`,
      `use.*cloud_object::models`, `super::models` inside
      `cloud_object/`, and `models::` anywhere in
      `crates/cloud_objects/`; the only repo-wide hits for
      `cloud_object::models` are the `plan.md` ledger mentions; the
      crate's package name is plain `cloud_objects` (no `warp_` prefix,
      so no differently-prefilled external path), and the only `models`
      word hits in the crate are prose (`server_object.rs:25` doc,
      `lib.rs:4` crate doc). Deleted the 1-byte file and its
      `pub mod models;` declaration at `cloud_object/mod.rs:29` (2
      files, +0/−2). Deliberately left: everything else — this round's
      deletion is Part 1 only; Part 2 is the survey below. Local-only
      safety: an empty module contributes no code, so deleting it is
      zero behavior change by construction — cloud-object
      persistence/sync, drive, terminal, tabs, panes, BYOK AI,
      settings, themes, and all other local features untouched; only an
      empty declaration is gone.

      Acceptance: clippy baseline captured at HEAD FIRST in both
      configs (12 sorted warning+location pairs each, 14 `^warning`
      lines each — the 12 pre-existing warnings: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`; the worktree already
      held this round's edits from an interrupted attempt, so the HEAD
      baseline was captured via `git stash push` / clippy / `git stash
      pop`); after the edit, `-p warp --lib --all-targets` is
      warning-identical to the baseline in BOTH configs (sorted-pair
      diffs empty, 14 `^warning` lines each). All 7 checks exit 0
      (`check -p cloud_objects --all-targets` ± `--all-features`,
      `check -p warp --lib --all-targets` both feature sets,
      `--no-default-features --features simplewarp --bin simplewarp`,
      `--bin warp-oss`, `--all-targets -p integration`) with 0 errors
      and only the two pre-existing `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`,
      observed in the integration check); format clean
      (`./script/format`, no diff beyond the deletion — diff remains
      exactly +0/−2). Nextest `-p warp --lib --no-fail-fast`: 4,652
      simplewarp passed / 4,653 default passed, 4 skipped each, 0
      failed (exactly the baseline, zero tests added or removed, no
      flakes). Runtime smoke test SKIPPED: the user is away and nobody
      can answer the macOS password prompt, so per the 2026-09-23
      convention change the GUI binary was not built or launched —
      unit tests plus checks are the acceptance for this round. Did
      not `cargo clean`.

      Survey (Part 2): fresh full sweep of `crates/cloud_objects`
      (ids.rs, drive/mod.rs, drive/sharing.rs, cloud_object/mod.rs,
      generic_cloud_object.rs, generic_string_model.rs,
      server_object.rs, creation.rs, auth/, lib.rs), bare-name +
      call-syntax + path-form per candidate (ledger / `schema.graphql`
      / fixtures excluded). Candidates and verdicts:

      - `ServerObjectModel` trait + its four impls — DEAD, whole-trait
        slice, DESIGNATED NEXT. The trait (`server_object.rs:26-29`,
        sole method `fn object_type(&self) -> ObjectType`) is named in
        `*.rs` ONLY at its def, its four impl blocks
        (`CloudFolderModel` at `cloud_object_models/src/folder.rs:30`,
        `CloudNotebookModel` at `notebook.rs:28`,
        `CloudWorkflowModel` at `workflow.rs:389`,
        `GenericStringModel<M, S>` at
        `cloud_objects/cloud_object/generic_string_model.rs:48`), and
        the four impl-side `use` lines — zero generic bounds
        (`: ServerObjectModel` outside impl headers), zero
        `dyn ServerObjectModel`, zero `ServerObjectModel::` path
        forms, zero use-site imports elsewhere. Every `.object_type()`
        call site repo-wide (20) triaged by receiver, NONE resolves
        through `ServerObjectModel`: 17 through the app-side
        `CloudObject` trait (`telemetry.rs:27`,
        `drive_object_type.rs:153`, app `cloud_object/mod.rs`
        546/647/677/788, `model/persistence.rs` 370/1516/1752,
        `toast_message.rs:19`, `drive/index.rs` 713/3689/3771/4288,
        `workflows/modal.rs:594`, `update_manager.rs:514`,
        `workspace/view.rs:7133`), one through the app-side
        `CloudModelType` trait (app `cloud_object/mod.rs:578`
        `self.model().object_type()` under an `M: CloudModelType`
        bound), one inherent `CloudObjectTypeAndId::object_type`
        (`agent_profiles_page.rs:355`), one `WarpDriveItem`-family
        `Option<DriveObjectType>` (`warp_drive_item.rs:33`). The four
        model types implement BOTH `ServerObjectModel` and the
        app-side `CloudModelType` with identical bodies
        (`ObjectType::Folder`/`Notebook`/`Workflow`/
        `GenericStringObject(S::model_format())`), and method
        resolution at every site lands on the only in-scope trait —
        the `ServerObjectModel` impls are unreachable and the trait
        unnameable outside its impl files. Same class as 4eq's
        `ServerObject` trait: this sharpens the 4eq handoff's "impl
        bodies caller-facing zero" into full trait deadness — delete
        the trait + all four impls + the `ObjectType`/`ServerObjectModel`
        import parts in the four impl files (per-file `ObjectType`
        usage to be re-checked at edit time).
      - `GenericStringModel::json_model`
        (`generic_string_model.rs:42-44`) — DEAD, own slice after the
        above. `.json_model(` zero repo-wide; `::json_model` only
        unrelated module paths (app's own `model::json_model`,
        `cloud_object_models::json_model`); bare word only module
        names. Inherent method in no trait, receiver reachable only
        through aliases that never call it.
      - `cloud_objects::auth` re-exports (`auth/mod.rs:1-2`) —
        `TEST_USER_EMAIL`/`TEST_USER_UID` have zero importers through
        `cloud_objects::auth` (app uses app-local
        `crate::auth::user::*`, `warp_server_auth` and
        `warp_server_client` their own paths), and the
        `pub use warp_server_auth::user_uid;` module re-export is
        likewise unused; the `UserUid` part of line 1 IS live
        (`sharing.rs:3` + `lib.rs` `pub use auth::UserUid` →
        `cloud_object_models/user_profile.rs:1`,
        `cloud_object_persistence/objects.rs:3`). Own slice: shrink
        line 1 to `UserUid`, delete line 2.
      - Trace-only, no verdict:
        `impl settings_value::SettingsValue for SyncId`
        (`ids.rs:104`) and `impl PartialEq for GenericCloudObject`
        (`generic_cloud_object.rs:54-61`) — inference-reachable trait
        impls; each needs its own 4em/4eo-class resolution trace
        before any judgment.
      - Everything else LIVE, re-verified with hits: sharing.rs
        (`can_move_drive` via `drive/index.rs:2304`, `is_user` via
        `cloud_object/mod.rs:482-483` + `view.rs:180`, `Subject`/
        `UserKind` structurally live); ids.rs (`HashableId`,
        `ClientId` + methods, `SyncId::uid`/`sqlite_uid_hash`/
        `into_server`/`into_client`, `ServerId::from_string_lossy`/
        `uid`/`sqlite_type_and_uid_hash`/both `TryFrom`s/
        `From<ServerId> for String`/test `From<i64>`,
        `parse_sqlite_id_to_uid` via
        `actions.rs:146`, `ServerIdAndType` +
        `sqlite_type_and_uid_hash` via
        `cloud_object_persistence/objects.rs:474,485`,
        `server_id_traits!` via cloud_object_models, `FolderId`,
        `GenericStringObjectId` (turbofish-wide; its inherent `uid`
        deferred to a receiver-typed trace — path form zero, nearby
        `.uid()` receivers are all `SyncId`)); drive/mod.rs
        (`CloudObjectTypeAndId` — all 13 methods hit: `uid`/`sync_id`/
        `sqlite_uid_hash` via `persistence.rs:329` +
        `update_manager.rs:789,840`, `object_type` via
        `agent_profiles_page.rs:355`, `object_id_type` via
        `persistence.rs:330,364`, `has_server_id` via
        `facts/view/mod.rs:337,346` + `drive/index.rs:948,958`,
        `as_folder_id` via `drive/index.rs:2338`, `as_notebook_id`
        via notebook sites, `as_generic_string_object_id` via env_var
        sites, `drive_row_position_id` via 5 sites, `from_id_and_type`
        via `drive/index.rs:2776` + notebooks/workflows,
        `from_generic_string_object` via env_var menus +
        `workspace/view.rs`); cloud_object/mod.rs (every pub item:
        `ObjectIdType::sqlite_prefix`, `ObjectType::
        sqlite_object_type_as_str` via
        `cloud_object_persistence/objects.rs:119,248`, both
        `*_PREFIX` consts, `GenericStringObjectFormat`,
        `GenericStringObjectUniqueKey` + `UniquePer` via the app ai
        crates, `JsonObjectType` + `as_str` (internal at `:170`),
        `Revision` — all five methods incl. `timestamp` via
        `search/ai_context_menu/rules/data_source.rs:46-47` and
        `utc` via app `cloud_object/mod.rs:826` + `view.rs:109`,
        `Owner` + `mock_current_user`, `ServerObjectContainer`,
        `ServerGuestSubject`/`ServerLinkSharing`/`ServerObjectGuest`
        (TryFroms live per 4eo), `ServerMetadata`,
        `ServerPermissions::mock_personal`, `NumInFlightRequests`,
        `CloudObjectSyncStatus`, `CloudObjectPermissions` — all four
        methods incl. `update_from_new_permissions_ts` via
        `persistence.rs:743`, `CloudLinkSharing`/`CloudObjectGuest`,
        `CloudObjectMetadata` — all methods incl.
        `update_from_new_metadata_ts` via `persistence.rs:608`,
        `CloudObjectStatuses::mock` + `render_icon` via six
        drive-item receivers (`ai_fact.rs:96`, `folder.rs:76`,
        `notebook.rs:94`, `workflow.rs:117`,
        `env_var_collection.rs:164`, `mcp_server.rs:61`),
        `CloudObjectEventEntrypoint`, `SerializedModel`
        `new`/`model_as_str`/`take` (`sqlite.rs:2152`),
        `RevisionAndLastEditor`); generic_cloud_object.rs
        (`model`/`shared_model`/`set_model`/`new`/`new_local`/
        `new_from_server`/`update_from_server_object`/`upsert_params`
        and the `From<CloudObjectUpsertParams>` impl — 4ei's
        alias/turbofish lesson re-applied, 8 alias-qualified
        `CloudFolder::from(params)`-style sites live);
        generic_string_model.rs (`Serializer` trait +
        `new`/`deserialize_owned` — live through ALIAS-qualified
        calls (`CloudPreferenceModel = GenericStringModel<Preference,
        JsonSerializer>` etc.): `json_model/persistence.rs:58-102` +
        `profiles_tests.rs:63`; `json_model` itself is the dead
        sibling above); server_object.rs (`ConflictStatus` +
        `has_conflicts` via `generic_cloud_object.rs:148` + app
        `cloud_object/mod.rs:594`, `GenericServerObject` +
        `new`/`Clone`/`Debug`); creation.rs (`ServerCreationInfo`
        live per 4en); auth (the `UserUid` re-export, live).
        Designated NEXT after the `ServerObjectModel` slice: if the
        crate then shows clean at re-survey, the effort pivots to the
        next major item (the cloud-run lifecycle walls per the 4ca
        plan) — not started this round.

- [x] **`ServerObjectModel` trait + four impls (4es) — DONE 2026-09-23.**
      The 4er-survey-designated slice, 4eq's whole-trait deadness class
      scaled up to four impls. Verification (fresh at HEAD; ledger /
      schema / fixtures excluded): bare-name
      `git grep -n ServerObjectModel -- '*.rs'` hits exactly the trait
      def with its two doc lines (`server_object.rs:25-29`), the four
      impl blocks (`for CloudFolderModel` at
      `cloud_object_models/src/folder.rs:30`, `for CloudNotebookModel`
      at `notebook.rs:28`, `for CloudWorkflowModel` at
      `workflow.rs:389`, `impl<M, S> for GenericStringModel<M, S>` at
      `cloud_objects/cloud_object/generic_string_model.rs:48`; all line
      numbers pre-edit), and the four impl-side `use` lines — zero
      bounds (`: ServerObjectModel`, `ServerObjectModel>`,
      `ServerObjectModel +`), zero `dyn`, zero `ServerObjectModel::`
      path forms, no other importers. All 20 `.object_type()` call
      sites resolve to other traits (17 app-side `CloudObject`, 1
      `CloudModelType` at app `cloud_object/mod.rs:578`, 1 inherent
      `CloudObjectTypeAndId::object_type` at
      `agent_profiles_page.rs:355`, 1 `WarpDriveItem`), and the four
      model types carry identical-bodied `CloudModelType` impls (app
      `cloud_object/folders.rs:17`, `notebooks/mod.rs:28`,
      `workflows/mod.rs:182`,
      `cloud_object/model/generic_string_model.rs:138`) — resolution
      never landed on `ServerObjectModel`, so the deleted impls were
      unreachable and removing them cannot change any site's value.
      Import dispositions: `ObjectType` dropped entirely from all five
      affected use lines (per-file grep: it appeared only on the
      deleted def/impl lines — also in server_object.rs itself);
      retained parts re-verified in use (`GenericCloudObject`/
      `GenericServerObject` for the aliases in the three model files,
      `GenericStringObjectFormat`/`SerializedModel` for the
      `Serializer` trait signature, `ServerMetadata`/
      `ServerPermissions` for the struct fields). Deleted the trait
      def with its doc comment and all four impl blocks (5 files,
      +5/−45). One-hop: all four model types stay live (app-side
      `CloudModelType` impls + the `cloud_object_models` type aliases
      + persistence readers); `ObjectType`, `Serializer` (still
      implemented by `JsonSerializer`; the app-side sibling trait
      carries its own `model_format`), `GenericStringObjectFormat`,
      `SerializedModel` all keep users. Newly orphaned: cloud-objects'
      `Serializer::model_format` loses its only in-repo dispatch site
      (the deleted `S::model_format()`) — public trait method,
      clippy-silent; next-round candidate, not deleted here.
      Deliberately left: `GenericServerObject`/`ConflictStatus` (LIVE
      per 4eq/4er), the `cloud_object_models` aliases (LIVE),
      everything else per the 4er survey. Designated NEXT:
      `GenericStringModel::json_model` (`generic_string_model.rs:42-44`
      pre-edit; `.json_model(` zero repo-wide), then the
      `cloud_objects::auth` re-exports (`TEST_USER_EMAIL`/
      `TEST_USER_UID` + the `pub use warp_server_auth::user_uid;`
      module re-export, zero importers via `cloud_objects::auth`; the
      `UserUid` part of line 1 is live), then the trace-only items
      (`impl SettingsValue for SyncId`, `impl PartialEq for
      GenericCloudObject` — 4em/4eo-class inference traces). Local-only
      safety: zero external mentions means zero behavior change — no
      code outside the five edited files could name the trait, and
      every `.object_type()` site already resolved through the
      surviving traits, so cloud-object persistence/sync, drive,
      terminal, tabs, panes, BYOK AI, settings, themes, and all other
      local features are untouched.

      Acceptance: clippy baseline captured at HEAD FIRST in both
      configs (14 `^warning` lines each = the 12 pre-existing warnings
      + 2 crate summaries; the worktree already held this round's
      edits from an interrupted attempt, so the HEAD baseline was
      captured via `git stash push` / clippy both configs / `git stash
      pop`, and the popped diff was re-verified against a from-scratch
      grep before continuing); after the edit, `-p warp --lib
      --all-targets` is warning-identical to the baseline in BOTH
      configs (sorted warning+location-pair diffs empty, 14 `^warning`
      lines each). All 7 checks exit 0 (`check -p cloud_objects
      --all-targets` ± `--all-features`, `check -p warp --lib
      --all-targets` both feature sets, `--no-default-features
      --features simplewarp --bin simplewarp`, `--bin warp-oss`,
      `--all-targets -p integration`) with 0 errors and only the two
      pre-existing `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`,
      observed in the integration check). Format clean
      (`./script/format`; diff remains exactly +5/−45 across the five
      files). Nextest `-p warp --lib --no-fail-fast`: 4,652 simplewarp
      passed / 4,653 default passed, 4 skipped each, 0 failed (exactly
      the baseline, no flakes). Runtime smoke test SKIPPED: the user
      is away and nobody can answer the macOS password prompt, so per
      the 2026-09-23 convention change the GUI binary was not built or
      launched — unit tests plus checks are the acceptance for this
      round. Did not `cargo clean`.

- [x] **`json_model` + auth re-exports + `Serializer::model_format` cluster
      (4et) — DONE 2026-09-23.** Three independently dual-confirmed
      zero-caller items in one round (the 4dw cluster precedent), all
      designated by the 4es handoff. Verification fresh at HEAD; ledger /
      `schema.graphql` / fixtures excluded. (1)
      `GenericStringModel::json_model` (`generic_string_model.rs:43-45`
      post-4es): call-syntax `.json_model(` zero in `*.rs`; path-form
      `::json_model` only unrelated module paths (the app's own
      `cloud_object::model::json_model` module, the
      `cloud_object_models::json_model` module); bare name only those module
      decls/imports plus the definition; zero turbofish/qualified forms
      (`<..>::json_model`, `>::json_model` — 4ei lesson re-applied). Inherent
      method in no trait, receiver reachable only through aliases that never
      call it. Deleted (4 lines incl. blank). (2) `cloud_objects::auth`
      re-exports: `TEST_USER_EMAIL` has zero importers through
      `cloud_objects::auth` — every other use is another path (app-local
      `crate::auth::user::*`, `warp_server_auth` internal
      `super::user_uid`, `warp_server_client`'s own separate re-export, all
      untouched); the `pub use warp_server_auth::user_uid;` module re-export
      has zero importers (`auth::(user_uid|TEST_USER)` hits only the def
      line, the in-crate `TEST_USER_UID` use, and warp_server_client's own
      line; zero `cloud_objects::auth` path mentions and zero glob imports
      repo-wide). BUT `TEST_USER_UID` is LIVE inside the crate —
      `cloud_object/mod.rs:328` imports `crate::auth::TEST_USER_UID` inside
      `#[cfg(any(test, feature = "test-util"))]`
      `Owner::mock_current_user` — so it stays (the consts are
      always-compiled pub consts in `warp_server_auth/src/user_uid.rs:6-7`,
      not test-gated at the source). Line 1's `user_uid::` prefix resolved
      through line 2 (no local `auth/user_uid.rs` — the dir holds only
      `mod.rs`), so the file collapsed to one line:
      `pub use warp_server_auth::user_uid::{TEST_USER_UID, UserUid};`
      (+1/−2). (3) `Serializer::model_format` (orphaned by 4es' deletion of
      its only dispatch, `S::model_format()`): exhaustive trace —
      `.model_format(` zero in `*.rs`; `::model_format` exactly 5 sites, all
      triaged — 3 resolve to the app-side `StringModel::model_format` (app
      `cloud_object/model/generic_string_model.rs:115,157,162`, live), 2 are
      the override bodies inside the trait's only two impls (exhaustive
      `impl[^;{]*Serializer[^;{]*for` sweep: `JsonSerializer` at
      `cloud_object_models/src/json_model.rs:27` and app
      `cloud_object/model/json_model.rs:18`), each delegating to its own
      `JsonModel`/`StringModel`; no `<.. as Serializer..>::model_format`
      qualified forms. Deleted the trait-method decl
      (`generic_string_model.rs:10`), both overrides, and the two
      now-unused `GenericStringObjectFormat` import parts (the type itself
      stays live — `cloud_object/mod.rs` match arms/enum, `drive/mod.rs`
      fields). Total: 4 files, +3/−16. Newly orphaned by this round, next
      sweep candidate (not deleted here):
      `cloud_object_models::json_model::JsonModel::model_format` default
      body (`json_model.rs:19-21`) — its only caller was the deleted
      override at :29 (its `json_object_type()` dependency stays live via
      `json_model/persistence.rs`). Deliberately left: `TEST_USER_UID` +
      the `UserUid` re-export (live, evidence above); app-side
      `StringModel::model_format` and its ten overrides (live through
      :115/:157/:162); app `JsonModel` (inherits `StringModel::model_format`;
      its `json_object_type` live via the env_vars/cloud_preferences/
      workflow_enum override bodies); both `JsonSerializer` impls and the
      `Serializer` trait (`serialize` via app generic_string_model.rs:149,
      `deserialize_owned` via `GenericStringModel::deserialize_owned` →
      `json_model/persistence.rs` per 4er); the `#[allow(dead_code)]`
      app-side `JsonSerializer` struct (out of scope). Local-only safety:
      every deleted item had zero reachable callers — an inherent getter
      never invoked, names no crate could import through
      `cloud_objects::auth`, and a trait method whose only dispatch was
      already deleted in 4es with every call site resolving to surviving
      traits — so cloud-object persistence/sync, drive, terminal, tabs,
      panes, BYOK AI, settings, themes, and all other local features are
      untouched.

      Acceptance: clippy baselines captured at HEAD FIRST (worktree clean,
      no stash needed) in both configs — 12 sorted warning+location pairs /
      14 `^warning` lines each (the 12 pre-existing warnings: 11
      unneeded-return in `app/src/terminal/input.rs` + 1
      single-element-loop in `terminal/model/lifecycle/mod_tests.rs:277`);
      after the edit, `-p warp --lib --all-targets` is warning-identical to
      the baseline in BOTH configs (sorted-pair diffs empty, 12 pairs / 14
      `^warning` lines each). All 7 checks exit 0 (`check -p cloud_objects
      --all-targets` ± `--all-features`, `check -p warp --lib
      --all-targets` both feature sets, `--no-default-features --features
      simplewarp --bin simplewarp`, `--bin warp-oss`, `--all-targets -p
      integration`) with 0 errors and only the two pre-existing `step.rs`
      unused-import warnings (`single_terminal_view_for_tab`,
      `crate::terminal::CLIAgent`, observed in the integration check).
      Format clean (`./script/format`; diff remains exactly +3/−16 across
      the four files). Nextest `-p warp --lib --no-fail-fast`: 4,653
      default passed, 4 skipped, 0 failed; 4,652 simplewarp — first run
      4,651 + the known `test_command_block_dispatches_event` load flake,
      rerun clean 4,652 passed, 4 skipped, 0 failed. Runtime smoke test
      SKIPPED: the user is away and nobody can answer the macOS password
      prompt, so per the 2026-09-23 convention change the GUI binary was
      not built or launched — unit tests plus checks are the acceptance
      for this round. Did not `cargo clean`.

      Designated NEXT: the two trace-only inference impls from the 4er
      survey — `impl SettingsValue for SyncId` (`ids.rs:104`) and
      `impl PartialEq for GenericCloudObject`
      (`generic_cloud_object.rs:54-61`) — each needs a 4em/4eo-class
      exhaustive `.into()`/`==`-resolution trace before any judgment;
      after those, the crate-clean verdict + the cloud-run-lifecycle pivot
      per the 4ca plan (not started).

- [x] **inference-impl traces + `JsonModel::model_format` (4eu) — DONE
      2026-09-23.** The 4et-designated round: both trace-only inference
      impls from the 4er survey got their 4em/4eo-class resolution traces
      (both found LIVE — recorded here, not deleted), plus the 4et-noted
      newly-orphaned default method deleted. (1) `impl
      settings_value::SettingsValue for SyncId` (`ids.rs:104`, empty
      serde-passthrough impl of the trait at
      `crates/settings_value/src/lib.rs:63`, whose own crate doc names
      SyncId as the passthrough example) — LIVE, structurally required by
      the settings machinery: `app/src/ai/cloud_agent_settings.rs:8-14`
      registers the live `CloudAgentSettings` group with setting
      `last_selected_environment_id { type: Option<SyncId> }` where
      `SyncId` is `crate::server::ids::SyncId` = re-export of
      `cloud_objects::ids::SyncId` (`app/src/server/ids.rs:8`;
      `warp_server_client/src/ids.rs:1` glob-re-exports the same type);
      `define_settings_group!` emits `type Value = Option<SyncId>`
      (`crates/settings/src/macros.rs:541`), and `Setting`'s
      `type Value: Serialize + DeserializeOwned + PartialEq + Debug +
      SettingsValue` bound (`crates/settings/src/lib.rs:299`) forces
      `Option<SyncId>: SettingsValue` through the collection impl
      `impl<T: SettingsValue> SettingsValue for Option<T>`
      (`settings_value/src/lib.rs:148`), whose `to_file_value`/
      `from_file_value` dispatch DIRECTLY into the SyncId impl — reached
      at runtime by the settings-file load/save paths
      (`lib.rs:509,560,583`), the default-value file serialization at
      registration (`macros.rs:860-862`), and the update callback
      (`macros.rs:877-884`); the setting itself is live
      (`catalog.rs:84-91` `persist_selection` → `set_value(Some(...))`,
      `:106-108` `saved_environment_id`, registered in
      `app/src/settings/init.rs:24`). Deleting the impl cannot compile.
      (2) `impl<K, M> PartialEq for GenericCloudObject<K, M> where M:
      PartialEq` (`generic_cloud_object.rs:54-61`) — LIVE, found by the
      EMPIRICAL delete-and-check (the conclusive method for PartialEq:
      not object-safe, no dyn dispatch, every use is a compile-time
      inference target). Exhaustive textual sweep found zero callers:
      word-guarded `==`/`!=` with any of the 13 `GenericCloudObject`
      alias operands zero; `.eq(`/`.ne(` all diesel DSL columns or UI
      editor handles; `.contains(&` all uid/path/flag collections;
      `.dedup()` zero on these collections; `assert_eq!`/`assert_ne!`
      all compare inner `.name` strings/ids; all PartialEq-consuming
      closures (`find`/`position`/`any`) compare ids/strings; the
      containers that hold the aliases (`WarpDrive*` items, search
      items, `CloudRuleRow`, `RuleEditorView`) derive only
      `Clone`/`Debug` or nothing; `catalog.rs:96`'s
      `environments != self.environments` is an app-local projection
      struct with its own derived PartialEq; `CloudObject` trait is
      `: Debug` only. But the temporary deletion made
      `cargo check -p warp --lib --all-targets` fail with exactly two
      E0369 errors — derive-macro expansions: `#[derive(PartialEq)]`
      `EnvVarCollectionType::Cloud(Box<CloudEnvVarCollection>)`
      (`app/src/env_vars/mod.rs:22-26`) and `#[derive(PartialEq)]`
      `WorkflowType::Cloud(Box<CloudWorkflow>)`
      (`app/src/workflows/mod.rs:116-121`) — the derived eq forwards
      through std's `impl<T: PartialEq> PartialEq for Box<T>` into the
      GenericCloudObject impl. This extends the 4ei alias/turbofish
      lesson: DERIVE-FORWARDED trait resolution through alias-qualified
      payload types is invisible to every operator grep; only the
      compiler sees it. Impl restored byte-identical; the derives stay
      (live enums, removing them would be a semantics change, out of
      scope). (3) `cloud_object_models::json_model::JsonModel::
      model_format` default body (`json_model.rs:19-21` pre-edit) —
      DELETED. Fresh exhaustive sweep at HEAD: `.model_format(` zero in
      `*.rs`; `::model_format` exactly 14 sites, all triaged — the
      required-method decl + 3 live `M::model_format()` dispatches in
      the app-side `StringModel` machinery
      (`app/src/cloud_object/model/generic_string_model.rs:55,115,157,162`)
      and 10 override bodies under `impl StringModel for X` /
      `impl JsonModel for X` in the app's ai/env_vars/settings modules
      (resolving to the app-side trait, per 4et); the only
      cloud_object_models mention was the default decl itself — its
      only caller was the `Serializer<M> for JsonSerializer` override
      4et already deleted, and the app twin
      (`app/src/cloud_object/model/json_model.rs`) never had a
      `model_format` (it inherits the live `StringModel::model_format`),
      nothing to delete there. Deleted the default method + its doc
      comment and shrank the now-partially-unused import
      (`GenericStringObjectFormat` part; `JsonObjectType`/
      `SerializedModel`/`Serializer` stay for `json_object_type` and
      the JsonSerializer impl) (1 file, +1/−8). Survey (Part 2),
      crate-clean verdict: crates/cloud_objects has exactly ONE known
      candidate left — `GenericStringObjectId::uid` inherent method
      (`ids.rs:419`, the 4er deferral; this pass found every `.uid()`
      receiver resolving to SyncId / CloudObjectTypeAndId / the
      CloudObject trait, and the sole `GenericStringObjectId::from_hash`
      value site converts to `ServerId`→`SyncId` without calling it —
      but it needs its own receiver-typed dual-confirm slice, never
      this round); the two inference impls are now resolved LIVE, so
      the crate is otherwise clean. crates/cloud_object_models gained
      def-only leaf candidates for future slices (bare-name single-hit
      = definition only, PCRE-guarded; the `\b` grep on this host
      silently returns zero hits and must not be trusted):
      `WorkflowModel::author_name` (workflow.rs:106; only other hit is
      the string literal `"author_name"` in workflow_tests.rs:125),
      `MCPServer::from_stored_json` (mcp.rs:216),
      `AIFact::is_memory` (ai_fact.rs:52),
      `AgentConfig::to_ambient_config` (cloud_agent_config.rs:33),
      `Workflow::from_harness_type` /
      `Workflow::get_enum_ids` /
      `Workflow::is_command_workflow` /
      `Workflow::replace_object_id` (workflow.rs),
      `AIExecutionProfile::is_always_ask` (ai_execution_profile.rs),
      `ScheduledAmbientAgent::from_harness_type`
      (scheduled_ambient_agent.rs); the models' persistence fns
      re-verified LIVE (`delete_folder`/`delete_notebook`/
      `delete_workflow` wired as fn pointers at
      `app/src/persistence/sqlite.rs:3169` family, `upsert_folders`/
      `upsert_notebooks`/`upsert_workflows` at sqlite.rs:648,665
      family, `get_init_command_for_env_var_value` +
      `serialize_variables_internal` internally called at
      env_vars.rs:137,165). Deliberately left: both inference impls
      (LIVE, evidence above), all 13 `GenericCloudObject` aliases and
      their containers, the settings machinery, the app-side
      `StringModel::model_format` + its ten overrides (live), both
      `JsonSerializer` impls and both `JsonModel` traits (live),
      `GenericStringObjectId::uid` (designated next candidate), the
      nine cloud_object_models def-only leaves (each its own slice),
      ambient plumbing, telemetry scope (4ca item7), fold (item8),
      redesigns, and all local features. Local-only safety: the single
      deleted item is a default trait method whose only dispatch was
      already deleted in 4et and whose every remaining call form
      resolves to the surviving app-side trait, so cloud-object
      persistence/sync (generic-string objects serialize through
      `JsonSerializer::serialize`/`deserialize_owned`, untouched),
      settings persistence (env ids round-trip through the live
      SettingsValue path), drive, terminal, tabs, panes, BYOK AI,
      themes, and all other local features are untouched; only a
      never-dispatched default body is gone.

      Acceptance: clippy baselines captured at HEAD FIRST (worktree
      clean, no stash needed) in both configs — 12 sorted
      warning+location pairs / 14 `^warning` lines each (11
      unneeded-return in `app/src/terminal/input.rs` + 1
      single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the edits
      (including the PartialEq delete-restore experiment),
      `-p warp --lib --all-targets` is warning-identical to the
      baseline in BOTH configs (sorted-pair diffs empty, 12 pairs / 14
      `^warning` lines each). All checks exit 0 (`check -p
      cloud_objects --all-targets` ± `--all-features`, `check -p
      cloud_object_models --all-targets`, `check -p warp --lib
      --all-targets` both feature sets, `--no-default-features
      --features simplewarp --bin simplewarp`, `--bin warp-oss`,
      `--all-targets -p integration`) with 0 errors and only the two
      pre-existing `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`,
      observed in the integration check). Format clean
      (`./script/format`; diff remains exactly +1/−8). Nextest `-p
      warp --lib --no-fail-fast`: 4,653 default passed, 4 skipped, 0
      failed; 4,652 simplewarp — first run 4,651 + the known
      `test_command_block_dispatches_event` load flake, rerun clean
      4,652 passed, 4 skipped, 0 failed. Runtime smoke test SKIPPED:
      the user is away and nobody can answer the macOS password
      prompt, so per the 2026-09-23 convention change the GUI binary
      was not built or launched — unit tests plus checks are the
      acceptance for this round. Did not `cargo clean`. Next major
      item remains the cloud-run lifecycle walls per the 4ca plan —
      NOT started; the orchestrator decides whether to first exhaust
      the small candidates above.

- [x] **`GenericStringObjectId::uid` (4ev) — DONE 2026-09-23.** The 4eu
      survey's last known `crates/cloud_objects` candidate (the 4er
      deferral), deleted after a receiver-typed dual-confirm. The method
      (inherent, `&self` -> `ObjectUid`, sole member of its impl block at
      `ids.rs:418-422` pre-edit) wraps `self.0.uid()` = `ServerId::uid`.
      Verification (fresh at HEAD; ledger / `schema.graphql` / fixtures
      excluded; PCRE lookahead guards throughout — no `\b`, per the 4eu
      tooling note): path-form `GenericStringObjectId(?![A-Za-z0-9_])::
      uid` zero in `*.rs` (all-files hits only the plan ledger);
      `::uid(?![A-Za-z0-9_])` zero (no qualified/turbofish/re-export
      spellings — `warp_server_client::ids` and `app/src/server/ids`
      are re-exports of the same type, verified);
      `as GenericStringObjectId` zero. Call-syntax `.uid(` (~130 hits)
      triaged receiver by receiver, NONE a `GenericStringObjectId`:
      `SyncId` receivers (`SyncId::uid`'s own match arms `ids.rs:77`,
      `drive/mod.rs:34-37` — the `CloudObjectTypeAndId::GenericString
      Object { id: SyncId }` variant itself carries `SyncId`
      (`drive/mod.rs:15`), so its `id.uid()` is a `SyncId` call —
      `persistence.rs` `get_folder/get_workflow/get_workflow_enum/
      get_ai_execution_profile/get_object_of_type*` helpers all take
      `&SyncId`, `create_object_internal(id: SyncId..)`, `upsert_from_
      server_object`'s `server_object.id` (`GenericServerObject.id` is
      `SyncId`, `server_object.rs:28`), `LinkedWorkflowData::Id(SyncId)`
      (`history.rs:203`), `EnvVarCollectionSource::Existing(SyncId)` /
      `WorkflowOpenSource::Existing(SyncId)`, the workflows modal's
      `workflow_id: Option<SyncId>`, catalog's
      `orchestration_default_environment_id -> Option<SyncId>`,
      `HandleConflictingWorkflow/EnvVarCollection` payloads,
      `catalog_tests.rs` `SyncId::ClientId`, `ids_tests.rs:9`,
      model_tests `SyncId` params, `metadata().folder_id` (`Option<
      SyncId>`)); `ServerId` receivers (`root_view.rs:839/1594`
      `arg.server_id`, `workspace/view.rs:15321/15470` `result.server_id`,
      `to_server_id().uid()` chains at `embedded_item.rs:211` /
      `embedding_model.rs:186`, workspace Team `uid: ServerId` fields
      (`team.rs:87`) behind `sqlite.rs:2069/2087` +
      `cli_agent.rs:445`, `workflow_view.rs:526`
      `result.server_id.unwrap_or_default()`, update_manager ids guarded
      by `.server_id()`); `CloudObjectTypeAndId` receivers
      (`drive_row_position_id` internal, `panel.rs` RunObject/Invoke
      handlers, `item.rs:416/421` `WarpDriveItemId::Object(object_id)`,
      `index.rs:1141-1153` match arms, `update_manager.rs:267/712/933`,
      `export.rs:449` `ExportId(CloudObjectTypeAndId, _)` `.0`,
      `workspace/view.rs:14920` via the `:14877`
      `Option<CloudObjectTypeAndId>` param, `view.rs:310/343`); the
      app-side `CloudObject` trait method (objects_by_id
      `Box<dyn CloudObject>` values and `get_by_uid` results:
      `telemetry.rs:28`, `terminal/view.rs:18067`, `export.rs:102`,
      `drive/index.rs:726/1803/2402`, `item.rs:820`, model_tests
      `naive_active_object_uids`, `persistence.rs:125/159`); and
      `GenericCloudObject`'s `.id` FIELD — `pub id: SyncId`
      (`generic_cloud_object.rs:35`, not the `K` phantom param — its
      `CloudObject::uid` impl body `self.id.uid()` at app
      `cloud_object/mod.rs:542` therefore calls `SyncId::uid`) — behind
      `info_box.rs:683`, `data_source.rs:53`, `snapshots.rs:541`,
      `notebooks/manager.rs:81/99`, `env_var_collection.rs:169`,
      `drive/items/workflow.rs:126`, `persistence.rs:1725/1736`. Every
      `GenericStringObjectId`-typed VALUE site enumerated: the workspace's
      only `: GenericStringObjectId` annotation is the
      `From<GenericStringObjectId> for SyncId` impl param
      (`ids.rs:413`), whose body does `Self::ServerId(id.into())` with no
      uid call; the `from_hash` site (`sqlite.rs:2323`) converts
      `.map(|id| SyncId::ServerId(id.into()))`; `get_server_enum_ids ->
      Vec<GenericStringObjectId>` (`workflow.rs:151`) and the telemetry
      `enum_ids`/`object_id` field inits (`input.rs:5952/12080/6322`,
      `terminal/view.rs:6621`, `workspace/view.rs:15097`) feed
      serde-derived telemetry structs whose fields are never read. The
      method is inherent (in no trait — no generic dispatch can reach it)
      and both `server_id_traits!` macro copies generate no `uid`.
      Test files specifically checked (ids_tests, catalog_tests,
      model_tests, profiles_tests, data_source_tests): all receivers
      `SyncId`/trait or turbofish generic params, zero
      `GenericStringObjectId` receivers. Single-column trace against the
      survivors: `SyncId::uid` stays live via app
      `cloud_object/mod.rs:542` + `persistence.rs:314`, `ServerId::uid`
      via `root_view.rs:839`, confirming the receiver triage
      distinguishes the dead wrapper from its live callees. Deleted the
      whole `impl GenericStringObjectId` block + the preceding blank
      line; no imports affected (`ObjectUid` is defined in `ids.rs` and
      stays live at `:74/:187/:223`) (1 file, +0/−6). Deliberately left:
      `GenericStringObjectId` itself (LIVE — turbofish-wide
      `get_all_objects_of_type::<GenericStringObjectId, _>` sites,
      `CloudStringObject::IdType`, telemetry field types, the
      `server_id_traits!` impls, From/TryFrom impls), `SyncId::uid` /
      `ServerId::uid` / `CloudObjectTypeAndId::uid` /
      `ServerCloudObject::uid` / the app-side `CloudObject::uid` /
      `settings_view::mcp_servers`' uid (all live, receivers verified),
      `ObjectUid` (live), the `From<GenericStringObjectId> for SyncId`
      impl (live — the `from_hash` path flows through it), the nine
      `cloud_object_models` def-only leaves (next rounds, below), and
      everything else per prior rounds. Local-only safety: zero callers
      means zero behavior change — generic-string cloud object
      persistence/sync runs through the live `JsonSerializer` /
      `SyncId` / `ServerId` paths untouched, and drive, terminal, tabs,
      panes, BYOK AI, settings, themes, and all other local features
      are untouched; only an id-unwrapping getter that could never be
      called is gone.

      Acceptance: clippy baselines captured at HEAD FIRST (worktree
      clean, no stash needed) in both configs — 12 sorted
      warning+location pairs / 14 `^warning` lines each (the 12
      pre-existing warnings: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the edit,
      `-p warp --lib --all-targets` is warning-identical to the
      baseline in BOTH configs (default and `--no-default-features
      --features simplewarp` — sorted-pair diffs empty, 12 pairs / 14
      `^warning` lines each). All 7 checks exit 0 (`check -p
      cloud_objects --all-targets` ± `--all-features`, `check -p warp
      --lib --all-targets` both feature sets, `--no-default-features
      --features simplewarp --bin simplewarp`, `--bin warp-oss`,
      `--all-targets -p integration`) with 0 errors and only the two
      pre-existing `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`,
      observed in the integration check). Format clean
      (`./script/format`; diff remains exactly +0/−6). Nextest `-p warp
      --lib --no-fail-fast`: 4,653 default passed, 4 skipped, 0 failed;
      4,652 simplewarp — first run 4,651 + the known
      `test_command_block_dispatches_event` load flake, rerun clean
      4,652 passed, 4 skipped, 0 failed. Runtime smoke test SKIPPED:
      the user is away and nobody can answer the macOS password
      prompt, so per the 2026-09-23 convention change the GUI binary
      was not built or launched — unit tests plus checks are the
      acceptance for this round. Did not `cargo clean`.

      CRATE-CLEAN VERDICT: crates/cloud_objects now has ZERO known
      candidates. The 4eu one-candidate note is resolved by this
      deletion; the two trace-only inference impls
      (`impl SettingsValue for SyncId`, `impl PartialEq for
      GenericCloudObject`) were resolved LIVE in 4eu; every other item
      of the 4er/4eu surveys is re-verified LIVE. The crate is clean;
      any future work there needs a fresh survey.

      Designated NEXT: the nine `crates/cloud_object_models` def-only
      leaves from the 4eu survey (bare-name single-hit = definition
      only at survey time; each needs a fresh independent dual-confirm
      at HEAD before deletion): `WorkflowModel::author_name`
      (`workflow.rs:106`), `MCPServer::from_stored_json`
      (`mcp.rs:216`), `AIFact::is_memory` (`ai_fact.rs:52`),
      `AgentConfig::to_ambient_config` (`cloud_agent_config.rs:33`),
      `Workflow::from_harness_type` / `Workflow::get_enum_ids` /
      `Workflow::is_command_workflow` / `Workflow::replace_object_id`
      (`workflow.rs`), `AIExecutionProfile::is_always_ask`
      (`ai_execution_profile.rs`), `ScheduledAmbientAgent::from_
      harness_type` (`scheduled_ambient_agent.rs`) — batched per the
      4dw/4et small-cluster precedent into the next round(s); the
      models' persistence fns re-verified LIVE in 4eu must not be
      touched. After those, the pivot decision (the cloud-run
      lifecycle walls per the 4ca plan) is the orchestrator's — not
      started this round.

- [x] **cloud_object_models cluster A (4ew) — DONE 2026-09-23.** The
      4ev-designated round: five of the nine `cloud_object_models`
      def-only leaves from the 4eu survey, batched as one cluster per
      the 4dw/4et small-cluster precedent, each independently
      dual-confirmed fresh at HEAD (bare-name + call-syntax + path-form
      + all-files word-guarded sweeps; ledger / `schema.graphql` /
      fixtures / different-type excluded; PCRE lookahead guards
      throughout — no `\b`, per the 4eu tooling note). All five
      verified DEAD and deleted; no live verdicts this round.
      Per-item evidence: (1) `Workflow::author_name` (`workflow.rs:106`
      pre-edit; the 4eu ledger's "WorkflowModel::" label was loose —
      the method is inherent on the `Workflow` enum): bare-name grep
      exactly two `*.rs` hits — the definition and the string literal
      `"author_name".to_string()` in `workflow_tests.rs:125` (a test
      VALUE assigned to the serde `author` field, not a method
      reference); call-syntax `.author_name(` zero. Method-vs-field
      check: it is a getter METHOD over the serde-serialized `author`
      field — the field, its derives, and the `new`/`From` writers
      stay (persisted-type convention); only the never-called getter
      went. (2) `TemplatableMCPServer::from_stored_json` (`mcp.rs:216`
      pre-edit; the 4eu ledger's "MCPServer::" label was loose):
      bare-name grep exactly one hit repo-wide, all files — the
      definition; inherent associated fn in `impl
      TemplatableMCPServer`, declared in no trait, zero call forms
      (`.from_stored_json(` zero, `::from_stored_json` only the def).
      (3) `AIFact::is_memory` (`ai_fact.rs:52` pre-edit): word-guarded
      `(?<![A-Za-z0-9_])is_memory(?![A-Za-z0-9_])` over ALL files
      exactly one hit — the definition; every other grep hit is
      `is_memory_enabled` (different methods/fields on `ai_settings`
      and the agent api payload — receiver-typed triage, excluded by
      the word guard); call-syntax `.is_memory(` zero; method on the
      `AIFact` enum, not a field — the serde `Memory` variant and
      `AIMemory` fields stay. (4) `AgentConfig::to_ambient_config`
      (`cloud_agent_config.rs:33` pre-edit): bare-name grep exactly one
      hit, all files — the definition; inherent method, zero call
      forms. (5) `ActionPermission::is_always_ask`
      (`ai_execution_profile.rs:45` pre-edit; the 4eu ledger's
      "AIExecutionProfile::" label was loose — the method is on
      `ActionPermission`): bare-name grep exactly one hit, all files —
      the definition; the sibling `is_always_allow` methods on the four
      permission enums are different names and stay (live); call-syntax
      zero. All five are inherent methods in no trait (no generic or
      dyn dispatch can reach them); no serde attribute references any
      of them by path; macro bodies are `*.rs` text so the bare-name
      hits already cover generated code. Deleted the five methods with
      their doc/comment lines, the now-empty `impl AIFact` and `impl
      AgentConfig` blocks, and the `AgentConfigSnapshot` part of
      cloud_agent_config.rs's `use crate::{...}` import (the type
      itself stays — defined at `scheduled_ambient_agent.rs:20`,
      heavily live via `app/src/ai/ambient_agents/task.rs:5` and the
      `AgentConfigSnapshot::is_empty` serde attr) (5 files, +1/−72).

      One-hop orphan check after the deletions, noted as next-round
      candidates rather than expanding scope: `FromStoredJsonError`
      (`mcp.rs:144`) — its only consumer was the deleted
      `from_stored_json`, now zero references repo-wide; and
      `TemplatableMCPServer::from_user_json` (`mcp.rs:213`) — its only
      in-crate caller was `from_stored_json` (every other
      `from_user_json` hit is an app-side type: the app trait methods
      on app `MCPServer` and `ParsedTemplatableMCPServerResult`); both
      are pub items in pub modules with glob re-exports, so no
      dead-code warning fires (`check -p cloud_object_models
      --all-targets` emits zero warnings). `find_template_map` /
      `find_template_map_strict` stay LIVE (`app/src/ai/agent_sdk/
      mcp_config.rs:55`, `app/src/ai/mcp/parsing.rs:248,269`).

      Deliberately left: the remaining cluster-B leaves from the 4eu
      survey (DESIGNATED NEXT: `Workflow::get_enum_ids` /
      `Workflow::is_command_workflow` / `Workflow::replace_object_id` /
      `ScheduledAmbientAgent::from_harness_type` — each needs its own
      fresh dual-confirm; correction to the 4eu list:
      `Workflow::from_harness_type` no longer exists at HEAD — the only
      `from_harness_type` left in `*.rs` is `ScheduledAmbientAgent`'s
      at `scheduled_ambient_agent.rs:95`); the one-hop orphans above;
      the models' persistence fns re-verified LIVE in 4eu
      (`delete_folder`/`delete_notebook`/`delete_workflow`,
      `upsert_folders`/`upsert_notebooks`/`upsert_workflows`,
      `get_init_command_for_env_var_value` +
      `serialize_variables_internal`) — must not be touched; the serde
      surface under the deleted getters (`Workflow::Command.author`,
      `AIFact::Memory` + `AIMemory`, `ActionPermission::AlwaysAsk`,
      the `AgentConfig` struct) — persisted, kept per convention; and
      everything else per prior rounds. Local-only safety: zero callers
      means zero behavior change — all five deleted methods could never
      be entered, so workflow listing and editing, MCP server template
      parse/install flows (which run through the app-side
      `ParsedTemplatableMCPServerResult` machinery, untouched), agent
      permission gating (the live `description()` / `is_always_allow`
      / `is_enabled` paths, untouched), cloud-object persistence/sync,
      drive, terminal, tabs, panes, BYOK AI, settings, themes, and all
      other local features are untouched; only five never-callable
      getters/constructors and one never-used import part are gone.

      Acceptance: clippy baselines captured at HEAD FIRST (worktree
      clean, no stash needed) in both configs — 12 sorted
      warning+location pairs / 14 `^warning` lines each (the 12
      pre-existing warnings: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the edit,
      `-p warp --lib --all-targets` is warning-identical to the
      baseline in BOTH configs (default and `--no-default-features
      --features simplewarp` — sorted-pair diffs empty, 12 pairs / 14
      `^warning` lines each). All checks exit 0 with 0 errors (`check
      -p cloud_object_models --all-targets` — zero warnings; `check -p
      cloud_objects --all-targets` ± `--all-features`; `check -p warp
      --lib --all-targets` both feature sets; `--no-default-features
      --features simplewarp --bin simplewarp`; `--bin warp-oss`;
      `--all-targets -p integration`) with only the two pre-existing
      `step.rs` unused-import warnings
      (`single_terminal_view_for_tab`, `crate::terminal::CLIAgent`,
      observed in the integration check). Format clean
      (`./script/format`; diff remains exactly +1/−72). Nextest `-p
      warp --lib --no-fail-fast`: 4,652 simplewarp passed, 4 skipped,
      0 failed; 4,653 default passed, 4 skipped, 0 failed — exactly
      the baseline, zero tests added or removed, no flakes. Runtime
      smoke test SKIPPED: the user is away and nobody can answer the
      macOS password prompt, so per the 2026-09-23 convention change
      the GUI binary was not built or launched — unit tests plus
      checks are the acceptance for this round. Did not
      `cargo clean`.

      Designated NEXT: cluster B, the four remaining `cloud_object_
      models` def-only leaves (`Workflow::get_enum_ids`,
      `Workflow::is_command_workflow`, `Workflow::replace_object_id`,
      `ScheduledAmbientAgent::from_harness_type`), then the one-hop
      orphans (`FromStoredJsonError`,
      `TemplatableMCPServer::from_user_json`); after those, the
      cloud-run-lifecycle pivot decision (the cloud-run lifecycle
      walls per the 4ca plan) is the orchestrator's — NOT started this
      round.

- [x] **cloud_object_models cluster B (4ex) — DONE 2026-09-23.** The
      4ew-designated round: the four remaining `cloud_object_models`
      def-only leaves from the 4eu survey plus both 4ew one-hop orphans,
      batched as one cluster per the 4dw/4et small-cluster precedent,
      each independently dual-confirmed fresh at HEAD (bare-name
      PCRE-word-guarded + call-syntax + path-form sweeps; ledger /
      `schema.graphql` / fixtures / different-type excluded). All six
      verified DEAD and deleted; no live verdicts this round.
      TOOLING correction discovered this round (extends the `\b` note):
      a first-pass call-syntax regex of the shape
      `(?<![A-Za-z0-9_])\.name\s*\(` silently misses `ident.method(`
      calls because the lookbehind anchors before the DOT and `h` in
      `h.model_config()` is a word char — it wrongly reported the LIVE
      `HarnessConfig::model_config` as callerless until the corrected
      lookbehind-free form `\.name\s*\(` found
      `app/src/ai/agent_sdk/mod.rs:452`; all call-syntax sweeps below
      were re-run with the corrected form. Per-item evidence: (1)
      `Workflow::get_enum_ids` (`workflow.rs:133` pre-edit): bare-name
      grep exactly two repo-wide hit classes — the definition and four
      `plan.md` ledger lines; corrected call-syntax zero; path-form
      ledger only. Sibling trace: `get_server_enum_ids` stays LIVE
      (`app/src/terminal/input.rs:5952,12083`,
      `app/src/terminal/view.rs:6621`) — the sweep tells live from dead
      in this same impl block. (2) `Workflow::is_command_workflow`
      (`workflow.rs:113` pre-edit): bare-name def + ledger only;
      call/path zero. Sibling trace: `is_agent_mode_workflow` LIVE
      (`workflow_search_item.rs:113`, `terminal/input.rs:6078`,
      `workflows/info_box.rs:162,349,799`). (3)
      `Workflow::replace_object_id` (`workflow.rs:167` pre-edit):
      bare-name def + ledger only; call/path zero. (4)
      `HarnessConfig::from_harness_type` (`scheduled_ambient_agent.rs:95`
      pre-edit; label correction: the method is on `HarnessConfig`, not
      `ScheduledAmbientAgent` — the task list's loose label): bare-name
      in `*.rs` exactly one hit — the definition; the only non-ledger
      mention anywhere is stale design prose
      (`specs/REMOTE-1454/TECH.md:326`, which names an
      `AmbientAgentViewModel::spawn_agent` that does not exist —
      `fn spawn_agent` zero in `*.rs`); construction-site proof (4em/4eo
      class): the ONLY `HarnessConfig {` construction repo-wide is a
      struct literal at `app/src/ai/agent_sdk/mod.rs:215`, not this
      constructor. Also re-verified the 4ew note: `Workflow::
      from_harness_type` zero hits in `*.rs` — no such method at HEAD.
      (5) `FromStoredJsonError` (`mcp.rs:144` pre-edit): bare-name
      exactly one hit repo-wide outside the ledger — the definition;
      zero construction/name sites (the 4ew one-hop prediction held).
      (6) `TemplatableMCPServer::from_user_json` (`mcp.rs:213`
      pre-edit): corrected call-syntax `.from_user_json(` zero
      repo-wide; every `::from_user_json` path hit triaged to app-side
      types — the app trait method on app `MCPServer`
      (`app/src/ai/mcp/mod.rs:292,301`, returns `Vec<MCPServer>`) and
      `ParsedTemplatableMCPServerResult`'s inherent
      (`app/src/ai/mcp/parsing.rs:259`) with their own callers
      (`driver.rs:1034`, `templatable_manager/native.rs:877`,
      `settings_view/mcp_servers/edit_page.rs:565,888`, tests) — zero
      calls resolve to the cloud_object_models inherent. Helpers
      triaged post-deletion: `find_template_map` /
      `find_template_map_strict` / `find_servers_under_known_keys` all
      stay LIVE (`agent_sdk/mcp_config.rs:55`, `mcp/parsing.rs:248,269`
      + internal calls); `to_user_json` LIVE
      (`edit_page.rs:240`, `mcp/mod_tests.rs:96`); `TemplateVariable` /
      `JsonTemplate` stay (persisted fields of `TemplatableMCPServer`).
      Deleted the six items with their doc/comment lines plus the two
      imports whose only users were the deleted bodies
      (`use chrono::Utc;` — `Utc::now()` in `from_user_json`; `use
      handlebars::get_arguments;` — the sole `get_arguments` call) (3
      files, +0/−110). No import changes needed in `workflow.rs`
      (`SyncId`/`GenericStringObjectId` still used by live siblings and
      serde fields) or `scheduled_ambient_agent.rs` (`Harness` still
      used by the `harness_type` field and the serde fns).

      Deliberately left: the LIVE MCP-template helpers above; the
      serde/persistence surface (`Workflow::Command.author` /
      `environment_variables`, `ArgumentType::Enum`, `HarnessConfig`
      fields incl. `harness_type` with its custom serde fns,
      `TemplatableMCPServer` + `JsonTemplate` + `TemplateVariable`,
      `FromStoredJsonError`'s old serde_json payload type) — persisted,
      kept per convention; `HarnessConfig::model_config` (LIVE via
      `agent_sdk/mod.rs:452` — caught only by the corrected
      call-syntax regex, see tooling note); the `Workflow` builder/live
      getters (`name`/`content`/`prompt`/`command`/`description`/
      `arguments`/`tags`/`source_url`/`shells`/
      `default_env_vars`/`name_starts_with_char_ignore_case`/`new`/
      `with_*`/`set_name`, `get_server_enum_ids`) — out of scope;
      and the now crate-orphaned `handlebars` and `chrono` workspace
      dependencies of `crates/cloud_object_models/Cargo.toml` (no
      remaining use in the crate's `src/` after this round) — noted as
      candidates; Cargo.toml dependency removal was not expanded into
      this round.

      Crate-clean verdict: this exhausts the known
      `cloud_object_models` candidates — the 4eu survey's nine
      def-only leaves are all deleted (five in 4ew, four here) and
      both 4ew one-hop orphans are gone, so the crate's known
      def-only-leaf queue is EMPTY; remaining cleanup in the crate is
      limited to the two orphaned Cargo dependencies above (needs a
      workspace-wide use check for `handlebars`/`chrono` before
      removal). The next major item is the cloud-run lifecycle walls
      per the 4ca plan — the ORCHESTRATOR decides the pivot; NOT
      started this round.

      Local-only safety: zero callers means zero behavior change — all
      six deleted items could never be entered, so workflow listing and
      editing, enum-ID telemetry (the live `get_server_enum_ids` path),
      MCP server template parse/install flows (which run entirely
      through the app-side `ParsedTemplatableMCPServerResult` /
      app-`MCPServer` machinery and the live `find_template_map` /
      `to_user_json` paths, untouched), agent spawn harness overrides
      (built via the struct literal at `agent_sdk/mod.rs:215`,
      untouched), cloud-object persistence/sync, drive, terminal, tabs,
      panes, BYOK AI, settings, themes, and all other local features
      are untouched; only four never-callable methods, one
      never-referenced error enum, and two now-unused imports are gone.

      Acceptance: clippy baselines captured at HEAD FIRST (worktree
      clean) in BOTH configs — 12 sorted warning+location pairs / 14
      `^warning` lines each (the 12 pre-existing warnings: 11
      unneeded-return in `app/src/terminal/input.rs` + 1
      single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`); after the edit,
      `-p warp --lib --all-targets` is warning-identical to the
      baseline in BOTH configs (default and `--no-default-features
      --features simplewarp` — sorted-pair diffs empty, 12 pairs / 14
      `^warning` lines each). Check suite exit 0 with 0 errors and zero
      new warnings: `check -p cloud_object_models --all-targets` (zero
      warnings), `check -p cloud_objects --all-targets` (plus
      `--all-features`), `check -p warp --lib --all-targets` both
      feature sets, `--no-default-features --features simplewarp --bin
      simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      (only the two pre-existing `step.rs` unused-import warnings,
      `single_terminal_view_for_tab` and `crate::terminal::CLIAgent`).
      Format clean (`./script/format`; diff remains exactly +0/−110).
      Nextest `-p warp --lib --no-fail-fast`: 4,652 simplewarp passed,
      4 skipped, 0 failed; 4,653 default passed, 4 skipped, 0 failed —
      exactly the baseline, zero tests added or removed, no flakes.
      Runtime smoke test SKIPPED: the user is away and nobody can
      answer the macOS password prompt, so per the 2026-09-23
      convention change the GUI binary was not built or launched —
      unit tests plus checks are the acceptance for this round. Did
      not `cargo clean`.

- [x] **orphaned-dependency sweep, cloud crates (4ey) — DONE 2026-09-23.**
      Follow-up to rounds 4eg–4ex: after the cloud-type deletions, EVERY
      dependency declared in `crates/cloud_object_models/Cargo.toml`
      (23 `[dependencies]` + 3 `cfg(not(wasm))` target deps + 1
      dev-dep = 27 verdicts) and `crates/cloud_objects/Cargo.toml`
      (17 `[dependencies]` = 17 verdicts; 44 total) was grep-verified
      against its own crate's `src/`: path form
      `(?<![A-Za-z0-9_])<rustname>\s*(::|;)` per dep (hyphens→
      underscores; no `package =` renames in either file), plus
      word-guarded all-files bare-name sweeps for attribute/string-only
      forms (`#[serde(with = ...)]`, `#[derivative(...)]`,
      `#[schemars(...)]`, `#[derive(thiserror::Error)]`,
      `cynic::Id`); neither crate has a build.rs, so no non-Rust dep
      entry points. Four deps verified UNUSED and removed (2+2):
      cloud_object_models `chrono` and `handlebars` (both 4ex-flagged;
      word-guarded grep over the whole crate dir hits ONLY the
      Cargo.toml lines — zero src uses; their sole users were 4ex's
      deleted `use chrono::Utc;` in `from_user_json` and
      `use handlebars::get_arguments;`; the `diesel` dep's separate
      `chrono` FEATURE token is diesel-side and untouched), and
      cloud_objects `lasso` (zero hits in any form anywhere in the
      crate) and `session-sharing-protocol` (zero src hits — the last
      import, `use session_sharing_protocol::common::Role;`, went with
      4em's deleted `From` impls). All 40 other entries verified USED
      with named evidence, kept — sample sites: models `ai`
      (`ai::LLMId` ai_execution_profile.rs:3,
      `ai::document::AIDocumentId` notebook.rs:4), `anyhow` (12 hits),
      `cfg-if` (`cfg_if::cfg_if!` ai_execution_profile.rs:299),
      `cloud_objects` (38), `lazy_static`, `log` (`log::warn!`
      scheduled_ambient_agent.rs:112), `regex` (ai_execution_profile.rs:9),
      `schemars` (JsonSchema impl ai_execution_profile.rs:256),
      `serde`, `serde_json` (77), `serde_regex` (string-path attribute
      `#[serde(with = "serde_regex")]` ai_execution_profile.rs:208),
      `session-sharing-protocol` (`ProfileData` user_profile.rs:3),
      `settings` (`SyncToCloud` preference.rs:8), `settings_value`
      (ai_execution_profile.rs:284), `uuid`, `warp-workflows`
      (`warp_workflows::Shell` workflow.rs:37), `warp_cli`
      (`warp_cli::agent::Harness` scheduled_ambient_agent.rs:8),
      `warp_core`, `warp_errors` (`report_error` mcp.rs:9),
      `warp_graphql` (26), `warp_util` (`path::ShellFamily`
      env_vars.rs:6), target deps `cloud_object_persistence` /
      `diesel` (17) / `persistence` (`use persistence::model::{Folder,
      ...}` folder/persistence.rs:8); the dev-dep
      `cloud_objects = { workspace = true, features = ["test-util"] }`
      KEPT as load-bearing via a macro-forwarded feature requirement
      (same compiler-only class as the 4eu derive lesson):
      cloud_object_models expands `cloud_objects::server_id_traits!`
      (notebook.rs:29, workflow.rs:338), whose test-gated
      `impl From<i64> for $t` body `Self(id.into())` needs
      `From<i64> for ServerId`, itself
      `#[cfg(any(test, feature = "test-util"))]` inside cloud_objects
      (ids.rs:259) — in cloud_object_models' test build cloud_objects
      compiles as a normal dep without `cfg(test)`, so only the
      dev-dep's feature supplies it. cloud_objects `anyhow` (14),
      `chrono` (`DateTime, Utc` cloud_object/mod.rs:6, live at :280+),
      `cynic` (`cynic::Id` mod.rs:1009,1013), `derivative` (mod.rs:7,316),
      `itertools` (`Itertools` ids.rs:3), `pathfinder_geometry`
      (`vec2f` mod.rs:8), `schemars` (ids.rs:15+), `serde`,
      `settings_value` (ids.rs:104, proven LIVE in 4eu), `thiserror`
      (ids.rs:178), `uuid` (ids.rs:5), `warp_core`, `warp_graphql`
      (49), `warp_server_auth` (the live `UserUid`+`TEST_USER_UID`
      re-export auth/mod.rs:1), `warpui_core` (mod.rs:15+).
      Workspace-level: NO changes — every removed name keeps live
      member references elsewhere: `chrono` in ~17 member Cargo.tomls;
      `handlebars` (vendored path crate `crates/handlebars`) still
      declared and genuinely used by `app` (app/src/ai/mcp/parsing.rs:3
      et al.); `lasso` still declared by warp_server_auth /
      warp_server_client / app; `session-sharing-protocol` still
      declared by warp_server_client / cloud_object_persistence / app
      (and by cloud_object_models, where it is used). Cargo.lock
      refreshed: the diff is exactly the four removed dependency EDGES
      of the two cloud crates — no package leaves the lock. Local-only
      safety: removing never-referenced dependency declarations
      compiles identical code — the deleted edges name no symbol in
      either crate's source (verified by the greps above and by the
      zero-warning clean compiles), so cloud-object persistence/sync,
      drive, terminal, tabs, panes, BYOK AI, settings, themes, and all
      other local features are untouched; only four unused Cargo
      dependency lines and their lockfile edges are gone.

      Acceptance: clippy baselines captured at HEAD FIRST (worktree
      clean) in BOTH configs — 16 sorted warning+location lines each
      (12 locations: 11 unneeded-return in
      `app/src/terminal/input.rs` + 1 single-element-loop in
      `terminal/model/lifecycle/mod_tests.rs:277`, plus 4 `^warning`
      lines: 2 crate summaries + 2 warning-kind lines); after the
      edit, `-p warp --lib --all-targets` is warning-identical to the
      baseline in BOTH configs (default and `--no-default-features
      --features simplewarp` — sorted-pair diffs empty). Check suite
      exit 0 with 0 errors and zero new warnings: `check -p
      cloud_object_models --all-targets` (0 errors, 0 warnings),
      `check -p cloud_objects --all-targets` and `+ --all-features`
      (0/0 each), `check -p warp --lib --all-targets` both feature
      sets (0/0 each), `--no-default-features --features simplewarp
      --bin simplewarp` (0/0), `--bin warp-oss` (0/0),
      `--all-targets -p integration` (exit 0; only the two
      pre-existing `step.rs` unused-import warnings,
      `single_terminal_view_for_tab` and `crate::terminal::CLIAgent`).
      `./script/format` no diff. Nextest `-p warp --lib
      --no-fail-fast`: 4,652 simplewarp passed, 4 skipped, 0 failed;
      4,653 default passed, 4 skipped, 0 failed — exactly the
      baseline, zero tests added or removed, no flakes. Runtime smoke
      test SKIPPED: the user is away and nobody can answer the macOS
      password prompt, so per the 2026-09-23 convention change the GUI
      binary was not built or launched — unit tests plus checks are
      the acceptance for this round. Did not `cargo clean`.

      Both cloud crates' leaf queues are exhausted (cloud_objects
      crate-clean per the 4ev verdict; cloud_object_models' def-only
      leaf queue empty per 4ex) and this round closes the flagged
      orphaned-Cargo-dependency follow-up; the next major item is the
      cloud-run lifecycle walls per the 4ca plan — the ORCHESTRATOR
      decides the pivot; NOT started this round.

- [x] **telemetry scope decision + slice plan (4ez) — RECORDED
      2026-09-23.** SCOPING round for 4ca item (7) — no code deleted;
      this entry is the deliverable. THE DECISION (orchestrator's, now
      grounded): delete the remote telemetry send machinery — every
      file/function that exists to ship events to Rudderstack — while
      preserving genuinely local behavior. Verified local survivors:
      local `log::*` output (never part of the send path — the only
      log line in the pipeline is the `log_named_telemetry_events`
      cargo-feature debug line in `event_store.rs:92`, which dies with
      the queue); `report_error!`/Sentry is SEPARATE — `warp_errors`'s
      macro is log-only (`lib.rs:56-68`, log::log! + ErrorExt), no
      telemetry dependency; network logging is SEPARATE —
      `warp_server_client/src/network_logging.rs` is pure
      `http_client` request hooks (`set_before_request_fn`) feeding
      the in-app pane (`NetworkLogModel` → `network_log_pane.rs`), its
      ONLY telemetry coupling is one line: `server_api.rs:283-285`
      installs it on `telemetry_api.client` alongside the base client;
      when TelemetryApi falls the install list shrinks to
      `[&mut client]` and nothing else changes — network_logging is an
      item-8 "moves" passenger, NOT item-7 scope. The AI-side secret
      redaction (`app/src/ai/blocklist/block/secret_redaction.rs`,
      visual safe-mode obfuscation) is a DIFFERENT module that stays;
      the telemetry-side `telemetry/secret_redaction.rs` dies (its
      only reader is `telemetry_ext.rs`; its only live writer is one
      call in `terminal/secret_regex_updater.rs:55`
      `update_telemetry_secrets_regex`, which dies with it — the
      updater's `set_user_and_enterprise_secret_regexes` line above is
      live local behavior and stays). Privacy settings with live LOCAL
      readers stay: `user_secret_regex_list` /
      `enterprise_secret_regex_list` (terminal model redaction),
      `is_crash_reporting_enabled`, `is_cloud_conversation_storage_enabled`;
      the telemetry-only fields die (`is_telemetry_enabled` setting +
      model field whose only readers are the collector, the snapshot,
      and the Settings toggle that is ALREADY HIDDEN in this build via
      `is_telemetry_available() == false`, `privacy_page.rs:1409`;
      `is_telemetry_force_enabled` — remote-teams-populated only;
      `should_collect_ai_ugc_telemetry` snapshot field; and the whole
      `PrivacySettingsSnapshot` type — every reader of every snapshot
      accessor is a telemetry call path; the one non-macro holder,
      `terminal/view.rs` `privacy_settings_snapshot` field
      `:2311/:3517/:3793`, exists solely to send
      `SessionAbandonedBeforeBootstrap`).

      ANCHOR CORRECTIONS: the "746 call sites" figure is wrong —
      measured at HEAD: 620 macro-invocation lines across 142 files
      (`send_telemetry_from_ctx!` 561, `send_telemetry_from_app_ctx!`
      41, `send_telemetry_sync_from_app_ctx!` 8,
      `send_telemetry_on_executor!` 6,
      `send_telemetry_sync_from_ctx!` 4; 139 files in app/src, 1 each
      in crates/ai, crates/onboarding, crates/repo_metadata). And
      "the flush poller" is THREE pollers plus two one-off flushers
      (below).

      PIPELINE MAP (end to end, every file/function): (1) DEFINITION —
      a type impls `warp_core::telemetry::TelemetryEvent`
      (name/payload/description/enablement_state/contains_ugc;
      `crates/warp_core/src/telemetry.rs:20-59`) and registers via
      `register_telemetry_event!` (`:62-71`, `inventory::submit!`
      non-wasm) — 19 impl blocks in 18 files (catalog `events.rs`
      plus 17 app files incl. `CliTelemetryEvent`
      `ai/agent_sdk/telemetry.rs`, and crates/ai, crates/onboarding,
      crates/repo_metadata). (2) ENQUEUE — `send_telemetry_from_ctx!`/
      `send_telemetry_from_app_ctx!` (`warp_core/src/telemetry.rs:147,
      176`): enablement check, read user_id/anonymous_id from the
      `TelemetryContextModel` singleton (backed by
      `AppTelemetryContextProvider`, `telemetry/context_provider.rs`,
      registered `lib.rs:1147`), then
      `warpui_core::record_telemetry_from_ctx!`/
      `record_telemetry_on_executor!` (`crates/warpui_core/src/
      telemetry/mod.rs:17,36`) → `EventStore::record_event` into the
      GLOBAL IN-MEMORY QUEUE (bounded `BoundedVecDeque`, 1024;
      `event_store.rs:9`). The macros do NOTHING besides this — no
      local logging, no UI event store, no DB write. (3) DIRECT PATH —
      `send_telemetry_sync_from_ctx!`/`..._from_app_ctx!` (`app/src/
      server/telemetry/macros.rs:5,40`) and one direct call
      (`terminal/view.rs:25423`) → `ServerApi::send_telemetry_event`
      (`server_api.rs:344`) → `TelemetryApi::send_telemetry_event`
      (`telemetry/mod.rs:215`) → `send_telemetry_event_internal`
      (`:239`, privacy gate → optional file persist →
      release/sandbox gate → send). (4) FLUSHERS — `TelemetryCollector`
      (`telemetry/collector.rs`, constructed `lib.rs:1537-1539`):
      `schedule_event_queue_flush` every 30s (`:200`,
      `flush_telemetry_events` → `TelemetryApi::flush_events` `:95`
      drains the queue); `schedule_send_active_usage_event` every 60s
      (`:172`, needs cargo `record_app_active_events`);
      `flush_persisted_events_from_disk` on startup (`:137`, reads
      `rudder_telemetry_events.json`, needs release/sandbox); shutdown
      flush (`lib.rs:1955` → `collector.rs:101`): SDK/daemon modes
      flush with a 5s timeout, GUI mode `write_telemetry_events_to_disk`
      persists ≤20 non-UGC events to `secure_state_dir()/
      rudder_telemetry_events.json`; plus the login one-off
      (`auth/auth_manager.rs:288-323`: record_identify + record + immediate
      flush); `clear_event_queue` on the telemetry-toggle-changed
      event (`collector.rs:73-77`). (5) MESSAGE BUILD —
      `telemetry_ext.rs::TelemetryExt::to_rudder_batch_message` maps
      `warpui::telemetry::Event` → `rudder_message.rs` Identify/Track
      types, redacting UGC payloads via `secret_redaction.rs`.
      (6) HTTP — `TelemetryApi::send_batch_messages_to_rudder`
      (`mod.rs:304`, UGC/non-UGC partition) → `send_rudder_request`
      (`:381`) POST `{root_url}/v1/batch` basic_auth(write_key) on
      TelemetryApi's own reqwest client (`:71-86`, https_only);
      destinations from `ChannelState::rudderstack_{ugc,non_ugc}_destination()`
      (`warp_core/src/channel/state.rs:287,300` ← channel config
      `telemetry_config`; `None` ⇒ `RudderStackDestination::default()`,
      empty root_url/write_key). (7) DOCS SURFACE —
      `warp_cli::Command::PrintTelemetryEvents` (`warp_cli/src/lib.rs:
      428,439`; dispatch `lib.rs:629`) →
      `TelemetryEvent::print_telemetry_events_json` (`events.rs:4313`)
      dumps the inventory catalog (the external privacy doc's
      exhaustive-telemetry-table source; `events_tests.rs` asserts
      non-empty names/descriptions). No schema export: schema.graphql
      has only the unrelated remote `TelemetrySettings` workspace
      types (item-8 endgame), persistence schema has nothing.

      CLASSIFICATION — REMOTE-ONLY (die in slice 1): `telemetry/mod.rs`
      (412, TelemetryApi + clear_event_queue/rudder_event_file_path),
      `rudder_message.rs` (249) + `LICENSE-RUDDER-SDK-RUST.txt`,
      `context.rs` (103), `telemetry_ext.rs` (127) + its tests,
      `secret_redaction.rs` (132) + its tests (222), `collector.rs`
      (229), `mod_tests.rs` (61), `macros.rs` sync-macro halves,
      ServerApi's `telemetry_api` field + 4 methods, the
      `secret_regex_updater.rs:43-55` update call. SHARED (die in
      slice 2 with the queue/catalog): `events.rs` (5,847 — the
      `TelemetryEvent` enum ~1,336 variant lines + impls; the ~70
      payload structs/enums are constructed only at macro sites;
      `AppStartupInfo`/`CloseTarget`/`PaletteSource` imported by
      `lib.rs:262` for macro sites only) + `events_tests.rs`; the
      warp_core trait/inventory machinery; `context_provider.rs` (26;
      backs the queue macros' identity read, so it dies with the
      queue not the HTTP); `warpui_core/src/telemetry/` (467) — the
      queue is LOCAL-ONLY code but has no drainer after slice 1, so
      it falls as dead-end; `app_focus_telemetry.rs` (warpui_core)
      records only into the queue — dies with it; the 17 other
      event-def files (~3,754 lines total, event parts only —
      `free_ai_removal_modal.rs` and `blocklist/telemetry.rs` are
      mixed files, delete only the event types/impls). LOCAL-ONLY
      (stays): blocklist visual secret redaction, network_logging,
      report_error!/warp_errors, `OperatingSystemInfo` (shared with
      graphql + http_client), privacy settings' secret-regex/crash/
      cloud-storage fields, the privacy-page UI minus the telemetry
      toggle.

      RUNTIME ANSWER (why the smoke tests see zero TCP): telemetry is
      default-ON in settings (`is_telemetry_enabled` default true,
      `privacy.rs:58`) but FOUR independent brakes close the send path
      in this repo's builds. (1) The `simplewarp` bin constructs
      ChannelState with `telemetry_config: None`
      (`app/src/bin/simplewarp.rs:27`; its doc says "no telemetry") —
      so even a reached send would POST to an empty root_url
      (`RudderStackDestination::default()`). (2) `ChannelState::
      is_release_bundle()` is `cfg!(feature = "release_bundle")`
      (`channel/state.rs:82`) and that cargo feature is in neither
      `default` nor `simplewarp`. (3) `WithSandboxTelemetry` is in no
      default flag list (not DOGFOOD/RELEASE/DEBUG; PREVIEW empty) —
      only the `warp` dev bin enables it, and only when the
      `WITH_SANDBOX_TELEMETRY` env var is set (`local.rs:15-17`); the
      simplewarp bin adds DEBUG_FLAGS only. (4) Consequently ALL
      THREE collector pollers are never scheduled — each arm of
      `initialize_telemetry_collection` (`collector.rs:44-65`) requires
      release-bundle/sandbox/`SendTelemetryToFile`/`RecordAppActiveEvents`,
      all absent. Net effect: events pile into the bounded-1024
      in-memory queue and are silently dropped; the only local trace
      is the GUI-shutdown disk write of ≤20 non-UGC events to
      `rudder_telemetry_events.json`, a file that is never read back
      without the release bundle. Deleting the send machinery is
      therefore behavior-invisible in this fork: it removes dead
      enqueue pressure, that shutdown file write, and ~11k lines.

      SLICE PLAN (each state compiles; order validated, split
      corrected — slice 2 must be three slices: the catalog and the
      queue have different dependency directions):
      SLICE 1 — remote send path (~1,700 deleted lines, ~15 files).
      Delete: `telemetry/mod.rs` TelemetryApi + flush/persist/
      clear functions, `rudder_message.rs`, `context.rs`,
      `telemetry_ext.rs` + tests, `secret_redaction.rs` + tests +
      the `secret_regex_updater.rs` update call, `collector.rs` +
      `lib.rs` wiring (`:262` import, `:1537-1540` construction,
      `:1955-1958` shutdown call), the `auth_manager.rs:285-323`
      login flush block (the DB persist above it stays),
      `terminal/view.rs` SessionAbandoned send block + the
      `privacy_settings_snapshot` field trio, ServerApi's
      `telemetry_api` field + 4 methods + constructor params,
      `network_log` install shrinks to `[&mut client]`.
      COMPILE-BRIDGE: keep `ServerApi::send_telemetry_event` as a
      no-op stub (same pattern as the AI `local_only_error()` stubs)
      so the 12 sync-macro sites keep compiling; the 608 queue macros
      keep working untouched (enqueue-only, same as today). Risk:
      moderate — many small edit sites, all subtractive; the
      `warpui::telemetry` queue still exists so zero macro-site churn.
      Acceptance evidence: the 7-check suite both feature sets,
      clippy-identical both configs, nextest baseline, and a smoke
      run confirming no TCP AND no `rudder_telemetry_events.json`
      written at shutdown. SLICE 2a — the event catalog + call sites
      (~8,500 lines, ~160 files; the big mechanical one): delete all
      620 macro invocations (142 files), `events.rs` +
      `events_tests.rs`, the event parts of the 17 other event-def
      files + `register_telemetry_event!` sites, `CliTelemetryEvent`,
      `PrintTelemetryEvents` (`warp_cli/src/lib.rs:428,439` +
      `lib.rs:629`), payload-type imports (`lib.rs:262` shrinks to
      `TelemetryCollector` → then nothing). Risk: LOW per site, high
      file count; every site is deleted not edited, presubmit
      compiles all of them. SLICE 2b — queue + trait machinery
      (~1,300 lines, ~20 files): `warpui_core/src/telemetry/` +
      `app_focus_telemetry.rs` daily-focus recording (+ the
      `lib.rs` shutdown `try_record_daily_app_focus_duration` call),
      `warp_core/src/telemetry.rs` whole file (trait, inventory,
      `EnablementState`, `TelemetryContextModel`/Provider, queue
      macros, mock), `telemetry/macros.rs` + the
      `ServerApi::send_telemetry_event` stub, `context_provider.rs`
      + `lib.rs:1147` registration, `auth_manager` record lines,
      `notebook_tests.rs:330` queue assertion; check the `inventory`
      dep (settings + warp_errors have their own collects) and the
      5 test-harness files registering `AppTelemetryContextProvider`.
      Risk: moderate — test-harness singletons. SLICE 2c — privacy
      telemetry-only fields (~300 lines, ~8 files): `PrivacySettingsSnapshot`
      (whole type), the `is_telemetry_enabled` setting + model field +
      `UpdateIsTelemetryEnabled` event + collector subscribe +
      terminal-view subscribe remnants, `is_telemetry_force_enabled`
      (user_workspaces + gql_convert plumbing — GraphQL types
      themselves are item-8), the privacy-page telemetry toggle
      section. Risk: settings-schema churn — regenerate via
      `generate_settings_schema` and eyeball the defaults diff.
      SLICE 3 — leftovers (~600 lines, ~15 files): channel-config
      `telemetry_config`/`TelemetryConfig`/`RudderStackConfig`/
      `RudderStackDestination`/`telemetry_file_name`/
      `is_telemetry_available` (touches all six bin channel configs
      that pass `telemetry_config: None` — decide removal vs keep),
      `execution_mode::send_telemetry_at_shutdown`, warp_features
      `WithSandboxTelemetry`/`SendTelemetryToFile`/`RecordAppActiveEvents`
      variants + app cargo features `send_telemetry_to_file`/
      `record_app_active_events`/warpui `log_named_telemetry_events`,
      then the EnablementState-flag sweep: flags referenced ONLY from
      deleted `enablement_state()` arms become newly callerless and
      get their own per-flag removal slices. Total ≈ 11,000+ lines.

      LANDMINES: (1) cfg-gated arms presubmit never compiles —
      `events.rs` has `#[cfg(feature = "local_fs")]` and
      `#[cfg(windows)]` match arms in the exhaustive
      contains_ugc/description matches (the 4ca `skip_login` class);
      slice 2a deletes the whole file so they vanish, but any
      intermediate refactor of those matches must compile both the
      windows/local_fs combos by hand. (2) wasm gates: `telemetry/
      mod.rs` + `context.rs` carry `#[cfg(target_family = "wasm")]`
      branches (`boxed_local`, `Client::default()`,
      `cfg_attr(wasm, allow(clippy::question_mark))`) — never
      compiled by presubmit; `register_telemetry_event!`'s inventory
      half is non-wasm-only. (3) inventory static registration is
      cross-crate: 18 files expand `inventory::submit!`; the crate
      dep stays (settings schema + warp_errors registration use
      their own collects). (4) `TelemetryContextModel` must be
      registered before any queue macro runs — five test files +
      `test_util/terminal.rs:99` register it; deleting the provider
      breaks those harnesses (compile-caught, but noisy). (5)
      `notebook_tests.rs:330` reads the queue back via
      `flush_events()` — the one LOCAL test reader; dies in 2b.
      (6) `AgentModeAnalytics` (DOGFOOD) forces
      `should_disable_telemetry()` false (`privacy.rs:322-324`) — a
      flag→telemetry coupling that silently disappears in 2c; no
      other flag consumer changes. (7) the shutdown
      `rudder_telemetry_events.json` write is the only on-disk
      telemetry artifact in this fork — slice 1 removes it; verify
      with the smoke run. (8) `warp_cli::Command::PrintTelemetryEvents`
      sits in the hidden-command list (`:439` returns true) — delete
      the variant, the hidden-check arm, and the dispatch together.
      (9) `onboarding::OnboardingEvent` shows zero senders outside
      its crate at survey time — a pre-existing candidate for the
      2a sweep to double-check, not assumed. (10) tooling: reapply
      the 4ex corrected call-syntax regex when sweeping macro
      removals' fallouts. DESIGNATED NEXT: slice 1 exactly as
      scoped above. Acceptance for THIS round: no code changes —
      this commit contains only plan.md (verified with
      `git show --stat`); no clippy/nextest required; app not
      launched; no .rs file touched.

- [x] **telemetry slice 1 — remote send path (4fa) — DONE 2026-09-23.**
      Executed the 4ez SLICE 1: every file/function that exists to
      ship telemetry events to Rudderstack is gone; the queue-side
      machinery (macros' enqueue halves, `warpui_core` queue, event
      catalog, `context_provider.rs`) and all local behavior are
      untouched. 20 files changed, 22 insertions(+), 1,881
      deletions(-) (git diff --stat vs 4ez HEAD c372dcfa3).

      DELETED wholesale (9 files, verified remote-only before
      removal): `app/src/server/telemetry/collector.rs` (229;
      TelemetryCollector — all three pollers, the startup
      disk-read flush, the shutdown flush/disk-write, the
      UpdateIsTelemetryEnabled clear_event_queue subscription);
      `telemetry/context.rs` (103; TelemetryContext + AttachContext
      — sole consumer was mod.rs's send path); `telemetry/
      rudder_message.rs` (249) + `telemetry/
      LICENSE-RUDDER-SDK-RUST.txt` (31; no `rudder` crate exists in
      any Cargo.toml/Cargo.lock — the license entry was vestigial);
      `telemetry/secret_redaction.rs` (132) + `telemetry/
      secret_redaction_tests.rs` (222; grep-verified the AI-side
      `ai/blocklist/.../secret_redaction.rs` is a different module
      whose consumers are untouched); `telemetry/mod_tests.rs` (61);
      `server/telemetry_ext.rs` (127) + `server/
      telemetry_ext_tests.rs` (103; 4ez map correction — they live
      in `server/`, not `telemetry/`).

      DELETED from surviving files: `telemetry/mod.rs` rewritten
      412→5 lines (mod.rs now only declares `context_provider`,
      `events`, `macros` and re-exports events) — TelemetryApi, its
      reqwest client, flush_events/flush_persisted_events_to_rudder/
      flush_and_persist_events/persist_events_at_path/
      persist_events_to_telemetry_log_file, send_telemetry_event
      (+internal), send_batch_messages_to_rudder/
      send_rudder_request, `clear_event_queue`,
      `rudder_event_file_path` and the `rudder_telemetry_events.json`
      constant all gone (grep: zero remaining references to
      TelemetryApi/TelemetryCollector/telemetry_context/
      clear_event_queue anywhere). `server/server_api.rs`: the
      `telemetry_api` field, the `TelemetryApi` import, the 3 flush/
      persist methods, and the `Path` import are gone; the
      `network_log` install shrank to `[&mut client]` (the network
      logging pane itself is untouched). `lib.rs`: TelemetryCollector
      import, the singleton registration + its
      INITIALIZE_TELEMETRY_COLLECTION mark_interval_end (no matching
      start site exists anywhere — inert), and the shutdown
      `flush_telemetry_events_for_shutdown` call are gone — the
      shutdown `rudder_telemetry_events.json` write died with the
      collector. `auth/auth_manager.rs`: the login one-off FLUSH is
      gone (snapshot read, server_api clone, `flush_telemetry_events`
      call); the `record_identify_user_event`/`record_event` enqueue
      lines and the DB persist above stay (2b removes the record
      lines). `terminal/view.rs`: the Drop-impl
      SessionAbandonedBeforeBootstrap send block is gone (the local
      `log::log!` abandonment line stays) plus the
      `privacy_settings_snapshot` field trio (decl, the
      UpdateIsTelemetryEnabled subscription, the init). `terminal/
      secret_regex_updater.rs`: the `update_telemetry_secrets_regex`
      call + its regex re-mapping are gone;
      `set_user_and_enterprise_secret_regexes` (live local
      redaction) stays. `remote_server/unix/mod.rs`: comment trimmed
      (it claimed TelemetryCollector was running periodic flushes
      and sending to Rudderstack). Scripts: rudder-sdk-rust license
      entries removed from `script/prepare_bundled_resources` and
      `script/windows/prepare_bundled_resources.ps1`.

      BRIDGE DECISION: `ServerApi::send_telemetry_event` is kept as
      a parameterless no-op stub (`pub async fn send_telemetry_event
      (&self) -> Result<()> { Ok(()) }`), and the two sync-macro
      bodies in `telemetry/macros.rs` were slimmed to match (they no
      longer build the privacy snapshot; the enablement_state gate
      and the ServerApiProvider lookup remain). All 12 sync-macro
      call sites compile unchanged; the 608 queue macros were never
      touched. The stub takes NO parameters (rather than keeping the
      `(event, snapshot)` signature with `_`-prefixed params) per
      the AGENTS.md no-underscore-params rule; `macros.rs` + the
      stub still die together in 2b, so no extra churn later.

      4ez-PLAN DEVIATIONS (all fallout-forced, all subtractive, all
      required to keep clippy warning-identical): (1) `AuthManager
      .server_api` field was readerless after the login-flush
      deletion — removed the field, `AuthManager::new`'s
      `server_api` param (one call site, `lib.rs`), and the
      `new_for_test` lookup lines. (2) `TerminalView` fields
      `server_api`, `bootstrap_start`, `background_executor` (+
      `Background`/`ServerApi` imports) were readerless after the
      Drop-block deletion — removed decls + inits;
      `TerminalViewResources.server_api` stays (Input consumes it).
      (3) `server/mod.rs`'s `pub use warp_core::...
      ::OperatingSystemInfo` re-export was consumerless (its only
      user was the deleted `context.rs`); the warp_core type itself
      stays (graphql + http_client). (4) `PrivacySettings`/`Privacy
      SettingsChangedEvent`/`PrivacySettingsSnapshot` imports
      dropped from view.rs and auth_manager.rs where the deleted
      blocks were the last users.

      Deliberately left (slices 2a/2b/2c per the 4ez plan): the 620
      macro sites, `events.rs`/`events_tests.rs` + the 17 event-def
      files, `PrintTelemetryEvents` (2a); `warpui_core/src/
      telemetry/` queue, `warp_core/src/telemetry.rs` trait/
      inventory machinery, `context_provider.rs` + `lib.rs:1147`
      registration, `telemetry/macros.rs` + the ServerApi stub,
      auth_manager record lines, `notebook_tests.rs:330` queue
      reader, the 5 test-harness `AppTelemetryContextProvider`
      registrations + `test_util/terminal.rs` (2b); the
      `PrivacySettingsSnapshot` type (its `should_disable_telemetry`
      accessor now has zero external callers but is pub-on-lib so no
      lint fires), `is_telemetry_enabled`/`is_telemetry_force_
      enabled`, the privacy-page toggle (2c); channel-config
      `telemetry_config`/`RudderStack*`/`telemetry_file_name`,
      `WithSandboxTelemetry`/`SendTelemetryToFile`/
      `RecordAppActiveEvents` flags (slice 3). Note: warp_logging's
      `ChannelState::telemetry_file_name()` rotate call (native.rs:
      189) still references the slice-3 config item.

      Local-only safety: the deleted code could never fire a network
      request in this fork (4 gates: telemetry_config None,
      no release_bundle, no sandbox flag, so all three collector
      pollers were never scheduled — 4ez RUNTIME ANSWER); events
      already piled into the bounded-1024 queue and were dropped,
      which is exactly what happens now minus the collector/flush
      churn. Log output, report_error!/Sentry, the network logging
      pane, AI-side visual secret redaction, terminal model
      redaction (user/enterprise regexes), privacy settings with
      live local readers, and the shutdown log line are all
      preserved; the only on-disk delta is that
      `rudder_telemetry_events.json` is no longer written at GUI
      shutdown (a file that could never be read back in this fork).

      Acceptance: clippy baselines captured at HEAD FIRST in BOTH
      configs — 12 sorted warning+location pairs each (11
      needless_return in `terminal/input.rs` + 1 single-element-loop
      in `lifecycle/mod_tests.rs:277`); post-edit re-runs are
      warning-IDENTICAL in both (sorted-pair diff empty).
      wasm32-unknown-unknown: target IS installed but has zero
      cached artifacts for this checkout (no HEAD baseline to diff
      against), so the optional check was SKIPPED per its terms;
      instead grep-verified every deleted wasm-gated branch —
      `Client::default()`, `boxed_local()`, `wasm::user_agent()`,
      `cfg_attr(wasm, allow(clippy::question_mark))` — lived only in
      wholesale-deleted files; the remaining gates are in events.rs/
      events_tests.rs (slice 2a, untouched) and two pre-existing
      server_api.rs sites (unchanged). Check suite exit 0, 0 errors:
      `check -p warp --lib --all-targets` default (0 warnings),
      simplewarp (clean), `--no-default-features --features
      simplewarp --bin simplewarp` (clean), `--bin warp-oss`
      (clean), `--all-targets -p integration` (only the two
      pre-existing `step.rs` unused-import warnings,
      `single_terminal_view_for_tab` and `crate::terminal::CLIAgent`),
      `check -p warp --lib --tests --features skip_login` (clean).
      `./script/format` idempotent (diff stable at +22/−1,881 across
      runs). Nextest `-p warp --lib --no-fail-fast`: default 4,631
      passed, 4 skipped, 0 failed; simplewarp 4,630 passed, 4
      skipped, 0 failed — exactly baseline minus the 22 telemetry
      tests deleted with their units (1 mod_tests
      `test_persist_events_doesnt_include_ugc_events`, 3
      telemetry_ext `to_rudder_batch_message_*`, 18
      secret_redaction_tests; 18 = 4 redact_secrets_in_string + 6
      replace_byte_ranges + 4 redact_secrets_in_value + 4
      compose_patterns). No flakes. Runtime smoke SKIPPED: user
      away, nobody to answer the macOS password prompt, so per the
      2026-09-23 convention change the GUI binary was not built or
      launched; the no-TCP claim rests on the 4ez static gating
      analysis, which this slice only shrinks. Did not `cargo clean`.

      DESIGNATED NEXT: slice 2a — the event catalog + call sites
      (~8,500 lines, ~160 files) exactly per the 4ez plan: delete
      all 620 macro invocations (142 files), `events.rs` +
      `events_tests.rs`, the event parts of the 17 other event-def
      files + `register_telemetry_event!` sites, `CliTelemetryEvent`,
      `PrintTelemetryEvents` (`warp_cli/src/lib.rs:428,439` +
      dispatch `:629`), payload-type imports (`lib.rs` import shrinks).
      Respect landmines 1/8/9 (cfg-gated `local_fs`/`windows` arms
      vanish with the file; delete the hidden-command arm with the
      dispatch; double-check `OnboardingEvent` senders).

- [x] **telemetry slice 2a — event catalog + producer sites (4fb) — DONE
      2026-09-23.** Executed the 4ez SLICE 2a: the telemetry event catalog
      and every producer site are gone. 198 files changed, 566
      insertions(+), 16,435 deletions(-) (git diff --stat vs 4fa HEAD
      42ffc2a59).

      SITE INVENTORY (recounted at HEAD): 620 `send_telemetry_*!`
      occurrences in 142 files (139 app/src, 1 each crates/ai,
      crates/onboarding, crates/repo_metadata) — 619 invocation
      statements + 1 doc-comment mention in
      `recording_telemetry.rs`. By form: `send_telemetry_from_ctx!` 560,
      `send_telemetry_from_app_ctx!` 41, `send_telemetry_sync_from_app_ctx!`
      8, `send_telemetry_on_executor!` 6, `send_telemetry_sync_from_ctx!`
      4. All 619 invocation statements deleted via a Rust-aware
      (string/comment-masking, brace-matching) scripted pass; sites that
      pre-built the event value as a `let` binding were collapsed per
      site (bindings removed where the value had no other consumer).

      DELETED wholesale: `app/src/server/telemetry/events_tests.rs`,
      `app/src/server/telemetry/macros.rs` (both sync macros +
      `send_telemetry_on_executor`; all sites dead this round per the
      "if ALL sites of a macro die" rule), `events.rs` reduced
      5,847 → 362 lines (catalog `TelemetryEvent` enum ~1,336 variant
      lines + `TelemetryEvent`/`TelemetryEventDesc` impls +
      `register_telemetry_event!` + `print_telemetry_events_json` +
      19 telemetry-only payload types removed; ~30 non-telemetry domain
      types RETAINED in place — see 4ez-PLAN CORRECTION below), plus 7
      pure event-def files: `agent_sdk/telemetry.rs`,
      `agent_sdk/driver/harness/telemetry.rs`, `antivirus/telemetry.rs`,
      `blocklist/action_model/recording_telemetry.rs`,
      `blocklist/action_model/recording_telemetry_tests.rs`,
      `blocklist/action_model/execute/stop_recording_tests.rs`,
      `blocklist/telemetry.rs` + `blocklist/telemetry_tests.rs`
      (whole file — `OrchestrationApprovalStatus` etc. were
      telemetry-only after the empty emit fns were traced to their
      callers), `blocklist/inline_action/malformed_line_heuristics.rs`
      (all consumers were telemetry), `system/info_tests.rs`,
      `crates/ai/src/telemetry.rs` + `telemetry_tests.rs`,
      `crates/onboarding/src/telemetry.rs` + `telemetry_tests.rs`,
      `crates/repo_metadata/src/telemetry.rs`. Mixed event-def files
      trimmed in place (11): agent_management/telemetry.rs (keeps
      `OpenedFrom`), request_file_edits/telemetry.rs (keeps
      `RequestFileEditsFormatKind`), pricing_promotion.rs (keeps
      Surface/State), skills/telemetry.rs (keeps `SkillOpenOrigin`),
      lsp_telemetry.rs (keeps LspEnablementSource/ControlActionType),
      code_review/telemetry_event.rs (keeps 8 live enums),
      tab_configs/telemetry.rs (keeps 4 enums),
      lifecycle/telemetry.rs (keeps LifecycleRecoveryRecord + limiter —
      live diagnostic path), free_ai_removal_modal.rs, vertical_tabs/
      telemetry.rs, agent/telemetry.rs (ForTelemetry trait removed;
      `EntrypointType::entrypoint()` + `AIIdentifiers` kept — live
      request-metadata plumbing). `register_telemetry_event!` sites: 0
      remain.

      DEAD-CONSEQUENCE FALLBACK removed (each traced to a deleted
      producer): `features_page.rs::telemetry_event` (227-line fn) +
      local `to_string`; `startup_shell.rs::telemetry_event`;
      `available_shells.rs::telemetry_value`; `command_palette::close`
      telemetry locals; `grep.rs::create_redacted_grep_error_event` +
      `log_grep_error` + both call sites; `stop_recording.rs::
      recording_stopped_telemetry`; `recording_controller.rs::
      telemetry_key`; `api_keys.rs::send_provider_credential_telemetry`
      + 2 helpers + `was_present`/`is_present` call-site guards;
      `agent_sdk::run` event binding + `command_to_telemetry_event`;
      `driver.rs` RuntimeErrorDetected spawn block; `run_agents.rs` /
      `run_agents_card_view.rs` `emit_decision` +
      `emit_orchestration_entered_once` + `decision_event_emitted`/
      `entered_event_emitted` guards; `orchestration_pill_bar.rs`
      5 telemetry fns + `PillClicked.is_breadcrumb` field (no reader
      left) + 7 call sites; `orchestration_config_block.rs` 2 empty
      emit fns + `status`/`snapshot_loaded` locals;
      `block.rs` surfaced-citations + for_telemetry if-let +
      `server_output_id` let; `conversation.rs` orphaned
      `AIIdentifiers` destructure; `agent/mod.rs` dead
      `telemetry_events: Vec<TelemetryEvent>` field + 3 `vec![]` inits;
      `response_stream.rs::emit_retryable_agent_mode_error_telemetry` +
      4 call sites + `original_error` field; `callout/model.rs::
      send_callout_displayed_telemetry`; `lib.rs::AppStartup` event
      (FIRST_FRAME_DRAWN mark_interval_end kept); `auth_manager.rs`
      login record_identify/record spawn block (DB persist above
      stays); `quit_warning::close_target`;
      `system/info.rs` ResourceUsageReporter + StatsBuffer + Cpu/Memory
      stats + `handle_block_created` + view.rs call;
      `input.rs` PageUp/Down event lets; `command_palette::close`
      buffer_length/filter locals; `mcp/native.rs::should_send_telemetry`
      + closure telemetry tail; `alias_bar` env_vars_space; input.rs
      workflow `space` let; `workspace/view.rs` entrypoint/cli_agent
      fields of RightPanelUpdateParams; shell_terminated_banner
      `Premature.reason` field;
      `malformed_line_heuristics.rs` (deleted);
      `diff_state` DiffOperation enum + BackendOrigin enum +
      backend_origin field + constructor params (all callers
      updated); `telemetry_event.rs::GitButtonKind`/
      `AddToContextOrigin` enums (orphaned);
      `suggested_rule_modal` rule-field event payloads; `code_diff_view
      edit_format_kind` field + `file_context_range_to_editor_range`;
      `agent/telemetry.rs::ForTelemetry` + block.rs for_telemetry
      callsites; `available_shells::telemetry_value`; onboarding
      `send_callout_displayed_telemetry`; lifecycle
      `telemetry_limiter` KEPT (live diagnostics).

      WARP_CLI: `Command::PrintTelemetryEvents` variant + hidden-check
      arm + `prints_to_stdout` arm + app `lib.rs` dispatch arm all
      deleted. DIRECT CALL (4ez landmine): already deleted in 4fa
      (SessionAbandonedBeforeBootstrap block) — confirmed gone at HEAD.

      Deliberately left (2b/2c/3 per 4ez): warpui_core/src/telemetry/
      queue; warp_core/src/telemetry.rs machinery + queue macros
      (untouched, verified); `server_api.rs::send_telemetry_event`
      no-op stub (2b — no remaining callers now, deletion trivial next
      round); `context_provider.rs` + lib.rs registration; notebook
      queue read; PrivacySettings telemetry fields + privacy-page
      toggle; `telemetry_banner.rs` + `should_collect_ai_ugc_telemetry`
      consumers; channel-config telemetry_config + feature flags;
      `AgentModeAnalytics` force.

      4ez-PLAN CORRECTION (substantive): 4ez claimed events.rs's ~70
      payload types were "constructed only at macro sites". False for
      ~50 types: `ImageProtocol` (terminal grid/ANSI handler),
      `PtySpawnMode` (PTY spawner), `CLIAgentType` (cli_agent),
      `CloseTarget` (quit_warning), `FindOption`, `PromptChoice`,
      `DownloadSource`, `PaletteSource`, `LaunchConfigUiLocation`,
      `AIIdentifiers`, `EntrypointType::entrypoint()`,
      `AgentModeEntrypoint`, `CLISubagentControlState`,
      `BootstrappingInfo`, `SlowBootstrapInfo`, `InteractionSource`,
      `SecretInteraction`, `PromptSuggestionFallbackReason`,
      `AgentModeRewindEntrypoint`, `CodeReviewPaneEntrypoint`,
      `GitOperationKind`/`GitDialogStatus`, `RequestFileEditsFormatKind`,
      `LspEnablementSource`/`LspControlActionType`, `SkillOpenOrigin`,
      tab_configs enums, `NotebookTelemetryMetadata` chain (notebook
      views), `AgentModeSetup*ActionType`, `AgentModeEntrypoint
      SelectionType`, `ToggleCodeSuggestionsSettingSource`,
      `MCPTemplateInstallationSource`, `PricingPromotionSurface`/
      `State`, `SaveAsWorkflowModalSource`, `AnonymousUserSignup
      Entrypoint`, `EnvVarTelemetryMetadata`, `CloudObjectTelemetry
      Metadata`, `TelemetryCloudObjectType`, `WorkflowTelemetryMetadata`,
      `VerticalTabsChipEntrypoint`/`DisplayOption`,
      `AgentModeSetup*`, etc. are constructed/threaded in live UI code.
      Keeping them (in events.rs, path unchanged to avoid 140-file
      import churn) avoids risky live-code rewrites; 19 of the 29
      "dead" candidates in the pre-pass were confirmed dead only after
      the post-fix compile. This is the designated follow-up: relocate
      the ~30 live types to proper modules (or delete the vestigial
      threading) as slice 2a′.

      Local-only safety: every deleted statement was enqueue-only (the
      macros do nothing besides the enablement check + queue push; the
      queue is never drained in this fork post-4fa). All deletions are
      producer-side; no local behavior (UI, logging, Sentry, DB,
      network logging pane, secret redaction) changed.

      Acceptance: clippy baselines captured at HEAD FIRST in both
      configs — 12 sorted warning+location pairs each (11
      needless-return in terminal/input.rs + 1 single-element-loop in
      lifecycle/mod_tests.rs); post-edit runs are warning-IDENTICAL in
      both configs (same 12 warnings; line numbers shifted within the
      same files due to in-file deletions — 10926→10705 etc.).
      `./script/format` idempotent (stable diff at 178 files).
      Check suite exit 0: `check -p warp --lib --all-targets` default +
      simplewarp, `--no-default-features --features simplewarp --bin
      simplewarp`, `--bin warp-oss`, `--all-targets -p integration`
      (only the two pre-existing step.rs unused-import warnings),
      `check -p warp --lib --tests --features skip_login`, plus
      `-p ai/-p onboarding/-p repo_metadata/-p warp_cli --all-targets`
      — all 0 errors. Nextest `-p warp --lib --no-fail-fast`: default
      4,611 passed / 4 skipped / 0 failed; simplewarp 4,610 passed /
      4 skipped / 0 failed; delta vs baseline −20/−19, exactly the 20
      removed telemetry-only tests verified by nextest list name-diff
      against HEAD: stop_recording_tests 5, recording_telemetry_tests
      4, malformed_line_heuristics tests 4, blocklist telemetry_tests
      2, events_tests 1, info_tests 1, grep_tests 1 (redaction),
      lifecycle mod_tests 1 (payload allowlist),
      pricing_promotion_tests 1 (payload surface), crates/ai
      telemetry_tests 1, crates/onboarding telemetry_tests 1. Runtime
      smoke SKIPPED per the 2026-09-23 convention (user away, password
      prompt unanswerable); no-TCP claim rests on the static gating
      analysis, which this slice only shrinks further (producers now
      gone). Did not `cargo clean`. No .md files other than plan.md.

      DESIGNATED NEXT: slice 2b per 4ez — warpui_core queue +
      app_focus_telemetry, warp_core telemetry.rs machinery + queue
      macros, `server_api.rs::send_telemetry_event` stub (now fully
      callerless), context_provider.rs + registrations, notebook
      queue read; PLUS slice 2a′: relocate or delete the ~30 retained
      domain types in `server/telemetry/events.rs` (now 362 lines,
      doc'd at top) and the vestigial payload-threading through live
      fns (e.g. PaneDragDrop `source: PaletteSource` params) —
      mechanical, low-risk, recommended before 2c.

- [x] **telemetry slice 2b — queue and machinery (4fc) — DONE 2026-09-23.**
      Executed the 4ez SLICE 2b: the event queue and everything that
      existed to buffer events between producer and the slice-1-deleted
      sender is gone. 41 files changed, 193 insertions(+), 1,118
      deletions(-) (git diff --stat vs 4fb HEAD b50a55edc, includes
      plan.md; code-only: 40 files, +5/−1,118).

      DELETED wholesale (7 files, 884 lines; each re-verified at HEAD
      before removal): `crates/warpui_core/src/telemetry/` — mod.rs (112;
      the TELEMETRY lazy_static global + record_event/
      record_identify_user_event/record_app_active_event/flush_events/
      create_event + the record_telemetry_from_ctx!/
      record_telemetry_on_executor! macros, whose only callers were
      warp_core's send macros — zero invocation sites since 4fb) +
      event_store.rs (204; the BoundedVecDeque-1024 EventStore) +
      event_store_tests.rs (151); `crates/warp_core/src/telemetry.rs`
      (262; TelemetryEvent/RegisteredTelemetryEvent/TelemetryEventDesc
      traits, register_telemetry_event! + the inventory
      TelemetryEventRegistration/AnyTelemetryEventRegistration
      cross-crate collect + enum_events/all_events, EnablementState, the
      send_telemetry_from_ctx!/send_telemetry_from_app_ctx! queue macros,
      TelemetryContextProvider/TelemetryContextModel + their
      Entity/SingletonEntity impls, and the cfg(any(test, feature =
      "test-util")) MockTelemetryContextProvider);
      `app/src/server/telemetry/context_provider.rs` (26;
      AppTelemetryContextProvider — its only reader was the deleted
      macros' identity read); `crates/warpui_core/src/
      app_focus_telemetry.rs` (88) + `app_focus_telemetry_tests.rs` (41;
      AppFocusInfo/DailyAppFocusDuration — its sole output was
      crate::telemetry::record_event).

      DELETED from surviving files: `warpui_core/src/lib.rs` the
      `mod app_focus_telemetry;` + `pub mod telemetry;` decls;
      `warpui_core/src/core/app.rs` the app_focus_info field +
      AppFocusInfo import + `AppFocusInfo::new()` init + the now-
      callerless pub App methods record_app_focus/record_app_blur/
      try_record_daily_app_focus_duration; `app/src/lib.rs` the
      AppTelemetryContextProvider import + production registration
      (was :1139) and all three app_callbacks sites — on_become_active's
      body was ONLY the record_app_focus read so the callback entry is
      now `None` (AppCallbacks.on_become_active is Option), the
      record_app_blur tail left on_resigned_active, the
      try_record_daily_app_focus_duration block left on_will_terminate
      (NotebookManager close, PersistenceWriter terminate, LSP shutdown,
      pty teardown all stay); `server/server_api.rs` the
      `send_telemetry_event` no-op stub (grep: zero callers); `telemetry/
      mod.rs` shell 4→3 lines (context_provider decl dropped; the events
      decl + re-export stay for 2a′). warp_core `lib.rs`: `pub mod
      telemetry;` AND the `pub use warpui_core;` re-export + its comment
      (stated purpose: "so that it can be referenced safely from the
      telemetry macros"; grep `warp_core::warpui_core` and
      `crate::warpui_core`: only the deleted macros used it).
      `crates/warp_core/Cargo.toml`: the `inventory` dependency removed —
      telemetry.rs was the only warp_core user (settings + warp_errors
      keep their own inventory deps; app keeps its dep for
      SettingSchemaEntry). Cargo.lock −1 line. `remote_server/unix/
      mod.rs`: stale comment trimmed (it claimed the DAEMON_SOCKET_BOUND
      IntervalTimer mark was "flushed as telemetry" via
      AppTelemetryContextProvider; mark_interval_end only emits
      tracing::info! and the call stays).

      LANDMINES: (4ez #4) TelemetryContextModel registrations — the 4ez
      map said "five test files + test_util/terminal.rs:99"; the real
      count at HEAD was 19 app files (import +
      `add_singleton_model(AppTelemetryContextProvider::
      new_context_provider)` each; current_prompt_tests ×4
      registrations): workspace/view_tests, pane_group/mod_tests,
      terminal/input_tests, workspaces/user_workspaces_tests,
      search/command_search/{searcher,view}_tests,
      ai/blocklist/prompt/prompt_alert_tests,
      ai/blocklist/history_model_tests, view_components/find_tests,
      drive/{index,panel}_tests, uri/docker_tests, notebooks/{manager,
      file/mod,notebook}_tests, code_review/{code_review_view,
      find_model,diff_state/remote}_tests, context_chips/
      current_prompt_tests, test_util/terminal.rs. All were write-only
      boilerplate since 4fb (nothing reads the singleton anymore):
      registrations + imports deleted; every file's other registrations
      (AuthStateProvider, AuthManager, …) untouched. (4ez #5)
      notebook_tests queue reader: `test_edit_telemetry` (#[test]
      #[ignore], was :327-414) was the one LOCAL queue reader
      (`warpui::telemetry::flush_events()` filtering "Notebook Edited"
      NamedEvents) — it tested the notebook edit-mode telemetry timer
      through the queue; with producer (4fb) and queue (this round) both
      gone it died whole, together with its private `ensure_saved`
      helper (its only callers were inside the test) and the now-unused
      Itertools/Timer/EventPayload imports +
      EDIT_WINDOW_DURATION/SAVE_PERIOD super-imports (Mode stays —
      other tests use it). MockTelemetryContextProvider users (4 files,
      test harnesses only): app ai/request_usage_model_tests,
      ai/blocklist/action_model/execute/run_agents_tests (:169),
      crates/warp_search_core/mixer_tests, crates/ai/api_keys_tests
      (×2) — registrations deleted. DIRECT FALLBACK: mixer_tests'
      harness fn `initialize_app(app: &mut App)` became an EMPTY fn with
      3 call sites — deleted the fn and all calls (no `_app` per
      AGENTS.md). (THE ~30 LIVE TYPES) verified BEFORE deleting the
      trait: none of the events.rs shell types implements or derives
      TelemetryEvent — they are plain serde data enums/structs (grep:
      zero `impl TelemetryEvent` anywhere outside the deleted file, zero
      register_telemetry_event! sites since 4fb); the trait deletion
      therefore deletes no impls, and every `crate::server::telemetry::X`
      import across app/src still compiles unchanged.

      Deliberately left: `app/src/server/telemetry/events.rs` (362) +
      the mod.rs re-export — the 2a′ relocation shell; slice 2c privacy
      telemetry-only fields (PrivacySettingsSnapshot,
      is_telemetry_enabled/is_telemetry_force_enabled, the privacy-page
      toggle, AgentModeAnalytics force); slice 3 (channel-config
      telemetry_config/TelemetryConfig/RudderStack*/
      telemetry_file_name, execution_mode::send_telemetry_at_shutdown,
      WithSandboxTelemetry/SendTelemetryToFile/RecordAppActiveEvents
      flags, warpui_core's now code-unreferenced
      `log_named_telemetry_events` cargo feature decl, the
      EnablementState-flag sweep). NOTE for the next slice:
      notebook.rs still carries the gutted edit-telemetry timer
      skeleton (check_edited/edit_telemetry_handle/send_edit_telemetry/
      last_content_length — wakes every EDIT_WINDOW_DURATION to compute
      a delta and discard it) plus the parameterless send_telemetry_
      action stub and the NotebookTelemetryAction enum — producer-side
      residue outside 2b scope, still compiled and live, best folded
      into 2a′/2c.

      Local-only safety: the queue was write-only with zero drainers
      since 4fa (4ez RUNTIME ANSWER: all three collector pollers were
      never scheduled in this fork; the only drainers ever were the
      deleted flush paths and test_edit_telemetry's flush_events()).
      Exhaustive grep at HEAD before deletion: the queue functions' only
      callers outside the module were app_focus_telemetry (died here),
      warp_core's macros (callerless since 4fb), and the notebook test
      (died here). All deletions are of code whose only observable
      effect was filling an in-memory bounded buffer that was never
      read; no log line, UI path, DB write, or file write changed. The
      app-callback deltas (on_become_active now None; the blur/
      terminate callbacks keep their non-telemetry work) are
      user-invisible — the focus/blur reads only fed duration
      bookkeeping for the deleted queue.

      Acceptance: clippy baselines captured at HEAD FIRST in BOTH
      configs — 12 sorted warning+location pairs each (11
      needless-return in terminal/input.rs + 1 single-element-loop in
      lifecycle/mod_tests.rs); post-edit re-runs warning-IDENTICAL in
      both configs (24 sorted pairs before and after, 0 new / 0 gone).
      Check suite exit 0, 0 errors, 9/9: `check -p warp --lib
      --all-targets` default (0 warnings) + simplewarp (clean),
      `--no-default-features --features simplewarp --bin simplewarp`
      (clean), `--bin warp-oss` (clean), `--all-targets -p integration`
      (only the two pre-existing step.rs unused-import warnings,
      single_terminal_view_for_tab + crate::terminal::CLIAgent),
      `check -p warp --lib --tests --features skip_login` (clean),
      `-p warpui_core --all-targets` (clean), `-p warp_core
      --all-targets` (clean), `-p warp_cli --all-targets` (clean).
      `./script/format` idempotent (zero unstaged changes after the
      run). Nextest `-p warp --lib --no-fail-fast`: default 4,611
      passed / 3 skipped / 0 failed; simplewarp 4,610 passed / 3
      skipped / 0 failed — passed counts IDENTICAL to the post-4fb
      baseline (4,611/4,610); skipped dropped 4→3 in both configs
      because the only deleted test, test_edit_telemetry, was
      #[ignore]d (it was the only #[ignore] in app/src and thus one of
      the 4 baseline skips). `cargo nextest run -p warpui_core
      --no-fail-fast`: 302 passed / 7 skipped / 0 failed (first
      warpui_core ledger baseline; one lower than pre-round by exactly
      the deleted app_focus test). No flakes. ENVIRONMENT INCIDENT: the
      data volume hit 0 bytes free during the first simplewarp-config
      test build (rustc ENOSPC; both feature configs' artifacts share
      target/), stalling even shell launches for a few minutes; fixed
      by deleting the five stale ~650MB `warp-<hash>` lib-test
      executables from target/debug/deps (relinkable from cached
      rlibs — NOT cargo clean; no rlib/rmeta/fingerprint touched), then
      the simplewarp + warpui_core runs completed above. Runtime smoke
      SKIPPED per the 2026-09-23 convention (user away, macOS password
      prompt unanswerable); the GUI binary was not built or launched;
      the no-behavior-change claim rests on the static write-only-
      queue analysis above, which this slice removes in full. Did not
      `cargo clean`.

      DESIGNATED NEXT: slice 2a′ + 2c in one round if 2a′ stays
      mechanical — relocate the ~30 live domain types out of
      `server/telemetry/events.rs` into proper modules (or delete the
      vestigial threading: notebook.rs's send_telemetry_action stub +
      edit-telemetry timer skeleton, PaneDragDrop's PaletteSource
      params), delete the events.rs shell + telemetry/mod.rs; then 2c
      (PrivacySettingsSnapshot whole type, is_telemetry_enabled setting
      + model field + UpdateIsTelemetryEnabled plumbing,
      is_telemetry_force_enabled user_workspaces/gql_convert plumbing,
      the privacy-page toggle section, settings-schema regeneration
      eyeball). If the relocation proves entangled (import churn
      across 100+ files), do 2c alone and split 2a′ into per-cluster
      slices.

- [x] **telemetry slices 2a′ + 2c — events shell dissolved + privacy fields
      (4fd) — DONE 2026-09-23.** `server/telemetry/` is gone: events.rs
      (362) + mod.rs (3) + the `pub mod telemetry;` decl deleted, all
      `crate::server::telemetry::` import paths eliminated (PCRE
      sweep: zero hits; also caught the sneaky
      `crate::terminal::view::telemetry::PromptSuggestionFallbackReason`
      path in view.rs:430 that resolved THROUGH the `use ...::{self,
      ...}` binding at :303 — a path-through-binding pattern to watch
      for in future module dissolutions). Code diff vs 4fc HEAD
      74d556162: 54 files, +137/−1,434 (includes crates/integration;
      plan.md excluded).

      Of the 29 types 4fb kept in events.rs as "live-shared", only 2
      had genuinely live consumers — RELOCATED as pure moves (2):
      `PaletteSource` → `search/command_palette/mod.rs` (LIVE:
      workspace/view.rs OpenPalette/TogglePalette handlers branch on
      `matches!(source, CtrlTab{..})` to pick ctrl_tab_palette vs
      palette, plus TitleBarSearchBar checks; emitted from lib.rs quit
      modal ×2, auth log-out modal, agent_tips, slash_commands
      keybinding, pane_group QuitModal, workspace/mod.rs keybindings,
      workspace/action.rs payloads) and `CommandXRayTrigger` →
      `editor/view/mod.rs` next to sibling `CommandXRayAnchor` (LIVE:
      input.rs show_xray's `trigger == Keystroke` decides whether the
      x-ray description is announced via a11y). RELOCATION LANDMINE:
      keep derives identical to the source — the original was
      Clone-only (no Copy) and deriving Copy broke the non-move
      closure in start_xray_at_offset (E0373: non-move closures
      capture even Copy types by reference; Clone-only ones move them
      by value). The other 27 types were DEAD after re-verification —
      every remaining reader was a discarded `let _x`/`_param`
      threading that only existed to feed deleted producers — deleted
      together with their threading (27):
      DownloadSource (whole file download_method.rs 78 lines: spawned
      a task that ran `brew list --cask warp` on macOS into a
      discarded `let _download_source`; lib.rs call + mod decl gone),
      TelemetryCloudObjectType + TelemetrySpace +
      CloudObjectTelemetryMetadata (only builders were the two
      `#[cfg_attr(not(wasm), allow(dead_code))]` zero-caller fns
      notebook.rs::generic_telemetry_metadata and
      workflow_view.rs::telemetry_metadata — both deleted),
      WorkflowTelemetryMetadata (view.rs built it then
      `if let Some(_metadata) = ... {}`; WorkflowSelectionSource
      import orphaned), EnvVarTelemetryMetadata (input.rs dead let),
      MCPTemplateInstallationSource (list_page.rs dead let; the
      is_server_template_shared method itself keeps 7 live callers),
      CLIAgentType + From<CLIAgent> impl (readers: use_agent_footer
      discarded let + one test assert on the impl itself), 
      NotificationAgentVariant (`_agent_variant` param of
      send_agent_desktop_notification_or_show_banner + 2 sites),
      PtySpawnMode (spawner.rs discarded let + the write-only
      is_fallback bookkeeping; the fallback spawn path and
      report_error! stay), SaveAsWorkflowModalSource (`_source` param
      of open_workflow_modal_with_command + 2 sites),
      LaunchConfigUiLocation (OpenLaunchConfigArg.ui_location never
      read by open_launch_config; field + 4 app constructors +
      type_getters.rs getter (file emptied, deleted, mod decl
      removed) + 7 crates/integration construction lines),
      AICommandSearchEntrypoint (dead let in show_ai_command_search),
      AnonymousUserSignupEntrypoint (payload threading
      InputEvent→TerminalView Event→terminal_pane→pane_group Event→
      initiate_user_signup(_entrypoint) whose body only opens the
      require-login modal; field removed from all 3 event variants +
      re-emits + 5 direct constructor sites + fn param),
      PromptChoice (editor_modal dead `let _prompt_info`;
      context_chips::telemetry_name() had it as sole caller — method
      deleted), ToggleBlockFilterSource (TerminalAction payload,
      handler ignored; Binding keybinding + ContextMenu menu sites +
      Display arm), AgentModeEntrypoint +
      AgentModeEntrypointSelectionType (NewTab/NewPaneInAgentMode
      `entrypoint: _` arms ignored it; field removed from both
      variants + NewPaneBinding binding + integration-test TabBar
      constructor + the lib.rs `pub use
      crate::server::telemetry::{AgentModeEntrypoint,
      AgentModeEntrypointSelectionType}` re-export — note
      input.rs:299's `#[allow(unused_imports)]` line had been masking
      the dead re-export import), InteractionSource
      (PromptSuggestionResolution::Accept{interaction_source} →
      `let _interaction_source = match ...` discard; variant became
      unit `Accept`, 4 constructors + block.rs 5 accept/dismiss sites
      updated), AgentModeRewindEntrypoint (RewindAIConversation
      payload ignored by show_rewind_confirmation_dialog; Button/
      ContextMenu/SlashCommand constructor sites),
      PromptSuggestionFallbackReason (PassiveCodeDiffFailed{reason};
      handler ignored; 7 emission sites in legacy.rs — the
      early-return control flow around each emit kept),
      AgentModeAutoDetectionFalsePositivePayload (
      maybe_send_autodetection_telemetry_on_manual_toggle was 100%
      dead computation — fn + both call sites deleted),
      AddTabWithShellSource (AddTabWithShell source payload →
      add_tab_with_shell(_source) ignored; ShellSelectorMenu +
      CommandPalette constructors), SharingDialogSource (
      WorkspaceAction::OpenObjectSharingSettings had ZERO constructors
      repo-wide and a `{ .. } => {}` handler arm — the whole variant +
      2 arms deleted), ImageProtocol (Event::ImageReceived +
      ModelEvent::ImageReceived payload; sole subscriber matched
      `image_protocol: _`; 4 ansi_handler emission sites),
      QueuedPromptSendNowTrigger (send_queued_row_immediately(_trigger)
      ignored; SendNowButton + EnterOnEmptyInput sites),
      PromptSuggestionViewType (prompt_suggestion_view_type() fed only
      `let _view = ...`).

      4fc-DESIGNATED EXTRAS: notebooks/telemetry.rs's
      NotebookTelemetryAction enum deleted — it only fed the EMPTY
      `send_telemetry_action(_action, _ctx)` stubs in notebook.rs and
      file/mod.rs (16 call sites); ActionEntrypoint/BlockInfo/
      EmbeddedObjectInfo/SelectionMode stay (live in the notebook
      editor's copy/cut/insert/selection paths). The gutted
      edit-telemetry timer skeleton deleted: check_edited (woke every
      EDIT_WINDOW_DURATION to compute `let _delta` and discard it),
      edit_telemetry_handle/send_edit_telemetry/last_content_length
      fields + inits + reset sites, both cfg'd EDIT_WINDOW_DURATION
      consts, Timer/AbortHandle imports; SAVE_PERIOD + the throttled
      save stream stay (live). EditorViewEvent::
      OpenedBlockInsertionMenu(BlockInsertionSource) payload removed —
      both subscribers ignored it; the type itself stays live in the
      insertion menu. Same-class ugc residue swept:
      input.rs/blocked `_should_collect_ugc`, input_model.rs
      `_buffer_length`/`_input_buffer_text_for_telemetry`/
      `other_buffer_cloned`/`_is_udi_enabled`, block.rs
      `should_collect_ugc`/`_redacted_query` dead block,
      view.rs `_query_string`/`_block_command` dead if-let,
      PassiveSuggestionsEvent::PromptSuggestionsGenerated lost its
      write-only command/request_duration_ms fields.

      PART B (2c) — re-verified at HEAD before touching; the 4ez 2c
      premise partially dissolved because the flag's readers were NOT
      all in the deleted pipeline: DELETED: PrivacySettingsSnapshot
      whole type + its 5 accessors + should_disable_telemetry + mock()
      + PrivacySettings::get_snapshot — zero callers repo-wide (the
      4ez-era readers incl. terminal/view.rs's snapshot field died in
      4fa/4fb); this also removes the AgentModeAnalytics force
      coupling (4ez landmine #6, explicitly blessed). DELETED:
      AppAnalyticsWidget (privacy_page.rs) + ZDR badge + TELEMETRY_
      TITLE/TELEMETRY_DESCRIPTION{_OLD}/TELEMETRY_DOCS_URL consts +
      widget-list entry — should_render returned false in every build
      of this fork (ChannelState::is_telemetry_available() == false:
      all three bins pass telemetry_config: None), so it never
      rendered. KEPT with live local readers: `is_telemetry_enabled`
      (model field + WarpDrivePrivacySettings setting + setter + cloud
      -sync subscribe arm + UpdateIsTelemetryEnabled event variant)
      — read by check_and_trigger_telemetry_banner_for_existing_users
      (workspace/view.rs), the flags::TELEMETRY_FLAG context flag, and
      the LIVE "app analytics" toggle binding in
      init_actions_from_parent_view (PrivacyPageAction::
      ToggleTelemetry + toggle_telemetry handler stay; the toggle is
      reachable outside the deleted widget via the settings-toggle
      bindings path); `is_telemetry_force_enabled` (field + getter/
      setter + user_workspaces teams plumbing) — read by
      CrashReportsWidget::should_render's org-force gate (deviation
      from the 4ez 2c list: the gate IS a live reader; note
      is_crash_reporting_available() is also false in this fork's
      bins); crash reporting, cloud conversation storage, user/
      enterprise secret regexes: untouched. KEPT for slice 3:
      TerminalModel's is_ai_ugc_telemetry_enabled plumbing +
      should_collect_ai_ugc_telemetry + TelemetryBanner UI — the flag
      switches block.rs serialized output between content_summary
      (2500,2500) and force-obfuscated truncation, i.e. REAL content
      behavior, not telemetry-only. Settings schema: no
      define_settings_group/maybe_define_setting definitions changed,
      so nothing to regenerate; verified by running `cargo run --bin
      generate_settings_schema` → 193 settings written,
      privacy.telemetry_enabled still present as expected.

      Deliberately left (slice 3 per 4ez): channel-config
      telemetry_config/TelemetryConfig/RudderStack*/
      telemetry_file_name/is_telemetry_available + the six-bin
      telemetry_config: None decision, execution_mode::
      send_telemetry_at_shutdown, warp_features WithSandboxTelemetry/
      SendTelemetryToFile/RecordAppActiveEvents + app cargo features
      send_telemetry_to_file/record_app_active_events + warpui's
      unreferenced log_named_telemetry_events cargo feature, the
      EnablementState-flag sweep (flags newly callerless after 4fb),
      the is_ai_ugc_telemetry_enabled/should_collect_ai_ugc_telemetry/
      TelemetryBanner cluster (above), AgentModeAnalytics flag itself,
      and rename candidates for now-misnamed telemetry-named modules
      that hold live types (notebooks/telemetry.rs,
      workspace/view/vertical_tabs.rs telemetry mod, tab_configs/
      telemetry.rs, code_review/telemetry_event.rs,
      ai/skills/telemetry.rs).

      Local-only safety: every deleted statement was either (a) an
      enqueue-only telemetry producer remnant, (b) a value computed
      and bound to an underscore/discarded name with zero readers
      (each verified by PCRE grep at HEAD before deletion), or (c)
      the DownloadSource `brew` probe whose result was discarded
      (deleting it removes an invisible background subprocess spawn,
      nothing else). The Action/Event payload removals touch
      dispatch/emit plumbing only where the field was provably never
      read; all handler bodies, control flow around emissions
      (legacy.rs early returns), the resolve_prompt_suggestion
      Reject path, and the rewind confirmation flow are unchanged.
      CommandXRayTrigger moved verbatim (derives adjusted back to the
      original Clone-only set — see landmine above; Debug was added,
      which is inert). No signature changed beyond dropping provably
      unread parameters/fields; no visibility widened beyond the two
      relocated enums.

      Acceptance: clippy baselines captured at HEAD FIRST in both
      configs — 12 sorted warning|location pairs each (11
      needless-return in terminal/input.rs + 1 single-element-loop in
      lifecycle/mod_tests.rs:272); post-edit re-runs are 12 pairs in
      both configs, warning-IDENTICAL modulo in-file line shifts in
      terminal/input.rs (10705→10642 etc., from deletions above them)
      — 0 new, 0 gone by normalized warning+file multiset diff.
      Check suite 0 errors, 7/7 post-incident re-run:
      `check -p warp --lib --all-targets` default + simplewarp,
      `--no-default-features --features simplewarp --bin simplewarp`,
      `--bin warp-oss`, `--all-targets -p integration` (only the two
      pre-existing step.rs unused-import warnings),
      `check -p warp --lib --tests --features skip_login`,
      `-p warp_cli --all-targets`. `./script/format` idempotent
      (second run: zero new changes). Nextest `-p warp --lib
      --no-fail-fast`: default 4,611 passed / 3 skipped / 0 failed;
      simplewarp 4,610 passed / 3 skipped / 0 failed — passed and
      skipped counts IDENTICAL to the post-4fc baseline. INCIDENT:
      the data volume hit 100% during the first simplewarp test build
      (ENOSPC → 4 spurious workspaces::workspace purchase_policy
      FAILs, all temp/DB-writing tests); freed ~2GB by deleting stale
      relinkable target/debug/deps executables before the round's
      builds, macOS purged more under pressure, and the full run
      passed on rerun with disk restored (~16-19GB free) — rerun
      before attributing, per the standing flake rule; a post-incident
      re-run of all 7 check commands was 0 errors. Runtime smoke
      SKIPPED per the 2026-09-23 convention (user away, macOS
      password prompt unanswerable); the GUI binary was not built or
      launched. Did not `cargo clean`.

      DESIGNATED NEXT: slice 3 per 4ez, now the only remaining
      telemetry work — (1) channel-config telemetry_config family
      (TelemetryConfig/RudderStack*/telemetry_file_name/
      is_telemetry_available; touches all three remaining bins that
      pass `telemetry_config: None` — decide removal vs keep) and
      crash_reporting_config symmetric decision, (2) execution_mode::
      send_telemetry_at_shutdown, (3) the three feature flags
      WithSandboxTelemetry/SendTelemetryToFile/RecordAppActiveEvents
      + their cargo features + warpui's log_named_telemetry_events
      feature decl, (4) the EnablementState-flag sweep is moot
      (EnablementState itself died in 4fc) — replace with the
      is_ai_ugc_telemetry_enabled/should_collect_ai_ugc_telemetry/
      AgentModeAnalytics/GlobalAIAnalyticsBanner flag cluster, which
      needs a content-behavior decision on block.rs's serialized
      output truncation before any deletion, (5) rename pass for the
      five now-misnamed telemetry modules holding live types.

- [x] **telemetry slice 3 — flags and config plumbing (4fe) — DONE
      2026-09-23.** Executed the 4ez SLICE 3: the switches that
      existed to turn the deleted pipeline on are gone. 17 files, 5
      insertions(+), 139 deletions(-).

      PER-ITEM VERDICTS. (1) Channel-config family DELETED:
      `ChannelConfig.telemetry_config` + `TelemetryConfig` +
      `RudderStackConfig` (with its non_ugc/ugc_destination methods)
      + `RudderStackDestination` (channel/config.rs), and
      ChannelState's `telemetry_file_name` / `is_telemetry_available`
      / `rudderstack_non_ugc_destination` /
      `rudderstack_ugc_destination` accessors + the `init()`
      `telemetry_config: None` (channel/state.rs). Zero callers at
      HEAD for all four accessors (4fa/4fb already consumed the
      pipeline readers); the explicit `telemetry_config: None,`
      constructor lines fell out of all four remaining bins
      (app/src/bin/{simplewarp,oss,integration}.rs, crates/integration
      /src/bin/integration.rs — dev/stable/preview/local deserialize
      via `load_config!`, and serde ignores the now-unknown key in
      generator output, so config loading is unchanged). The family's
      last live reader chain fell too: warp_logging/native.rs's
      telemetry-file rotation branch
      (`SendTelemetryToFile.is_enabled()` →
      `ChannelState::telemetry_file_name()`), incl. the FeatureFlag
      import; `rotate_log_files` now rotates logs only. (2)
      execution_mode::send_telemetry_at_shutdown DELETED (10 lines) —
      zero callers since 4fa killed the shutdown flush;
      ExecutionMode::Sdk/RemoteServerDaemon stay live via
      is_autonomous. (3) FeatureFlag variants DELETED from
      warp_features: WithSandboxTelemetry (zero is_enabled readers at
      HEAD — the collector gates died in 4fa; sole enabler was
      bin/local.rs under the WITH_SANDBOX_TELEMETRY env var),
      RecordAppActiveEvents (zero readers; the usage poller died in
      4fa), SendTelemetryToFile (one reader — the rotation branch
      above). Their enablement plumbing went with them:
      app/src/features.rs's two cfg-gated entries, local.rs's
      WITH_SANDBOX_TELEMETRY block (`let mut` → `let`), app cargo
      features `record_app_active_events` + `send_telemetry_to_file`,
      the `"warpui/log_named_telemetry_events"` entry in app's
      agent_mode_evals, warpui's + warpui_core's
      `log_named_telemetry_events` feature declarations (zero
      `cfg(feature = ...)` readers remained after 4fc deleted the
      queue — pure declaration residue), the
      `with_sandbox_telemetry:WITH_SANDBOX_TELEMETRY` legacy-feature
      mapping line in script/run + script/wasm/bundle, and
      app/build.rs's `rerun-if-env-changed=WITH_SANDBOX_TELEMETRY`.
      NOT touched: the `release_bundle` cargo feature +
      `ChannelState::is_release_bundle()` — packaging machinery with
      live non-telemetry consumers (app_services, login_item,
      warp_channel_config, profiling); its only telemetry reader
      (collector.rs) died in 4fa. (4) EnablementState: already gone —
      the telemetry type died in 4fc with warp_core/src/telemetry.rs;
      the `EnablementState` surviving in
      app/src/ai/persisted_workspace.rs is the unrelated LSP-server
      enablement type with live persistence/sqlite readers. Nothing
      dangles. (5) AppAnalyticsWidget residue: none (4fd deleted the
      widget; zero refs). KEPT as live local behavior, none of it
      implied by the three deleted flags: `flags::TELEMETRY_FLAG` is
      a &str context-flag const in settings_view/mod.rs (a separate
      mechanism from the FeatureFlag enum — not one of the 4ez
      three), the "app analytics" palette toggle binding
      (privacy_page.rs init_actions_from_parent_view →
      PrivacyPageAction::ToggleTelemetry → toggle_telemetry →
      set_is_telemetry_enabled), the `is_telemetry_enabled` setting +
      WarpDrivePrivacySettings plumbing + UpdateIsTelemetryEnabled
      event, `is_telemetry_force_enabled` (CrashReportsWidget
      org-force gate), and TelemetryBanner +
      check_and_trigger_telemetry_banner_for_existing_users
      (workspace/view.rs, defined :6896 called :9832) — deleting any
      of these would remove reachable local UI, violating the
      zero-behavior-change constraint.

      Deliberately left: TerminalModel's is_ai_ugc_telemetry_enabled
      / should_collect_ai_ugc_telemetry / block.rs serialized-output
      truncation (user decision pending — real content behavior); the
      five telemetry-named module renames (cosmetic, skipped);
      `.agents/skills/add-telemetry` SKILL.md, which still documents
      the deleted pipeline + `log_named_telemetry_events` (lockfile-
      managed common skill — orchestrator call); the remote GraphQL
      `TelemetrySettings` workspace types (item-8 endgame);
      crash_reporting_config (non-telemetry, live readers via
      sentry_url / is_crash_reporting_available).

      Local-only safety: every deleted switch was already inert in
      this fork's builds — the 4ez four-brakes analysis now holds
      structurally: there is no telemetry_config to populate, no flag
      to set, no accessor to call. SendTelemetryToFile's sole reader
      guarded rotation of a file nothing writes anymore (the shutdown
      write died in 4fa); log_named_telemetry_events had no code
      readers; the script mapping only exported an env var whose sole
      reader (local.rs) is deleted. Channel deserialization for the
      remaining keys is unchanged (plain serde struct, unknown-field
      tolerant); all remaining ChannelConfig fields keep their
      values.

      Acceptance: clippy baselines captured at HEAD FIRST in both
      configs (12 sorted short-format warning|location pairs each:
      11 needless-return in terminal/input.rs + 1
      single-element-loop in lifecycle/mod_tests.rs:272); post-edit
      re-runs are warning-IDENTICAL in both configs (empty diff, not
      even line shifts — nothing above input.rs's warnings moved).
      Check suite 0 errors: `check -p warp --lib --all-targets`
      default + simplewarp, `--no-default-features --features
      simplewarp --bin simplewarp`, `--bin warp-oss`, `--all-targets
      -p integration` (only the two pre-existing step.rs
      unused-import warnings), `check -p warp --lib --tests
      --features skip_login`, `-p warp_cli --all-targets`, plus
      `-p {warp_core,warp_features,warp_logging,warpui,warpui_core}
      --all-targets` for every member whose Cargo.toml or code lost a
      feature or flag. `./script/format` run twice: idempotent, diff
      still exactly 17 files / 5 insertions / 139 deletions. Nextest
      `-p warp --lib --no-fail-fast`: default 4,611 passed / 3
      skipped / 0 failed; simplewarp 4,610 passed / 3 skipped / 0
      failed (one informational nextest "leaky" annotation on a
      passing test) — passed/skipped counts identical to the
      post-4fd baseline. Disk healthy throughout (~29GB free at
      round start, ~45GB after; no stale-executable cleanup needed,
      no cargo clean). Runtime smoke SKIPPED per the 2026-09-23
      convention (user away, macOS password prompt unanswerable); no
      binary launched — cargo check/clippy/nextest builds only.

      TELEMETRY EFFORT COMPLETE (4ez–4fe): every remote-telemetry
      component is deleted — scope plan (4ez, plan-only), send path
      (4fa: 21 files, 1,881−), event catalog + producer sites (4fb:
      199 files, 16,435−), queue + trait machinery (4fc: 41 files,
      1,118−), events shell + privacy fields (4fd: 55 files,
      1,436−), switches + config plumbing (4fe: 17 files, 139−) —
      21,009 deleted vs 1,533 inserted across the five code rounds,
      ≈19,500 net lines gone. What remains is local-only (blocklist
      secret redaction, report_error!/warp_errors, network logging,
      privacy settings with local readers, the analytics
      banner/toggle UI, the ai_ugc content-behavior switch) plus the
      cosmetic rename candidates. DESIGNATED NEXT: item 8, the fold —
      per the 4ca plan: "ServerApi/Provider/BaseClient collapse,
      warp_server_client and firebase fall, network_logging moves,
      warp_server_auth stays as local identity, and cloud_objects +
      the warp_graphql types are the endgame." The ORCHESTRATOR will
      scope it; NOT started here.
- [ ] **the fold scope survey + slice plan (4ff) — RECORDED 2026-09-23.**
      SCOPING round for 4ca item (8) — no code deleted; this entry is
      the deliverable.

      TASK-9 ANSWER (what network identity remains after the fold):
      identity/transport code STAYS, and which binaries can reach it is
      decided by channel config + feature set, exactly the telemetry
      pattern. (a) The product bin `simplewarp` is fully braked:
      `app/src/bin/simplewarp.rs` hardcodes
      `WarpServerConfig::local_only()` (`.invalid` RFC-2606 hosts,
      `firebase_auth_api_key: ""`) and reads NO env override —
      `SERVER_ROOT_URL`/`WS_SERVER_URL` flow only through the
      `warp-channel-config` generator consumed by the dev/local/
      preview/stable bins — and its feature set is `local_only` +
      `skip_login` (+ `SkipFirebaseAnonymousUser` flag):
      `AuthState::initialize` stops before adopting any user,
      `AuthSession::get_or_refresh_access_token` bails on both
      `cfg!` gates before touching credentials, and
      `sign_in_url()` targets a `.invalid` root, so no login path can
      succeed. (b) Login against a LOCAL warp-server is a KEPT
      developer feature of the `warp` Local-channel bin
      (`script/run` picks it when `warp-channel-config` is on PATH;
      WITH_LOCAL_SERVER per AGENTS.md): the whole flow is live —
      browser redirect → `AuthRedirectPayload` intake
      (`root_view.rs:1533`) → `AuthManager::initialize_user_from_
      auth_payload` → `AuthClient::fetch_user` (Firebase token
      exchange + the GetUser GraphQL call) → override-warning modal,
      plus startup `refresh_user` (`lib.rs:1718`) and the `Reauth`
      workspace action (`view.rs:20825`). (c) `warp-oss`
      (default-run bin) uses `WarpServerConfig::production()` and
      also keeps the flows functional. (d) A second live AuthClient
      consumer outside login: `remote_server::wire_auth_token_
      rotation` + `auth_context.rs` use
      `get_or_refresh_access_token()`/`AccessTokenRefreshed` for SSH
      daemon tokens (compiled non-wasm, all bins). CONSEQUENCE: the
      4ca wording "warp_server_client falls entirely" needs one
      correction — the CRATE falls, but its auth half
      (AuthClient/AuthSession/AuthEvent/fetch_user/GetUser) MOVES
      into `warp_server_auth`, which becomes the single identity
      crate (state + credentials + session + token exchange +
      GetUser). Deleting it instead would kill the documented local
      warp-server dev flow and login in the warp/warp-oss bins. GetUser
      is therefore the last LIVE GraphQL op and stays.

      PER-CRATE MAPS (verified at f2b2e2137). ServerApi
      (`app/src/server/server_api.rs`, 445 + auth.rs 5 + auth_tests
      54): `ServerApi = { base_client: Arc<BaseClient> }` +
      `Deref<BaseClient>` + four `Err(local_only_error())` stubs
      (generate_ai_input_suggestions, get_relevant_files,
      generate_am_query_suggestions, transcribe);
      `ServerApiProvider` singleton = `{ server_api, auth_client }`
      + the AuthEvent pump (`:377-404`: NeedsReauth→AuthManager,
      UserAccountDisabled→`app:log_out` global action, re-emit).
      `get()` callers: `Arc<ServerApi>` threaded as a plumbing type
      through 9 files (root_view, workspace/view, pane_group/mod,
      terminal/view resources, terminal/input, docker_sandbox,
      mock_terminal_manager, view/testing, ai/agent/api/impl.rs —
      the last already dead: `let _ = &server_api;`); actual method
      calls are only the 4 stubs (voice_transcriber.rs:34,
      get_relevant_files/controller.rs:152,
      blocklist/passive_suggestions/legacy.rs:274,
      predict/next_command_model.rs:662) plus BaseClient's
      `get_or_refresh_access_token` via Deref
      (workspace/view.rs:20718, CopyAccessTokenToClipboard dev
      action). `get_auth_client()`: auth_manager (login),
      remote_server_controller.rs:536, tests. `get_http_client()`:
      7 sites (init_project ×2, load_ai_conversation,
      persisted_workspace ×4). AuthEvent subscribers: workspace/
      view.rs:2192 (StagingAccessBlocked), remote_server/mod.rs:57
      (AccessTokenRefreshed→rotate), mcp/templatable_manager/
      native.rs:370. BaseClient (crates/warp_server_client/src/
      base_client.rs, 107+28 tests): `{client, auth_state,
      auth_session, graphql_routing}` + the cfg `agent_mode_evals`
      EVAL_USER_IDS key-install block; accessors
      owned_http_client/auth_session/anonymous_id/user_id/
      graphql_request_options_with_token — every one consumed only
      by server_api.rs or AuthClientImpl; sole external importer is
      app/src/server/server_api.rs. → fully dissolvable.
      warp_server_client (863 src lines): auth/mod.rs (198 —
      AuthClient trait + automock, AuthClientImpl, FetchUserResult,
      UserAuthenticationError + `From<FirebaseError>`,
      EXPERIMENT_ID_HEADER), auth/session.rs (219 — AuthSession,
      AuthEvent, identitytoolkit exchange with proxy fallback),
      base_client (135), network_logging (235 w/ tests), drive.rs +
      ids.rs (1-line cloud_objects re-export shims), lib.rs (8).
      External importers: exactly 9 app files (listed as consumers
      above + cloud_object/folders.rs and
      cloud_object/model/generic_string_model.rs for the ids
      shims); Cargo dependents: app only.
      firebase (145 lines, pure serde data types: FirebaseError,
      AccountInfo/GetAccountInfoResponse, FetchAccessTokenResponse;
      no HTTP, no config, no feature): both importers are
      warp_server_client/auth; the actual network calls live in
      AuthSession::fetch_auth_tokens and are braked per the task-9
      answer; app/Cargo.toml:430 declares the dep but no app/src
      file imports firebase (already dead).
      network_logging: `NetworkLogModel` +
      `install_on_clients` (`set_before_request_fn`/
      `set_after_response_fn` → bounded async channel → model);
      deps are http_client + warpui_core + warp_errors +
      bounded_vec_deque + chrono ONLY — no auth/graphql/firebase
      coupling, so it moves cleanly to `app/src/server/`.
      Registration order matters: lib.rs:1123-1126 registers it
      BEFORE ServerApiProvider. Pane surface (network_log_view.rs +
      network_log_pane_manager.rs) is already app-side and stays;
      everything gated on `ContextFlag::NetworkLogConsole`.
      warp_server_auth (1,432 lines): anonymous_id, auth_state
      (AuthState/AuthStateProvider/PersistAction; initialize order
      test user → WARP_USER_SECRET → persisted user; `local_only`
      stops first), credentials, user (+ secure-storage
      persistence), user_uid. CORRECTION to "no dependency on the
      falling crates": it depends on warp_graphql today —
      auth_state/credentials use object_permissions::OwnerType,
      user.rs converts FROM get_user types (FirebaseProfile,
      PrincipalType, AnonymousUserPersonalObjectLimits,
      ServerTimestamp) and mutations::create_anonymous_user. So
      "stays" = keeps warp_graphql and GAINS http_client +
      async-channel + async-trait + instant when the auth half
      moves in. warp_graphql (crates/graphql; 31 importer files,
      down from 49 at 4ca): GetUser::build at
      warp_server_client/src/auth/mod.rs:70 is the ONLY op build
      site workspace-wide (4ca's "four operations" is now one; the
      api-key trio fell in 4cd); get_conversation_usage and
      create_anonymous_user are types-only; client.rs has exactly
      2 importer files, both warp_server_client; type half by
      consumer: scalars::ServerTimestamp ×10, billing ×5
      (app/pricing), workspace ×4 (app/workspaces),
      object_permissions ×4, generic_string_object ×4,
      ai::AgentTaskState ×3, object ×2, get_user ×4. cloud_objects
      (1,946 lines, 102 importer files: 74 app,
      21 cloud_object_models, 3 warp_server_client [the shims],
      2 persistence, 2 cloud_object_persistence): endgame,
      untouched by the fold except the shim repoint.

      SLICE PLAN (every intermediate state compiles;
      behavior-preserving; smallest blast radius first):
      SLICE 1 — network_logging moves to app (~7 files, ≈+20/−240,
      mostly git-mv): move network_logging.rs + tests →
      app/src/server/; `pub mod network_logging;` in server/mod.rs;
      repoint 3 imports (lib.rs:203, network_log_view.rs:13,
      server_api.rs:13); drop the module from
      warp_server_client/src/lib.rs. Risk LOW — self-contained, all
      deps are already app deps, tests are warpui_core-only.
      Acceptance: check + clippy both feature sets, nextest (3
      tests move), optional smoke: NetworkLogConsole pane still
      populates. SLICE 2 — the shims fall (~4 files, ≈−10):
      folders.rs + generic_string_model.rs repoint to
      `cloud_objects::ids::`; delete drive.rs, ids.rs, and lib.rs
      re-exports (`warp_server_client::UserUid` and
      `::server_id_traits` verified zero external users). Risk:
      none. SLICE 3 — BaseClient dissolves (~8 files, ≈−250):
      GraphqlRoutingConfig + the agent_mode_evals eval-user block
      move into `AuthClientImpl::new(client, auth_state,
      event_sender, routing)`, which builds its own AuthSession;
      graphql_request_options_with_token inlines; delete
      base_client.rs + tests; server_api.rs loses base_client +
      Deref — `ServerApi = { http_client: Arc<http_client::Client>
      }` + the 4 stubs; CopyAccessTokenToClipboard and
      auth_tests.rs:48 repoint to
      `get_auth_client().get_or_refresh_access_token()`;
      ServerApiProvider::new constructs AuthClientImpl directly.
      Risk: moderate — the evals block is cfg'd code presubmit
      never compiles (4ca skip_login class); hand-compile with
      `--features agent_mode_evals`. SLICE 4 — warp_server_client
      + firebase fall; auth half moves into warp_server_auth (~12
      files touched, 2 crates + workspace entries deleted, ≈−1,050
      net, ~630 lines move): git mv auth/{mod,session}.rs +
      session_tests → warp_server_auth/src/{auth_client,session}.rs;
      firebase/src/lib.rs → warp_server_auth/src/firebase.rs;
      warp_server_auth Cargo.toml gains http_client,
      async-channel, async-trait, instant; app repoints
      server_api.rs imports, the server_api/auth.rs re-export
      source (auth_manager/root_view import paths unchanged), the
      3 AuthEvent consumers; Cargo.toml drops firebase +
      warp_server_client members; app/Cargo.toml drops both deps +
      the dead firebase line:430 and repoints the feature
      forwardings — local_only/skip_login (:893-894), test-util
      (:906), integration_tests (:912), dev-dep (:444-445). Delete
      the automock while moving (zero MockAuthClient users) so
      mockall/test-util need not follow into warp_server_auth.
      Risk: highest of the round — feature-forwarding chains,
      wasm `async_trait(?Send)` branches move verbatim,
      integration_tests code is never compiled by presubmit
      (hand-check), ServerApiProvider::new_for_test harnesses.
      Acceptance: check + clippy in default AND simplewarp sets,
      plus `--features integration_tests` and `fast_dev`
      hand-checks, nextest both sets. SLICE 5 — optional tail, its
      own round: the four stub walls + callers together
      (established 4cf/4cg pattern — every stub is an
      unconditional Err, so taking the existing error fallback is
      behavior-identical): voice_transcriber,
      get_relevant_files/controller path,
      blocklist/passive_suggestions/legacy.rs (whole file likely
      falls), next_command_model suggestion path, the dead
      ai/agent/api/impl.rs field; then dissolve the
      `Arc<ServerApi>` plumbing across the 9 files into
      ServerApiProvider::{get_http_client, get_auth_client} — the
      provider keeps {http_client, auth_client} + event pump.
      ≈−500..800. ENDGAME (recorded, not scheduled): cloud_objects
      + the warp_graphql type half fall as their consumers fall
      (workspaces/pricing/cloud_object/drive); terminal
      warp_graphql = schema + get_user + client.rs + scalars, alive
      as warp_server_auth's identity dependency for as long as
      login remains a dev-bin feature.

      LANDMINES: (1) the four feature-forwarding chains
      app→warp_server_client→warp_server_auth (local_only,
      skip_login, integration_tests, test-util) must all repoint in
      slice 4 — `fast_dev = ["skip_login"]` and the simplewarp set
      ride them. (2) cfg code presubmit never compiles:
      agent_mode_evals (slice 3), wasm async_trait(?Send) +
      `initialize_user_from_session_cookie` (slice 4),
      integration_tests (slice 4). (3) the AuthEvent pump's
      `app:log_out` global action exists to avoid a circular model
      reference (comment at server_api.rs:381-386) — any provider
      refactor must keep the pump shape. (4) NetworkLogModel must
      stay registered BEFORE ServerApiProvider (lib.rs:1123-1126).
      (5) test fixtures carry bearer/refresh tokens
      (session_tests `Bearer("daemon-token")`, base_client_tests) —
      move with their modules; never add them to logs. (6)
      EXPERIMENT_ID_HEADER + anonymous_id decoration on GetUser
      must move with fetch_user_properties. (7) the login UI's
      server interactions (Reauth/sign_in_url, auth-redirect
      intake, override-warning modal, startup refresh_user) must
      keep working against a LOCAL warp-server via the warp
      Local-channel bin — no AuthManager path may be stubbed in
      this round. (8) `app/Cargo.toml:430` firebase is already a
      dead dep — verified zero app/src imports before removing.
      (9) `warp_server_client::auth` re-exports
      `warp_server_auth::user_uid` — the app-side re-export chain
      (`crate::auth`) must stay intact when the module moves.
      DESIGNATED NEXT: slice 1 exactly as scoped above.
      Acceptance for THIS round: no code changes — this commit
      contains only plan.md (verified with `git show --stat`); no
      clippy/nextest required; app not launched; no .rs file
      touched.

- [x] **the fold slice 1 — network_logging moves to app (4fg) —
      DONE 2026-09-23.** Executed the 4ff SLICE 1: pure code
      motion — `network_logging` now lives in the app and no
      longer routes through (or links into) warp_server_client;
      first crack in the falling crate. 7 files, 4 insertions(+),
      4 deletions(-) on top of two byte-identical git renames
      (235 lines moved); net workspace line delta 0.

      MOVE TABLE. crates/warp_server_client/src/network_logging.rs
      (164 lines: NetworkLogModel + install_on_clients +
      NetworkLogItem) → app/src/server/network_logging.rs,
      byte-identical rename; crates/warp_server_client/src/
      network_logging_tests.rs (71 lines, 3 tests) →
      app/src/server/network_logging_tests.rs, byte-identical
      (the `#[path = "network_logging_tests.rs"]` wiring moved
      unchanged — both files stay in the same directory).

      REPOINTS (the only edits). app/src/server/mod.rs:
      `pub mod network_logging;` (alphabetical, after
      network_log_view). The import
      `warp_server_client::network_logging::NetworkLogModel` →
      `crate::server::network_logging::NetworkLogModel` in 3
      files: app/src/lib.rs (was :203; new line sits in the
      crate:: group next to update_manager), server/
      network_log_view.rs (was :13), server/server_api.rs
      (was :13). crates/warp_server_client/src/lib.rs: dropped
      `pub mod network_logging;` — the crate is now exactly
      auth/ + base_client + drive + ids + two re-exports.
      No Cargo.toml touched: the module's full dep set
      (http_client, warpui_core, warp_errors, bounded_vec_deque,
      chrono, async_channel, anyhow, reqwest types) was already
      app's.

      REGISTRATION ORDER PRESERVED VERBATIM (landmine 4): the
      lib.rs comment + `add_singleton_model(NetworkLogModel::
      default())` (:1123-1126) still precede the
      `ServerApiProvider::new` closure (:1129-1130), and the hook
      install is untouched (server_api.rs:275-279: NetworkLog
      Console gate → `install_on_clients([&mut client])` inside
      `ServerApi::new`, which runs during provider construction,
      i.e. after registration).

      DEVIATIONS FROM THE 4FF MAP. (1) Zero in-file edits — 4ff
      priced ≈+20/−240 with visibility adjustments, but every item
      was already `pub` and the test wiring is path-relative, so
      the renames needed no touches at all; the round's real diff
      is +4/−4 of import rewiring. (2) The dep list was slightly
      broader than 4ff recorded (also async_channel + anyhow +
      reqwest for the item Debug formatting) — still zero
      coupling to the falling crates, so the "moves cleanly"
      conclusion held. (3) Baseline line refs at this HEAD: the
      single-element-loop clippy warning sits at
      mod_tests.rs:272 (4fe-consistent), not :277; ServerApi's
      install block is at server_api.rs:275-279, not 283-285.

      FOR SLICE 4 (noted, not done): warp_server_client's
      `bounded-vec-deque` dep is now orphaned (network_logging was
      its only user); async_channel/anyhow still have auth +
      base_client users. No network_logging re-export existed in
      warp_server_client beyond the mod decl, and no other crate
      imports it (importers were app-only; all three repointed).

      Deliberately left: slice 2 — drive.rs + ids.rs 1-line
      cloud_objects shims fall, cloud_object/folders.rs +
      cloud_object/model/generic_string_model.rs repoint to
      `cloud_objects::ids::`, lib.rs re-exports (`UserUid`,
      `server_id_traits`) die. Slice 3 — BaseClient dissolves
      into AuthClientImpl (GraphqlRoutingConfig + the cfg
      agent_mode_evals eval-user block move in; hand-compile with
      `--features agent_mode_evals`), server_api.rs loses
      base_client + Deref, CopyAccessTokenToClipboard +
      auth_tests.rs repoint to
      get_auth_client().get_or_refresh_access_token().
      Slice 4 — warp_server_client + firebase crates fall, auth
      half moves into warp_server_auth (feature-forwarding chains
      local_only/skip_login/test-util/integration_tests repoint;
      automock dies in the move), orphaned bounded-vec-deque dep
      removed. Slice 5 — the four Err-stub walls + their callers,
      then the Arc<ServerApi> plumbing dissolution into
      ServerApiProvider accessors.

      Local-only safety: the network log pane is a local
      debugging surface — before_request/after_response taps on
      the app's own http_client::Client feeding a bounded async
      channel into an in-memory singleton, gated only by the
      local ContextFlag::NetworkLogConsole. It reads nothing
      remote, persists nothing, and touches no auth path. The
      move changed module paths only: same registration order,
      same hooks on the same client instance, so pane behavior is
      identical. The kept local warp-server login flow is
      untouched — no AuthManager/ServerApiProvider construction
      or AuthClient code changed this round.

      Acceptance: clippy baselines captured at HEAD FIRST in both
      configs (12 canonical file:line + message tuples each: 11
      needless-return in terminal/input.rs + 1
      single-element-loop in lifecycle/mod_tests.rs:272);
      post-edit re-runs with `--message-format=json` primary-span
      extraction are warning-IDENTICAL in both configs (empty
      diff — same 12 warnings, same lines). Check suite 0 errors:
      `check -p warp --lib --all-targets` default + simplewarp,
      `--no-default-features --features simplewarp --bin
      simplewarp`, `--bin warp-oss`, `--all-targets -p
      integration` (only the two pre-existing step.rs
      unused-import warnings), `check -p warp --lib --tests
      --features skip_login`, `-p warp_server_client
      --all-targets` (the moved-from crate still compiles clean).
      `./script/format` no diff — still exactly 7 files / 4
      insertions / 4 deletions. Nextest `-p warp --lib
      --no-fail-fast`: default 4,614 passed / 3 skipped / 0
      failed; simplewarp 4,613 passed / 3 skipped / 0 failed —
      exactly the post-4fe baselines (4,611 / 4,610) plus the 3
      moved network_logging tests, verified individually passing
      in both configs under `warp server::network_logging::tests`.
      Disk healthy (~45GB free at round start; no stale-executable
      cleanup needed, no cargo clean). Runtime smoke SKIPPED per
      the 2026-09-23 convention (user away, macOS password prompt
      unanswerable); no binary launched — cargo check/clippy/
      nextest builds only.

      DESIGNATED NEXT: fold slice 2 — the shims fall (~4 files,
      ≈−10): folders.rs + generic_string_model.rs repoint to
      `cloud_objects::ids::`; delete drive.rs, ids.rs, and the
      lib.rs re-exports. Then slice 3 — BaseClient dissolves into
      AuthClientImpl + client-only ServerApi (~8 files, ≈−250).

- [x] **the fold slices 2+3 — shims fall, BaseClient dissolves (4fh)
      — DONE 2026-09-23.** Executed the 4ff SLICES 2 + 3 combined
      (small-cluster precedent): the ids/drive re-export shims are
      gone and BaseClient is dissolved into AuthClientImpl + a
      client-only ServerApi. 11 files, 113 insertions(+), 229
      deletions(-) — net −116; four files deleted outright
      (drive.rs, ids.rs, base_client.rs, base_client_tests.rs);
      warp_server_client/src/lib.rs is now the single line
      `pub mod auth;`.

      SHIM TABLE (item → real home → importer repoint). All four
      shims verified as pure `pub use cloud_objects::…::*`
      one-liners before deletion:
      * `warp_server_client::ids::FolderId` →
        `cloud_objects::ids::FolderId` (pub struct,
        crates/cloud_objects/src/ids.rs:398) →
        app/src/cloud_object/folders.rs re-export repointed; the
        stale `// Re-exported from warp_server_client.` comment
        deleted with it.
      * `warp_server_client::ids::GenericStringObjectId` →
        `cloud_objects::ids::GenericStringObjectId`
        (crates/cloud_objects/src/ids.rs:409) →
        app/src/cloud_object/model/generic_string_model.rs
        re-export repointed.
      * `warp_server_client::drive::*` → `cloud_objects::drive::*`
        → zero importers workspace-wide; module deleted with no
        repoint needed.
      * `warp_server_client::UserUid` (lib re-export) and
        `warp_server_client::server_id_traits` (lib re-export) →
        verified ZERO external users (`git grep` on the
        call-syntax forms: every UserUid user goes through
        `crate::auth` or `cloud_objects`/`warp_server_auth` direct;
        every server_id_traits! invocation is
        `cloud_objects::server_id_traits!`); both re-export lines
        deleted. `auth/mod.rs`'s own
        `pub use warp_server_auth::user_uid;` chain (4ff landmine
        9) untouched.

      BASECLIENT DISSOLUTION EVIDENCE (where each piece landed).
      * Transport + session: `AuthClientImpl`
        (crates/warp_server_client/src/auth/mod.rs) now owns
        {client: Arc<http_client::Client>, auth_state:
        Arc<AuthState>, auth_session: Arc<AuthSession>,
        graphql_routing: GraphqlRoutingConfig};
        `AuthClientImpl::new(client, auth_state, event_sender,
        graphql_routing)` builds its own AuthSession exactly as
        BaseClient::new did.
      * GraphqlRoutingConfig moved to auth/mod.rs (pub, Default)
        and is imported by the app as
        `warp_server_client::auth::GraphqlRoutingConfig`.
      * The cfg `agent_mode_evals` EVAL_USER_IDS eval-user block
        moved VERBATIM into AuthClientImpl::new — including the
        `wk-1.{eval_user_id:0>64x}` Credentials::ApiKey install
        (base_client's cfg-gated `use …Credentials` died:
        auth/mod.rs already imports Credentials unconditionally).
        The rand dep was already
        `agent_mode_evals = ["dep:rand"]` on
        warp_server_client — no Cargo.toml change.
      * `graphql_request_options_with_token` inlined into its sole
        caller fetch_user_properties as a RequestOptions literal;
        the EXPERIMENT_ID_HEADER + anonymous_id decoration
        survives (landmine 6, now via
        self.auth_state.anonymous_id()).
      * ServerApi = { http_client: Arc<http_client::Client> } +
        the four `Err(local_only_error())` stubs (untouched —
        slice 5); the Deref impl is gone;
        ServerApi::new(ctx) still builds the client and installs
        the NetworkLogConsole hooks, and ServerApi::http_client()
        hands that SAME Arc to both get_http_client() (7 sites)
        and AuthClientImpl (so the network-log taps still see
        GraphQL traffic) — registration order preserved
        (ServerApi::new runs inside provider construction, after
        NetworkLogModel registration; landmine 4 intact).
      * ServerApiProvider::new constructs AuthClientImpl directly
        with server_api.http_client(); the AuthEvent pump body is
        byte-identical (NeedsReauth → AuthManager, UserAccountDisabled
        → `app:log_out` global action, re-emit) — the circular
        reference workaround is not broken (landmine 3);
        ServerApiProvider::new_for_test now creates the test
        AuthState itself (ServerApi::new_for_test no longer can).
      * get_or_refresh_access_token is reachable only through the
        AuthClient trait: CopyAccessTokenToClipboard
        (workspace/view.rs:20714) repointed from
        `self.server_api.get_or_refresh_access_token()` (Deref)
        to `ServerApiProvider::as_ref(ctx)
        .get_auth_client().get_or_refresh_access_token()`;
        remote_server/auth_context.rs already used the trait and
        is untouched. get_auth_client() (2 sites) and
        get_http_client() (7 sites) signatures unchanged.

      AGENT_MODE_EVALS LANDMINE HANDLING. Feature spelling
      confirmed from app/Cargo.toml:809-816:
      `agent_mode_evals = ["integration_tests", …,
      "warp_server_client/agent_mode_evals"]` (which enables
      warp_server_client's `dep:rand`). Baselined BEFORE editing:
      `cargo check -p warp --lib --features agent_mode_evals` at
      HEAD = exit 0 with 5 warnings (3 pre-existing dead-code:
      profiles.rs:2120, request_usage_model.rs:73, lib.rs:299;
      plus the 2 step.rs unused-imports), saved via
      primary-span JSON extraction. Post-edit rerun: 0 errors,
      warning list byte-identical (empty diff) — the moved block
      compiles under its feature.

      DEVIATIONS FROM THE 4FF MAP. (1) auth_tests.rs:48 repointed
      to a DIRECT AuthClientImpl construction (bearer token set on
      AuthState::new_logged_out_for_test, plain
      http_client::Client::new()) rather than through
      get_auth_client(): the test had built ServerApi — not the
      provider — so there was no provider to ask; the exercised
      path (AuthSession's skip_login bail) is identical, and
      ServerApi::new_for_test_with_bearer_token is deleted.
      (2) 4ff's "GraphqlRoutingConfig … move into
      AuthClientImpl::new(… routing)" is true for the TYPE and the
      evals block, but the routing VALUE construction (the
      agent_mode_evals path_prefix cfg pair) moved to
      ServerApiProvider::new/new_for_test — the only caller left
      holding the feature context, matching 4ff's own "provider
      constructs AuthClientImpl directly". (3) The wasm-only
      `#[cfg_attr(target_family = "wasm",
      allow(unused_variables))]` on ServerApiProvider::new was
      dropped: auth_state is now consumed unconditionally, so the
      allow is dead on every target (wasm not compiled here; the
      async_trait(?Send) branches moved nowhere and stay verbatim).
      (4) Deleted size came in at net −116, not 4ff's ≈−260: ~70
      of base_client's lines MOVED (evals block + routing config +
      constructor) rather than vanished, and slice 2 was −9 not
      −10. (5) Nextest counts are UNCHANGED from baseline —
      base_client_tests.rs lived in the warp_server_client
      package, never in `-p warp --lib`, so its 1 test was never
      in the 4,614/4,613 counts.

      Deliberately left (SLICE 4 — designated next): warp_server_client's
      remaining auth half (auth/mod.rs + auth/session.rs +
      session_tests) and the firebase crate's serde types move
      into warp_server_auth, the two crates merge, the workspace
      member + app dep lines and the four feature-forwarding
      chains (local_only, skip_login, test-util, integration_tests)
      repoint, app/Cargo.toml's already-dead firebase line drops
      (landmine 8), the orphaned bounded-vec-deque dep is removed,
      and the zero-user automock dies in the move; hand-compile
      `--features integration_tests` + wasm-gated
      `initialize_user_from_session_cookie` (landmine 2). SLICE 5
      after that: the four `Err(local_only_error())` stub walls +
      their callers, then the `Arc<ServerApi>` plumbing
      dissolution across the 9 files into ServerApiProvider
      accessors.

      Local-only safety: zero behavior change for every locally
      runnable feature. The locally-compiled code paths are the
      same objects wired the same way: one http_client::Client
      Arc carrying the same optional network-log hooks, one
      AuthSession over the same AuthState, one AuthEvent channel
      of the same bounded capacity feeding the unchanged pump, and
      the same four stub methods returning the same error. The
      kept local warp-server login flow is intact end to end:
      fetch_user (Firebase exchange → GetUser with
      EXPERIMENT_ID_HEADER + anonymous_id), refresh_user, the
      Reauth action, NeedsReauth → set_needs_reauth, and
      remote_server token rotation via AccessTokenRefreshed are
      all byte-for-byte the same logic; no AuthManager path is
      stubbed. skip_login/local_only still bail in AuthSession
      before any network call, and the eval-user block compiles
      only under agent_mode_evals with identical values.

      Acceptance: clippy baselines captured at HEAD FIRST in both
      configs (12 primary-span file:line + message tuples each:
      11 needless-return in terminal/input.rs + 1
      single-element-loop in lifecycle/mod_tests.rs:272);
      post-edit re-runs are warning-IDENTICAL in both configs
      (empty diffs). Check suite 0 errors in all eight
      configurations: `check -p warp --lib --all-targets` default
      + simplewarp, `--no-default-features --features simplewarp
      --bin simplewarp`, `--bin warp-oss`, `--all-targets -p
      integration` (only the two pre-existing step.rs
      unused-import warnings), `check -p warp --lib --tests
      --features skip_login`, `check -p warp_server_client
      --all-targets`, and `check -p warp --lib --features
      agent_mode_evals` (warnings identical to its HEAD baseline).
      `./script/format` exit 0, no diff beyond the round's own
      edits. Nextest `-p warp --lib --no-fail-fast`: default 4,614
      run / 4,613 passed / 3 skipped / 0 failed; simplewarp 4,613
      run / 4,613 passed / 3 skipped / 0 failed — both identical
      to the post-4fg baseline (one load-flake timeout on
      test_char_cell_diff_pipeline_populates_ghosts_and_hidden_ranges
      in the default run passed in 0.08s on individual rerun,
      same class as the known
      test_command_block_dispatches_event flake). Also `nextest
      -p warp_server_client`: 3/3 session tests pass. Disk healthy
      (44GB free at round start, 41GB after; no stale-executable
      cleanup needed, no cargo clean). Runtime smoke SKIPPED per
      the 2026-09-23 convention (user away, macOS password prompt
      unanswerable); no binary launched — cargo
      check/clippy/nextest builds only.

      DESIGNATED NEXT: fold slice 4 — warp_server_client + firebase
      fall, the auth half moves into warp_server_auth, both crates
      merge (feature-forwarding chains repoint; bounded-vec-deque
      and the dead firebase dep drop). Then slice 5 — the stub
      walls and the Arc<ServerApi> plumbing dissolution.

- [x] **the fold slice 4 — warp_server_client + firebase fall (4fi)
      — DONE 2026-09-23.** Executed the 4ff SLICE 4: the
      warp_server_client and firebase crates are deleted and their
      auth half lives in warp_server_auth, now the single identity
      crate. 19 files, 42 insertions(+), 168 deletions(−) — net
      −126 — plus 677 lines moved in four git renames (532
      warp_server_client + 145 firebase); two crate directories
      removed from the workspace.

      MOVE TABLE (module → new home). All four renames moved the
      bodies verbatim; only import paths were rewritten:
      * crates/warp_server_client/src/auth/mod.rs (246 lines) →
        crates/warp_server_auth/src/auth_client.rs — AuthClient
        trait (+ wasm `async_trait(?Send)` attrs verbatim),
        AuthClientImpl {client, auth_state, auth_session,
        graphql_routing}, FetchUserResult,
        UserAuthenticationError + From<FirebaseError> +
        register_error!, EXPERIMENT_ID_HEADER, GraphqlRoutingConfig,
        the cfg agent_mode_evals EVAL_USER_IDS block (moved
        byte-identical).
      * crates/warp_server_client/src/auth/session.rs (219) →
        crates/warp_server_auth/src/session.rs — AuthSession,
        AuthEvent (+ the redacting Debug and the wasm dead_code
        allow on the token field), get_or_refresh_access_token
        (skip_login/local_only cfg! bails verbatim),
        exchange_credentials, fetch_auth_tokens (identitytoolkit
        POST + proxy fallback), fetch_access_token_via_proxy.
      * crates/warp_server_client/src/auth/session_tests.rs (66) →
        crates/warp_server_auth/src/session_tests.rs — 3 tests with
        their bearer/refresh token fixtures intact (landmine 5).
      * crates/firebase/src/lib.rs (145) →
        crates/warp_server_auth/src/firebase.rs — pure serde types
        (FirebaseError, AccountInfo, GetAccountInfoResponse[Payload],
        FetchAccessTokenResponse), byte-identical; both its
        importers were the moved auth files.
      * warp_server_auth/src/lib.rs: `pub mod {auth_client,
        firebase, session};` added to the alphabetical module list.
      * warp_server_auth/Cargo.toml: gains http_client,
        async-channel, async-trait, instant, optional rand, feature
        `agent_mode_evals = ["dep:rand"]`, dev-dep futures (the
        moved tests' block_on). Existing role/deps unchanged —
        warp_graphql stays (GetUser is the last live op).

      CRATE DELETIONS + MANIFESTS. Deleted outright:
      crates/warp_server_client/Cargo.toml (63) + src/lib.rs (1),
      crates/firebase/Cargo.toml (12). Root Cargo.toml: both
      [workspace.dependencies] entries dropped (members list is a
      `crates/*` glob — no member edit). app/Cargo.toml: main-dep
      line (:246), already-dead dev-dep `firebase.workspace = true`
      (:430, landmine 8 — verified zero app/src imports), and
      dev-dep warp_server_client (:444) dropped. Cargo.lock
      refreshed (both packages and their now-unreachable dep edges
      pruned, −51 lines). warp_server_client's orphaned
      `bounded-vec-deque` dep went with the crate (network_logging
      was its last user, moved to app in 4fg).

      REPOINTS (all six app importers at HEAD; 4ff's nine included
      the three consumed by 4fh's slice 2/3). `AuthEvent` (3 files:
      ai/mcp/templatable_manager/native.rs, remote_server/mod.rs,
      workspace/view.rs) → `warp_server_auth::session::AuthEvent`;
      server_api.rs → `warp_server_auth::auth_client::
      {AuthClientImpl, GraphqlRoutingConfig}` + the session AuthEvent;
      server_api/auth.rs's re-export source →
      `warp_server_auth::auth_client::{AuthClient, FetchUserResult,
      UserAuthenticationError}` (the app-side `crate::auth` chain —
      4ff landmine 9 — stays intact for auth_manager/root_view);
      server_api/auth_tests.rs:45 → auth_client paths. The
      ServerApiProvider construction sites (4fh deviation 2) needed
      no edits beyond that import line.

      FEATURE-FORWARDING DISPOSITIONS (4ff landmine 1). The readers
      moved with the code, so the chains now end at
      warp_server_auth directly: `local_only` and `skip_login` →
      `warp_server_auth/{local_only,skip_login}` (the cfg! bails and
      the Credentials::Test arm live in moved session.rs);
      `integration_tests` → `warp_server_auth/integration_tests`
      (auth_state.rs readers predate the move); `test-util` list
      entry `warp_server_client/test-util` → `cloud_objects/test-util`
      — that was the old feature's only real cargo (its mockall half
      existed solely for the automock, which dies this round, so
      mockall does not follow into warp_server_auth; the
      `warp_server_auth/test-util` forwarding is untouched —
      auth_state/credentials read it); `agent_mode_evals` →
      `warp_server_auth/agent_mode_evals` (new feature owning the
      moved EVAL_USER_IDS block; `dep:rand`). The integration crate
      is untouched — its `warp = { features = ["integration_tests"] }`
      rides the repointed app chain.

      AUTOMOCK. Deleted in the move (4ff instruction): `git grep
      MockAuthClient` = zero users at HEAD; the trait's
      cfg_attr(automock) + the mockall imports are gone.

      DEVIATIONS FROM THE 4FF MAP. (1) Importer count is 6, not 9
      (above). (2) session.rs's `Credentials::Test` match arm gained
      `test` in its cfg (`any(test, feature = "integration_tests",
      feature = "skip_login")`, now exactly matching credentials.rs's
      gating): inside the crate, cfg(test) compiles the variant into
      warp_server_auth's own unit-test builds, so the match would
      not be exhaustive without it; as an external dependency
      warp_server_auth was never compiled with cfg(test), so no
      product config is affected — only this crate's own nextest
      target. (3) auth/mod.rs's two user_uid re-export lines
      (`pub use user_uid::{TEST_USER_EMAIL, TEST_USER_UID, UserUid}`
      and `pub use warp_server_auth::user_uid;`) were dropped rather
      than moved: zero importers reach them through
      warp_server_client (all users go via app `crate::auth::user`,
      `cloud_objects::auth`, or warp_server_auth directly), and
      inside warp_server_auth they would be self-re-exports. The
      `pub use session::*` glob became direct
      `crate::session::{AuthEvent, AuthSession}` imports; app
      consumers import the two modules directly. (4) .github/
      STAKEHOLDERS: `/crates/warp_server_client/ @ianhodge`
      repointed to `/crates/warp_server_auth/` — the repo had zero
      stale ownership paths; letting this one dangle would have
      created the first. (5) Deleted size came in at net −126, not
      4ff's ≈−1,050: 4fh had already consumed ~330 of the crate's
      src lines, and the bulk of slice 4's volume (677 lines) MOVES
      as renames; what actually fell is the crate shells (76: both
      manifests + warp_server_client's 1-line lib.rs), lockfile
      pruning (−51), and the forwarding/import
      churn. (6) The wasm-gated branches (async_trait(?Send),
      initialize_user_from_session_cookie — landmine 2) moved
      verbatim; wasm is not compiled on this host, same as every
      prior round.

      Deliberately left (SLICE 5 — designated next): the four
      `Err(local_only_error())` stub walls + their callers
      (voice_transcriber, get_relevant_files/controller,
      blocklist/passive_suggestions/legacy.rs, next_command_model
      suggestion path, the dead ai/agent/api/impl.rs field), then
      the `Arc<ServerApi>` plumbing dissolution across the 9 files
      into ServerApiProvider::{get_http_client, get_auth_client}
      (+ event pump). ENDGAME after that (per 4ff, recorded, not
      scheduled): cloud_objects + the warp_graphql type half fall
      as their consumers fall; terminal warp_graphql = schema +
      get_user + client.rs + scalars, alive as warp_server_auth's
      identity dependency. Also left: the
      logging-and-error-reporting SKILL.md's example path
      (crates/warp_server_client/src/auth/mod.rs) — lockfile-managed
      common skill, orchestrator call, same disposition as 4fe left
      add-telemetry.

      Local-only safety: zero behavior change for every locally
      runnable feature — the moved bodies are byte-identical, so
      each login-path piece is verified by construction plus
      compile/test: fetch_user (AuthSession::exchange_credentials →
      identitytoolkit POST with proxy fallback → GetUser with
      EXPERIMENT_ID_HEADER + anonymous_id + routing path_prefix),
      startup refresh_user (get_or_refresh_access_token's 5-minute
      lookahead branch: NeedsReauth on DeniedAccessToken,
      AccessTokenRefreshed + update_firebase_tokens on success),
      the Reauth workspace action and AuthEvent pump
      (NeedsReauth → AuthManager, UserAccountDisabled → `app:log_out`
      — server_api.rs bodies untouched, only its import line), and
      remote_server wire_auth_token_rotation (its consumer file
      changed only the import) all compile unchanged in every
      checked config; the skip_login/local_only bails still fire
      before any network call (skip_login's reject test re-run
      green in the warp suite); AuthManager, sign_in_url, the
      auth-redirect intake and override-warning modal were not
      modified at all. Token-bearing fixtures moved with their
      tests; AuthEvent's Debug keeps redacting the token.

      Acceptance: clippy baselines captured at HEAD FIRST in both
      configs (12 primary-span message|location pairs each: 11
      needless-return in terminal/input.rs + 1 single-element-loop
      in lifecycle/mod_tests.rs:272), plus a 16-pair
      agent_mode_evals baseline (11 needless-return — that config
      runs without --all-targets, so the test-only loop warning is
      absent — + 3 pre-existing dead-code + 2 step.rs
      unused-imports); post-edit re-runs are
      warning-IDENTICAL in all three (empty diffs). Check suite 0
      errors in ten configurations: `check -p warp --lib
      --all-targets` default + simplewarp, `--no-default-features
      --features simplewarp --bin simplewarp`, `--bin warp-oss`,
      `--all-targets -p integration` (only the two pre-existing
      step.rs warnings), `check -p warp --lib --tests --features
      skip_login`, `check -p warp_server_auth --all-targets`, and
      the forwarding hand-checks `--features {agent_mode_evals,
      integration_tests, local_only, fast_dev}` (the latter two
      presubmit never compiles). `./script/format` exit 0, no diff
      beyond the round's own edits. Nextest `-p warp --lib
      --no-fail-fast`: default 4,614 run / 4,614 passed / 3
      skipped / 0 failed; simplewarp 4,613 run / 4,613 passed / 3
      skipped / 0 failed — both identical to the post-4fh baseline
      (no flake this round). `nextest -p warp_server_auth`: 7 run /
      7 passed / 0 skipped — the crate's 4 pre-existing tests plus
      the 3 moved session tests, now running under the identity
      crate in both default and test cfgs. Disk healthy (41GB free
      at round start; no stale-executable cleanup needed, no cargo
      clean). Runtime smoke SKIPPED per the 2026-09-23 convention
      (user away, macOS password prompt unanswerable); no binary
      launched — cargo check/clippy/nextest builds only.

      DESIGNATED NEXT: fold slice 5 tail — the four
      `Err(local_only_error())` stub walls + their callers, then the
      `Arc<ServerApi>` plumbing dissolution into ServerApiProvider
      accessors; cloud_objects/warp_graphql endgame thereafter.

- [x] **the fold slice 5 tail — AI stub walls + ServerApi plumbing
      dissolves (4fj) — DONE 2026-09-23.** Executed the 4ff SLICE 5:
      the four `Err(local_only_error())` walls fall together with
      their caller chains, `ServerApi` dissolves into
      `ServerApiProvider`, and the `Arc<ServerApi>` threading across
      the 9 files is gone. 37 files, 141 insertions(+), 1,114
      deletions(−) — net −973; 12 files deleted outright
      (server/voice_transcriber.rs, voice/{mod,transcriber}.rs,
      ai/voice/ + transcribe/api/ (4 files),
      ai/predict/generate_am_query_suggestions{,/api/} (4 files),
      ai/get_relevant_files/api.rs).

      PER-WALL CALLER-CHAIN EVIDENCE AND DISPOSITIONS. Every stub
      was an unconditional `Err` in every feature set, so each
      server feature could only ever fail locally; per the
      4cc/4cf precedents the caller's guaranteed outcome was kept
      where a consumer still needs an answer, and the whole chain
      died where nothing local could answer.
      * transcribe — ServerVoiceTranscriber (the ONLY Transcriber
        impl) → VoiceTranscriber singleton → two consumers:
        editor voice dictation (voice.rs handle_voice_session_result)
        and CLI-agent dictation (agent_input_footer
        handle_cli_voice_session_result). Both consumers already
        had a `transcriber() == None` degrade path
        (`lifecycle.fail()`), so the collapse deletes the trait +
        singleton + ServerVoiceTranscriber + both
        apply_transcribed_* callbacks + the
        VoiceInputState/cli_transcription_handle transcription
        handles, and both session handlers now send every
        VoiceSessionResult (Audio or Aborted) to the fail path that
        the None branch always produced. The genuinely local half
        is KEPT: the voice_input crate recording (mic buttons,
        lifecycle UI, VoiceInput singleton, settings, limit toasts)
        is untouched — recording still works, it simply can no
        longer produce text because nothing could ever transcribe
        it. Downstream: TranscribeError, crate::ai::voice
        (TranscribeRequest/Response/Provider), the editor re-export
        `Transcriber, VoiceTranscriber`, and the un-gated
        lib.rs registration all fall.
      * get_relevant_files — controller.rs send_local_request
        Complete-outline arm: <2 files → local WholeFile Success
        (kept verbatim); >=2 files → server ranking → always
        `Err(local_only_error())` → report_error + Error event. The
        agent-facing guaranteed outcome was the Error event, so the
        else-branch now emits `Error { action_id }` synchronously;
        the report_error of the stub error, the spawned future,
        handle_relevant_file_paths_result, the outline_request
        building, and get_relevant_files/api.rs types all die. The
        `>= 2` const is renamed
        MINIMUM_FILE_COUNT_FOR_API_CALL →
        WHOLE_REPO_SUGGESTION_FILE_LIMIT. pending_requests/cancel
        machinery stays (search_codebase.rs calls
        cancel_request_for_action). The remote (SSH) half is
        untouched.
      * generate_am_query_suggestions — passive_suggestions/legacy
        generate_prompt_suggestions: static-suggestion half (local,
        kept) vs server fetch (dies). The `warp_account_available()`
        early-return, request building, spawned fetch, error arm
        (report_error + `AgentModePromptSuggestion::Error`),
        build_prompt_suggestions_request, map_prompt_suggestions_
        response, and the never-again-set
        prompt_suggestions_future_handle all die;
        4ff's "whole file likely falls" was WRONG — the unit-test
        suggestion (git diff + BYOK controller) and passive code
        diff (BYOK) halves are live local behavior and stay. The
        module ai/predict/generate_am_query_suggestions{,/api/} had
        no other consumers and falls. Cascade: the stub error was
        the ONLY producer of `AgentModePromptSuggestion::Error`,
        and map_prompt_suggestions_response the only producer of
        `::None`, so the enum collapses to its inner
        PromptSuggestion (terminal/view.rs definition, the
        execute-plan construction site, on_legacy_prompt_suggestion_
        generated match → direct body, view_tests call site).
      * generate_ai_input_suggestions — next_command_model
        generate_ai_input_suggestions_if_available (with its
        warp_account_available guard) dies; the zero-state and
        prefix-fallback call sites return
        GenerateAIInputSuggestionsResponseV2::default() (exactly
        what the guard produced in local_only builds), the always-Ok
        Result in the 7-tuple is flattened, and the Err arm's
        log::error dies. NextCommandModel loses server_api;
        Input::new and TerminalViewResources lose the param. The
        history-based suggestion logic (the genuinely local half)
        is untouched.

      SERVERAPI DISSOLUTION SHAPE. Option A from the task: the
      struct VANISHES with its members moving into
      ServerApiProvider = { http_client: Arc<http_client::Client>,
      auth_client: Arc<dyn AuthClient> }. ServerApi::new's
      NetworkLogConsole hook install now runs at the top of
      ServerApiProvider::new (still inside provider construction,
      after NetworkLogModel registration — landmine 4 intact); the
      same Arc feeds AuthClientImpl and get_http_client() (7 sites,
      signatures unchanged: persisted_workspace ×4, init_project
      ×2, load_ai_conversation ×1), so the network-log taps still
      see GraphQL traffic. `get()` is deleted; get_auth_client() (3
      sites: auth_manager, remote_server_controller, lib.rs) and
      new_for_test are reshaped with identical wiring. The
      Arc<ServerApi> plumbing dies in root_view (field),
      workspace/view (field + 3 PaneGroup ctor args),
      pane_group/mod (PaneGroup field, TerminalViewResources field,
      4 ctor params, new_internal, 3 resources clones),
      terminal/view (Input::new arg), terminal/input (Input::new
      param + NextCommandModel::new), mock_terminal_manager,
      docker_sandbox, testing.rs, response_stream
      (generate_multi_agent_output loses its dead `server_api`
      param — it has run on local_inference since the BYOK move,
      `let _ = &server_api;` included), and the dead
      ai/agent/api/impl.rs field. AuthEvent subscribers are
      attached IDENTICALLY (landmine 3): the pump body is
      byte-identical and the three subscribers
      (workspace/view observe_server_api, remote_server/mod,
      mcp templatable_manager/native) are untouched — two of the
      three files have no diff at all.

      ORPHANS CHECKED AND DELETED: local_only_error +
      LOCAL_ONLY_MESSAGE (last producers were the stubs),
      TranscribeError (last consumers were the deleted voice
      apply-paths), AIApiError::NoContextFound (verified ZERO
      producers already at HEAD — only match arms; deleted with its
      agent/mod.rs arm). Checked and KEPT: all other AIApiError
      variants (live via local inference/response_stream/agent
      rendering), DeserializationError, the
      GenerateAIInputSuggestions types (they type the local
      history-path state), HistoryContext/NextCommandContext.

      Deliberately left: the CLOUD_OBJECTS/WARP_GRAPHQL ENDGAME —
      orchestrator scopes the survey next; NOT started here (cloud
      objects ~1,900 lines + the graphql type half fall only as
      their consumers fall; terminal warp_graphql = schema +
      get_user + client.rs + scalars stays alive as
      warp_server_auth's identity dependency). Also left: the
      next-command LOCAL path's request assembly
      (create_generate_ai_input_suggestions_request +
      get_context_messages + NextCommandContext.context_messages/
      ai_execution_context) — the assembled request is stored in
      NextCommandSuggestionState/ZeroStateSuggestionInfo but never
      read (its consumer was the server body); removing it cascades
      into WarpAiExecutionContext plumbing through input.rs and is
      local-path churn, not a stub wall — follow-up candidate. The
      AuthEvent pump's comments still mention `ServerApi` (logic
      unchanged, kept byte-identical). The logging-and-ERROR-
      reporting SKILL.md path (left in 4fi, orchestrator call).

      Local-only safety: zero behavior change for any locally
      runnable feature. Every collapsed caller now produces exactly
      what its stub-fed code path always produced: voice sessions
      end in the lifecycle fail path (the toast the stub error
      raised died with the stub), large-repo search returns the
      Error event the agent already always received, prompt
      suggestions are static-only (server suggestions never
      succeeded), next-command falls back to the empty response the
      local_only guard already returned. Login flow untouched:
      AuthClientImpl/AuthSession/fetch_user/refresh_user/Reauth/
      auth-redirect intake all byte-identical; AuthEvent pump and
      all three subscribers unchanged; network-log pane machinery
      (registration order, hook install on the one shared client)
      unchanged; BYOK agent paths (local_inference, blocklist
      controller streams) only lost a dead parameter.

      Acceptance: clippy baselines captured at HEAD FIRST in both
      configs (12 primary-span message|location pairs each: 11
      needless-return in terminal/input.rs + 1 single-element-loop
      in lifecycle/mod_tests.rs:272); post-edit re-runs are
      warning-IDENTICAL in both configs except the 11 input.rs
      needless-return warnings, which sit 3 lines higher (10642 →
      10639 … 10717 → 10714) because this round deleted 3 lines of
      server_api plumbing INSIDE input.rs above them — same 12
      warnings, same messages, zero new or removed. Check suite 0
      errors: `check -p warp --lib --all-targets` default + 
      simplewarp (0 warnings at all), `--no-default-features
      --features simplewarp --bin simplewarp`, `--bin warp-oss`,
      `--all-targets -p integration` (only the two pre-existing
      step.rs unused-import warnings), `check -p warp --lib --tests
      --features skip_login`, `check -p warp_server_auth
      --all-targets`, and the forwarding hand-checks: `--features
      agent_mode_evals` (0 errors, 5 warnings = its 4fi baseline:
      3 dead-code + 2 step.rs, line-shifted) and `--features
      voice_input` for the de-gated voice edits — that gate was
      ALREADY broken at HEAD (2 byte-identical pre-existing errors
      in agent_input_footer's subscribe closure: undeclared
      CLIAgentSessionsModelEvent + missing `me`, both present
      verbatim in the HEAD file); this round's voice code compiles
      clean under it and removes two of its pre-existing unused-
      variable warnings. `./script/format` run twice: idempotent,
      no diff beyond the round's own edits. Nextest `-p warp --lib
      --no-fail-fast`: default 4,614 run / 4,614 passed / 3 skipped
      / 0 failed; simplewarp 4,613 run / 4,613 passed / 3 skipped /
      0 failed — EXACTLY the post-4fi baseline, no deltas, no
      flake. Disk healthy throughout (47GB free at round start and
      after; no stale-executable cleanup, no cargo clean). Runtime
      smoke SKIPPED per the 2026-09-23 convention (user away, macOS
      password prompt unanswerable); no binary launched — cargo
      check/clippy/nextest builds only.

      FOLD COMPLETION VERDICT (4ca item 8). DONE: network_logging
      moved into the app (4fg); the ids/drive shims fell and
      BaseClient dissolved into AuthClientImpl + a client-only
      ServerApi (4fh); warp_server_client + firebase fell and the
      auth half became warp_server_auth, the single identity crate
      (4fi); and now the four AI stub walls + their caller chains
      are deleted and ServerApi is dissolved — ServerApiProvider IS
      the network surface: {get_http_client, get_auth_client} + the
      AuthEvent pump. warp_server_auth stays as local identity;
      the local warp-server login flow is intact. REMAINING of
      item 8: the ENDGAME only — cloud_objects (≈1,900 lines, ~100
      importer files) and the warp_graphql type half fall as their
      consumers fall (workspaces/pricing/cloud_object/drive);
      terminal warp_graphql = schema + get_user + client.rs +
      scalars stays alive for as long as login remains a dev-bin
      feature. DESIGNATED NEXT: the endgame survey (plan-only,
      4ff-style scope record of cloud_objects + warp_graphql type
      consumers) — do NOT start deleting cloud_objects/warp_graphql
      in the survey round.

- [x] **the endgame scope survey + slice plan (4fk) — RECORDED
      2026-09-23; all six planned slices executed and recorded
      4fl–4fq.**
      SCOPING round for 4ca item (8) endgame — no code deleted; this
      entry is the deliverable. THE DECISION (orchestrator's, now
      grounded): cloud_objects is NO LONGER a wire crate — after
      4eg–4ey it is the LOCAL cloud-object model layer, and the
      endgame deletes only what is still wire/remote-SHAPED (server
      intake paths, GraphQL conversion walls, dead sync-queue event
      verticals, type-only schema fragments) while the local model
      layer stays. There is no live server sync left anywhere: the
      only GraphQL op built workspace-wide is GetUser
      (`crates/warp_server_auth/src/auth_client.rs:110`, moved in
      4fi); `ServerApiProvider` is `{get_http_client, get_auth_client}`
      + the AuthEvent pump; nothing else can reach a server.

      WIRE-VS-LOCAL CLASSIFICATION (verified at HEAD a9f688b64).
      cloud_objects (1,946 lines, 87 importer files — down from 102 at
      4ff): LOCAL-STAYS — `ids.rs` (416: SyncId/ServerId/ClientId/
      FolderId/GenericStringObjectId, sqlite hashes, HashableId,
      ToServerId, serde; `app/src/server/ids.rs` is a pure re-export
      shim of it); `cloud_object/mod.rs` local types (ObjectIdType,
      ObjectType + FromStr/Display/sqlite prefixes,
      GenericStringObjectFormat/JsonObjectType (live: env_vars,
      facts, profiles, mcp, blocklist), Revision, Owner,
      ServerObjectContainer (typed in local model fields),
      CloudObjectSyncStatus/NumInFlightRequests + CloudObjectStatuses
      `render_icon` (LIVE drive UI, `drive/items/*.rs`),
      CloudObjectMetadata/-Permissions/-Statuses, CloudLinkSharing/
      CloudObjectGuest, SerializedModel, CloudObjectEventEntrypoint,
      RevisionAndLastEditor (sqlite read path)); `generic_cloud_object.rs`
      (181: GenericCloudObject + CloudObjectUpsertParams — the
      `upsert_event`/`bulk_upsert_event` constructor machinery is LIVE
      per the retired-4ei trace: folders.rs:36, notebooks/mod.rs:66,
      workflows/mod.rs:216, generic_string_model.rs:175 impls;
      update_manager.rs:143,668,694 + persistence.rs:906,1749 senders;
      terminal_pane.rs:1570 `maybe_build_ai_query_upsert_event`);
      `generic_string_model.rs` (41, local Serializer/GenericStringModel);
      `drive/mod.rs` (137, CloudObjectTypeAndId — 12 importers);
      `drive/sharing.rs` (49, `can_move_drive` live via
      app/src/drive/index.rs:2304, `is_user` live via
      cloud_object/mod.rs:483 + model/view.rs:180); `auth/mod.rs`
      (1-line warp_server_auth UserUid re-export — local identity).
      WIRE-FALLS (zero live production feeders, callers are tests
      only): `server_object.rs` GenericServerObject (79; ConflictStatus
      in that file STAYS — runtime state, live reader
      notebooks/active_notebook_data.rs:271); the whole GraphQL
      conversion wall in `cloud_object/mod.rs:786-1017` +
      ObjectType↔gql conversions :228-262 (only consumer is
      cloud_object_models/server_cloud_object.rs's TryFromGql bridge,
      whose entry `TryFrom<warp_graphql::object::CloudObject>` has
      ZERO callers repo-wide); ServerMetadata/ServerPermissions/
      ServerGuestSubject/ServerObjectGuest/ServerLinkSharing
      (guest/sharing trio: zero users outside the wall);
      `creation.rs` ServerCreationInfo; new_from_server/
      update_from_server_object on GenericCloudObject/CloudObjectMetadata/
      CloudObjectPermissions. ToServerId/to_server_id traced per
      anchor: ALL uses are LOCAL (notebooks/editor/embedded_item.rs:211
      and embedding_model.rs:186 uid lookups,
      cloud_object_persistence/objects.rs:528, generic bounds in
      app/src/cloud_object/mod.rs + model/persistence.rs) — it is an
      "extract inner ServerId" accessor on typed ids, misnamed but
      load-bearing; STAYS. UpdateManager
      (app/src/server/cloud_objects/update_manager.rs, 1,051): LOCAL
      — writes SQLite + in-memory CloudModel only, no server calls at
      all despite the directory name; live callers: cloud_preferences_
      syncer, toast_message, terminal input/view record_object_action,
      integration_testing QA, drive/import queue (local file import →
      create_folder/create_notebook). STAYS ENTIRELY. The sync-queue
      EVENT vertical is dead: ModelEvent::MarkObjectAsSynced,
      ::IncrementRetryCount, ::UpdateObjectAfterServerCreation,
      ::RecordTimeOfNextRefresh have ZERO senders (handlers only:
      sqlite.rs:657,668,674,718); `set_latest_revision_and_editor`,
      `check_and_maybe_clear_current_conflict`,
      `cloud_objects_force_refresh_pending` (persistence.rs:246,263,
      1761) zero callers; the refresh chain
      (record/read_time_of_next_force_object_refresh + the
      cloud_objects_refreshes table + CloudModel's
      time_of_next_force_refresh field + lib.rs:1204/1225/1574
      wiring) is orphaned end to end. The server-INTAKE half of
      CloudModel (`upsert_from_server_object` family
      persistence.rs:402-532,1662-1749, `update_object_after_server_
      creation`:139) has zero production callers — only
      model_tests.rs/profiles_tests.rs/data_source_tests.rs. Pricing:
      `PricingInfoModel::pricing_info` is `None`-initialized with NO
      setter — promotion_message() can only return None
      (ai/pricing_promotion.rs:55 is the sole live read); the three
      accessor methods already carry `#[allow(dead_code)]`. The
      workspaces gql wall: `gql_convert.rs` (1,343 + 363 tests) is
      self-referential (`gql_convert::` referenced nowhere else) and
      every From impl consumes gql types that no op can ever
      construct; the LOCAL workspace/team types it fed stay — they
      are constructed from sqlite (NewTeamMember/NewTeamSettings
      sqlite.rs:1935-2080, sqlite.rs:2730) and literals
      (user_workspaces.rs:826). Adjacent orphan (AI vertical, not
      cloud_objects scope): ServerAIConversationMetadata
      (agent/conversation.rs:4454, holds ServerMetadata +
      ServerPermissions) — `set_server_metadata` has zero production
      callers; deletable before or with slice 2b.

      warp_graphql (crates/graphql, 2,757 lines, 30 importer files):
      the LIVE terminal core is exactly what warp_server_auth's login
      flow needs — `client.rs` (220: Operation trait/send_request/
      RequestOptions/get_request_context), `api/queries/get_user.rs`
      (109, the only sent op), `api/mutations/create_anonymous_user.rs`
      (66: AnonymousUserType), `api/object_permissions.rs` OwnerType
      (auth_state.rs:9, credentials.rs:11, get_user), `api/
      experiment.rs` (78, get_user fragment), `api/request_context.rs`
      (21), `scalars/` (55: Time via ServerTimestamp ×10 importers,
      Uint32), `lib.rs`+`api/mod.rs`+schema link (30). Everything else
      is type-only or orphaned: billing.rs (438), workspace.rs (428),
      get_conversation_usage.rs (329 — types-only; op never built),
      ai.rs (246 — BUT AgentTaskState + AgentHarness are LIVE local
      vocabulary CONSTRUCTED in app code: agent_sdk/driver.rs:34,
      error_classification.rs, harness_availability.rs:137; the rest
      — AIConversationArtifact ×8, FileArtifact, AICreditAvailability*,
      PlatformErrorCode zero users, artifact fragments — is dead),
      object.rs (116), generic_string_object.rs (79 — one LIVE
      consumer: blocklist/controller/input_context.rs:283 uses the gql
      format enum's to_string() for the local DriveObjectPayload
      string), folder (18), notebook (34), workflow (10), user (17),
      error.rs (70, ZERO importers), mcp_gallery_template (27, zero),
      object_actions (41, zero), response_context (6, zero),
      ai_tests (153), billing_tests (60). cloud_object_models
      (3,720): local model types + persistence adapters STAY;
      `server_cloud_object.rs` (327, the gql→ServerCloudObject
      bridge) + the 13 `Server*` GenericServerObject type aliases
      FALL with slice 2b. cloud_object_persistence (668): LOCAL
      sqlite helpers — STAYS minus the sync remnants
      (mark_object_as_synced, increment_retry_count,
      update_object_after_server_creation, refresh.rs) that slice 1
      orphans.

      PER-CRATE TERMINAL-STATE DECISION (what the endgame records):
      (1) cloud_objects — STAYS as a crate, shrunk ≈1,946 → ≈1,350
      lines: the local cloud-object model substrate (ids, object
      types/formats, owner/metadata/permissions/statuses,
      GenericCloudObject + upsert events, GenericStringModel, drive,
      sharing). NOT deleted and NOT merged — 87 importer files across
      app/cloud_object_models/cloud_object_persistence keep it the
      shared model layer. (2) warp_graphql — STAYS as a crate,
      shrunk ≈2,757 → ≈700 lines to the login identity core (client +
      get_user + create_anonymous_user + OwnerType + experiment +
      request_context + scalars + schema link); NOT folded into
      warp_server_auth (the merge would churn 5 crates' imports to
      save zero code; the crate boundary documents "GraphQL wire
      schema for login"). GetUser remains the only op for as long as
      login remains a dev-bin feature. (3) warp_graphql_schema —
      FOLDED into crates/graphql: move `api/schema.graphql` + the
      1-line `#[cynic::schema]` module + package.json/graphql.config
      TS download tooling decision (drop it — nothing in script/ or
      .github/ invokes codegen; the SDL refresh was a manual dev
      flow against staging) → delete the crate + workspace member.
      cynic's standard single-crate layout supports schema + queries
      in one crate; graphql/build.rs already registers the SDL.
      AGENTS.md's GraphQL section (mentions codegen from
      crates/warp_graphql_schema) gets updated in that slice.

      SLICE PLAN (each state compiles; smallest blast radius first):
      SLICE 1 — the dead sync-queue event vertical + refresh chain
      (~12 files, ≈−400): ModelEvent variants MarkObjectAsSynced/
      IncrementRetryCount/UpdateObjectAfterServerCreation/
      RecordTimeOfNextRefresh + their 4 sqlite handler arms +
      persistence/mod.rs imports; cloud_object_persistence
      mark_object_as_synced/increment_retry_count/
      update_object_after_server_creation + refresh.rs (35) + lib.rs
      exports; CloudModel's update_object_after_server_creation,
      set_latest_revision_and_editor,
      check_and_maybe_clear_current_conflict,
      cloud_objects_force_refresh_pending + the
      time_of_next_force_refresh field/ctor param + lib.rs:1204/1225/
      1574 wiring + the sqlite load-struct field. Test fallout:
      model_tests.rs:329/430/481 + profiles_tests.rs:284 blocks.
      Risk LOW — every deletion is a zero-sender/zero-caller removal;
      acceptance: both feature sets check/clippy, nextest baseline,
      no sqlite schema changes (leave the table + columns).
      SLICE 2a — CloudModel server-intake functions + tests
      (~6 files, ≈−700, half of it tests): upsert_from_server_object/
      _internal/_notebook/_cloud_object, update_cloud_object_if_exists,
      bulk equivalents (persistence.rs:402-532,1662-1749); delete the
      server halves of model_tests.rs/profiles_tests.rs/
      search/ai_context_menu/rules/data_source_tests.rs (ServerAIFact)
      that only exercise them. Risk LOW-moderate — largest test
      rewrite of the round; keep the local halves of those files
      green. SLICE 2b — GenericServerObject + Server* aliases +
      Server* metadata/permissions types + the conversion wall
      (~10 files, ≈−900): crates/cloud_objects server_object.rs
      GenericServerObject (ConflictStatus stays), creation.rs,
      ServerMetadata/ServerPermissions/ServerGuestSubject/
      ServerObjectGuest/ServerLinkSharing + the :786-1017 TryFrom
      wall + :228-262 gql ObjectType conversions + From<Owner> for
      gql Owner; cloud_object_models/server_cloud_object.rs (327,
      whole file) + the 13 Server* aliases + lib.rs re-exports; app
      re-export cloud_object/mod.rs:890. BEFORE this slice: decide
      ServerAIConversationMetadata (delete it here or in its own
      micro-slice — it is the last ServerMetadata/ServerPermissions
      consumer in app). Risk moderate — pub types give no dead-code
      warnings, so the callgraph is the only evidence; hand-check
      `--features test-util` compiles (mock_current_user/
      mock_personal constructors ride the deleted types).
      SLICE 3a — the app conversion walls + pricing (~8 files,
      ≈−2,000): gql_convert.rs + gql_convert_tests.rs (1,706),
      pricing/mod.rs + pricing_tests.rs + ai/pricing_promotion.rs +
      lib.rs:1313 registration (~200 — read collapses to None, the
      exact always-None behavior today), workspace.rs's
      `pub use warp_graphql::billing` re-export + ServiceAgreement
      field localized to local copies (drop cynic derives),
      crates/ai/llm_provider.rs From<gql LlmProvider> impl,
      artifacts/mod.rs TryFrom<gql AIConversationArtifact> + its
      test half. Risk LOW-moderate — the policy/usage local types
      stay but become permanently-default; do NOT cascade-prune
      workspace policy fields here (follow-up candidates, local-path
      churn). SLICE 3b — warp_graphql dead modules (~20 files,
      ≈−2,100): delete billing, workspace, get_conversation_usage,
      object, folder, notebook, workflow, user, error,
      mcp_gallery_template, object_actions, response_context,
      ai_tests, billing_tests; shrink ai.rs to AgentTaskState +
      AgentHarness (or move the two enums to crates/ai — decide in
      slice); shrink generic_string_object.rs by repointing
      input_context.rs:283 to the LOCAL GenericStringObjectFormat's
      Display first. Update object_permissions.rs toward OwnerType-
      only (AccessLevel's last users are slice-2b's wall + warp_
      server_auth conversions — verify). Risk moderate: cynic
      fragments cross-reference; delete leaf modules first and
      compile the crate per step. SLICE 4 — schema crate fold +
      terminal tidy (~8 files, ≈−50 code + 1 crate): move
      api/schema.graphql + the schema module into crates/graphql,
      delete crates/warp_graphql_schema + workspace member + TS
      tooling, update AGENTS.md's GraphQL section. Risk LOW; hand-
      compile (build.rs change). Total endgame ≈ −6,200..−7,000.

      LANDMINES: (1) persisted sqlite shapes are NOT serde'd through
      the falling types — cloud_object_persistence reads/writes
      columns and builds Owner/RevisionAndLastEditor directly
      (objects.rs:155,604); the serde formats that MUST stay exactly:
      SyncId/ServerId custom serde (ids.rs), Owner
      User{user_uid}/Team{team_uid}, ServerObjectContainer,
      SharingAccessLevel, GenericStringObjectFormat/JsonObjectType
      (persisted as strings via as_str/sqlite prefixes). Deleting
      ServerObjectGuest etc. touches no persisted bytes (server
      response types, never written). (2) NO migrations this round —
      the `cloud_objects_refreshes` table and sync-retry columns stay
      in schema.rs; slices stop reading/writing them. (3) cynic
      derive coupling: surviving fragments pin their transitive
      types (get_user → OwnerType/experiment/request_context/Time/
      AnonymousUserType); exhaustively compile crates/graphql after
      each module deletion. (4) input_context.rs's GraphQLFormat is
      a live local string formatter for AI prompt payloads — repoint
      before deleting the gql enum (slice 3b). (5) workspace.rs
      re-exports billing types used as local field types
      (ServiceAgreement, AiCreditsUsage*) — localize first (3a) or
      billing.rs cannot fall (3b). (6) cfg/wasm: client.rs + auth
      chain carry wasm gates presubmit never compiles — slices 3b/4
      must be verbatim-subtractive there. (7) `pub` aliases/types
      (Server* in cloud_object_models) produce no dead-code warnings
      — the zero-production-caller evidence is the trace above, not
      the compiler. (8) ConflictStatus/has_conflicting_changes is
      LIVE (active_notebook_data.rs:271) even though conflicts can
      now never arise at runtime — keep the state machine, delete
      only its dead feeders (update_from_server_object, 2b). (9)
      integration_testing QA (assertions.rs, cloud_object/mod.rs,
      workflow/step.rs, rules/step.rs) drives UpdateManager — stays
      green through every slice; do not repoint it. (10) the warp
      Local-channel login flow (AuthClientImpl → GetUser) is
      untouchable: no slice may stub or reorder AuthClientImpl/
      AuthSession; network-log install order stays. (11) AGENTS.md
      says "Schema and client code generation from
      crates/warp_graphql_schema/api/schema.graphql" — stale after
      slice 4; update it in that slice. (12) server_api.rs's
      AuthEvent-pump comments still mention ServerApi (left in 4fj)
      — cosmetic, not this endgame's scope. DESIGNATED NEXT:
      slice 1 exactly as scoped above. Acceptance for THIS round:
      no code changes — this commit contains only plan.md (verified
      with `git show --stat`); no clippy/nextest required; app not
      launched; no .rs file touched.

- [x] **endgame slice 1 — dead sync events + force-refresh chain
      (4fl) — DONE 2026-09-24.** Executed the 4fk SLICE 1: the four
      zero-sender sync-queue ModelEvents, their handler arms, the
      cloud_object_persistence sync helpers, the whole orphaned
      force-refresh chain, and the one-hop orphans that surfaced —
      22 files changed, 37 insertions(+), 882 deletions(−), net
      −845 (2 files deleted outright:
      cloud_object_persistence/src/refresh.rs,
      cloud_objects/src/cloud_object/creation.rs).

      PER-EVENT VERIFICATION (all at HEAD ea4ef344, pre-delete).
      A repo-wide PCRE sweep (`(?<![A-Za-z0-9_])ModelEvent::` —
      lookbehind form because `\b` fails on this host — excluding
      the terminal ModelEvent, a different enum) shows each of the
      four names appears ONLY in the enum definition
      (persistence/mod.rs) and the sqlite subscriber match
      (persistence/sqlite.rs): MarkObjectAsSynced,
      IncrementRetryCount, UpdateObjectAfterServerCreation,
      RecordTimeOfNextRefresh — zero senders each, exactly as the
      4fk predicted. Deleted the 4 variants + their 4 handler arms
      together (exhaustive-match safe: arms and variants removed in
      the same edit set), plus the now-unused imports
      (RevisionAndLastEditor, ServerCreationInfo, ServerTimestamp,
      chrono::Utc) in persistence/mod.rs.

      CHAIN SPAN + DELETION EVIDENCE. Force-refresh chain, verified
      orphaned end-to-end before deletion: sqlite.rs read
      (`read_time_of_next_force_object_refresh` at the old
      :2844) → PersistedData.time_of_next_force_object_refresh
      field → lib.rs tuple wiring (old :1204/:1225/:1574) →
      CloudModel::new third param → the
      time_of_next_force_refresh field →
      cloud_objects_force_refresh_pending +
      mark_cloud_objects_refresh_as_completed (the latter had zero
      callers even before this round) → RecordTimeOfNextRefresh
      event → record_time_of_next_refresh + refresh.rs
      (cloud_objects_refreshes table read/write). Every link had
      zero live callers outside the chain itself. Also fell:
      CloudModel's set_latest_revision_and_editor,
      check_and_maybe_clear_current_conflict (zero callers each),
      update_object_after_server_creation (callers were the three
      dedicated model_tests tests + one profiles_tests block, all
      in slice-1 fallout), the MIN/MAX_MINUTES_UNTIL_NEXT_FORCE_
      REFRESH constants + the rand/Duration imports, and the three
      cloud_object_persistence helpers mark_object_as_synced/
      increment_retry_count/update_object_after_server_creation.

      ONE-HOP CONSEQUENCES RESOLVED IN THIS ROUND. (1)
      CloudModelEvent::ObjectSynced — its ONLY emitter was
      CloudModel::update_object_after_server_creation, so the
      variant fell with its 7 dead subscriber arms:
      cloud_environments/catalog.rs, ai_document_model.rs (its
      reconcile arm; reconcile_server_backed_notebook stays live
      via ObjectCreated/ObjectUpdated), cloud_object/model/view.rs,
      drive/index.rs, mcp_servers/list_page.rs,
      templatable_manager/native.rs, execution_profiles/profiles.rs
      (the if-let arm calling replace_client_id_with_server_id —
      restructured to the InitialLoadCompleted-only check). (2)
      replace_client_id_with_server_id itself: its only production
      trigger was the deleted ObjectSynced arm, so it fell together
      with the two profiles_tests tests whose sole purpose was
      driving it (migration_retries_after_pending_legacy_profile_
      receives_server_id, materialized_pending_profile_is_rekeyed_
      after_server_id_arrives — both also rode slice-2a's
      upsert_from_server_object). legacy_profile_id/
      imports_legacy_profiles stay (12+ live callers). (3)
      CloudObject::set_server_id trait method + its single impl —
      only caller was the deleted update_object_after_server_
      creation. (4) RevisionAndLastEditor struct — the 4fk
      "sqlite read path" note is stale at HEAD; its only remaining
      users were the falling methods, so the struct fell (cloud_
      objects/cloud_object/mod.rs). (5) creation.rs
      ServerCreationInfo — after slice-1 removals its users were
      zero (the 4fk slice-2b listing front-ran; 2b just gets
      smaller). (6) NewCloudObjectsRefresh + CloudObjectsRefresh
      persistence structs — only users were refresh.rs. (7) Test
      fallout beyond the named blocks: the
      cloud_model_sync_event_reconciles_stale_document_client_id
      test + its add_server_backed_plan_notebook helper
      (ai_document_model_tests.rs, fed the deleted variant
      directly), and CloudModel::new 2-arg updates at 3 test call
      sites + the #[cfg(test)] CloudModel::mock constructor.
      Final PCRE sweep over every deleted name: zero references
      remain; nothing newly-orphaned left unresolved.

      Deliberately left (per the 4fk plan): SLICE 2a — CloudModel
      server-intake functions + the remaining server-half tests
      (upsert_from_server_object family persistence.rs
      :402-532/:1662-1749 pre-4fl numbering,
      update_cloud_object_if_exists, bulk equivalents; the server
      halves of profiles_tests.rs/model_tests.rs/data_source_
      tests.rs). SLICE 2b — GenericServerObject + Server* aliases +
      Server* metadata/permissions types + the conversion wall
      (server_object.rs with ConflictStatus kept, the
      cloud_object/mod.rs :786-1017 TryFrom wall, :228-262 gql
      ObjectType conversions, server_cloud_object.rs; creation.rs
      already gone). SLICE 3a — app conversion walls + pricing.
      SLICE 3b — warp_graphql dead modules. SLICE 4 — schema crate
      fold. Also untouched by design: the cloud_objects_refreshes
      sqlite table + object_metadata sync-retry columns (no
      migrations, slices stop reading/writing them), UpdateManager
      (local, stays entirely), ConflictStatus state machine (live
      reader in active_notebook_data.rs).

      Local-only safety: every deletion is a zero-sender event, a
      zero-caller function, or a subscriber arm of a
      never-emitted event — no reachable runtime path changed.
      UpdateManager's local write paths (SQLite + in-memory
      CloudModel) untouched; sqlite schema.rs and migrations
      untouched (verified: no schema/migration file in the diff);
      the local warp-server login flow (AuthClientImpl → GetUser)
      untouched; persisted serde shapes never routed through the
      falling types (per the 4fk landmines), so no stored bytes
      change.

      Acceptance: (1) clippy baseline diff — captured FIRST at
      HEAD in both configs (sort -u pairs), post-change runs are
      WARNING-IDENTICAL: the same 12 pre-existing warnings
      (11 needless-return in terminal/input.rs at identical line
      numbers — input.rs untouched; 1 single-element-loop in
      terminal/model/lifecycle/mod_tests.rs:272), one new
      transient warning (replace_client_id_with_server_id never
      used) was resolved by deleting the method rather than
      suppressed. (2) cargo check suite 0 errors, both feature
      sets (-p warp --lib --all-targets default + --no-default-
      features --features simplewarp), --bin simplewarp,
      --bin warp-oss, --all-targets -p integration (shows only
      the 2 pre-existing step.rs unused-import warnings),
      --lib --tests --features skip_login, -p cloud_objects
      --all-targets. (3) ./script/format — idempotent, no diff
      after rerun. (4) nextest -p warp --lib: default 4,608 run
      (4,614 baseline − 6 tests deleted this round), 4,607
      passed + the known test_command_block_dispatches_event
      load flake failing once and passing on isolated rerun,
      3 skipped, 0 failed; simplewarp 4,607 run (= 4,613 − 6),
      all passed, 3 skipped, 0 failed. Runtime smoke SKIPPED per
      round instructions (user away — app must not be launched;
      password prompt unanswerable); compile + test evidence
      stands in. DESIGNATED NEXT: slice 2a exactly as scoped in
      the 4fk plan (CloudModel server-intake + server-half
      tests, ≈−700, half of it tests).

- [x] **endgame slice 2a — CloudModel server-intake half (4fm) —
      DONE 2026-09-24.** Executed the 4fk SLICE 2A: the nine
      server-intake functions on CloudModel, their test halves,
      and the one-hop orphans that surfaced (the bulk_upsert_event
      trait method + its ModelEvent variants) — 17 files changed,
      132 insertions(+), 674 deletions(−), net −542 (no whole
      file deleted; 4fk's ≈−700 estimate assumed deleting the
      server-half test files, but nearly all of those tests were
      LOCAL migration/scoring tests that got re-seeded instead —
      the "largest test rewrite" the 4fk predicted).

      PER-FUNCTION VERIFICATION (all at HEAD 2ccd869df,
      pre-delete; repo-wide PCRE call-syntax sweep
      `\.NAME\s*\(` plus qualified-path form). Every intake
      function had ZERO production callers — the server halves
      that fed them died in earlier rounds (4fl confirmed). Only
      test callers remained:
      (1) `upsert_from_server_object` — profiles_tests.rs ×15,
      data_source_tests.rs ×6, the in-file wrappers, and the
      cfg(test) `update_objects`; DELETED.
      (2) `upsert_from_server_object_internal` — only caller
      `update_objects_from_initial_load` (itself falling);
      DELETED.
      (3) `update_cloud_object_if_exists` — only callers were
      (1) and (2); DELETED.
      (4) `upsert_from_server_notebook` — only
      `upsert_from_server_cloud_object` + model_tests.rs:244 +
      notebook_tests.rs:142; DELETED.
      (5) `upsert_from_server_cloud_object` — the ServerCloudObject
      dispatcher had ZERO callers repo-wide (not even tests);
      DELETED.
      (6) `upsert_from_server_folder` — only the dispatcher;
      DELETED.
      (7) `upsert_from_server_workflow` — only the dispatcher;
      DELETED.
      (8) `update_objects_from_initial_load` (bulk equivalent 1)
      — only profiles_tests.rs:325/721/1033 + a doc-comment
      mention in profiles.rs (comment fixed); DELETED.
      (9) `#[cfg(test)] update_objects` (bulk equivalent 2, the
      4fk's pre-4fl :1662-1749 block) — only
      model_tests.rs:271/272; DELETED.

      LIVE-SHAPE TRAPS: none — no intake function was reachable
      from live code. One pre-existing deadness surfaced: 
      `CloudModelEvent::InitialLoadCompleted` has no production
      emitter at HEAD (only profiles_tests emits it — kept, the
      rewrites need it); this is NOT newly orphaned (it was
      already emitter-less before this round) and its 6 subscriber
      arms stay; recorded under next-candidates.

      ONE-HOP CONSEQUENCES RESOLVED IN THIS ROUND. (1)
      `GenericCloudObject::update_from_server_object`
      (crates/cloud_objects/generic_cloud_object.rs) — only caller
      was `update_cloud_object_if_exists`; DELETED. It was the
      last PRODUCER of `ConflictStatus::ConflictingChanges`; per
      landmine 8 the state machine stays (field, enum,
      has_conflicting_changes with its live reader
      active_notebook_data.rs:271, and the trait consumers
      conflicting_object_revision/clear_conflict_status/
      replace_object_with_conflict). (2)
      `maybe_open_welcome_folder` — only caller was
      update_objects_from_initial_load; DELETED, cascading to
      drive/mod.rs `should_auto_open_welcome_folder` +
      `write_has_auto_opened_welcome_folder_to_user_defaults`
      (zero other callers) and the orphaned
      `HAS_AUTO_OPENED_WELCOME_FOLDER` const in drive/settings.rs;
      chain dead end-to-end. (3) `create_object_internal` — its
      "used during initial load" purpose vanished; inlined into
      `create_object` (create_object itself keeps live callers:
      UpdateManager, drive index tests, catalog tests). (4)
      `CloudModelType::bulk_upsert_event` + its 4 impls
      (folders.rs, notebooks/mod.rs, workflows/mod.rs,
      generic_string_model.rs) — the ONLY call site in the
      workspace was the falling cfg(test) `update_objects`;
      DELETED, cascading to the 4 `ModelEvent` variants it built —
      UpsertWorkflows/UpsertNotebooks/UpsertFolders/
      UpsertGenericStringObjects — whose only constructors those
      impls were; variants + their 4 sqlite handler arms deleted
      in the same edit set (exhaustive-match safe; the singular
      Upsert* variants and the upsert_workflows/notebooks/folders/
      generic_string_objects sqlite helpers stay, still fed by the
      singular events; `From<CloudObjectUpsertParams>` stays, fed
      by the singular impls). (5) the app re-export
      `pub use cloud_object_models::{ServerCloudObject,
      ServerFolder, ServerNotebook, ServerWorkflow}`
      (app/src/cloud_object/mod.rs) — unused after the dispatcher
      died; removed, repointing notebooks/editor/model_tests.rs to
      `cloud_object_models::ServerWorkflow` directly (front-runs
      the 2b re-export item; the cloud_object_models side of the
      aliases stays for 2b).

      TEST HANDLING (delete vs rewrite, per the 4fk "keep the
      local halves green"). model_tests.rs: deleted
      `test_update_with_deleted_objects` (the round's ONLY test
      deletion; 9→8) — it exercised the intake machinery itself
      (upsert_from_server_notebook + update_objects
      reconciliation) — plus its 4 orphaned server-mock helpers
      (mock_server_metadata/mock_server_permissions/
      mock_server_workflows/mock_server_notebooks) and the
      chrono::Utc/ServerMetadata/ServerPermissions/
      NumInFlightRequests/CloudWorkflowModel imports. The other 8
      local tests untouched. data_source_tests.rs: both
      RulesDataSource scoring tests KEPT; seeding rewritten from
      the ServerAIFact alias + upsert_from_server_object to
      locally-constructed `CloudAIFact` (GenericCloudObject::new
      with a hand-built CloudObjectMetadata carrying the same
      revision timestamps that drive the scoring) + `add_object`.
      profiles_tests.rs: all 17 tests KEPT (they test LOCAL
      migration logic); seeding rewritten to `add_object` of
      locally-constructed `CloudAIExecutionProfile`/`CloudPreference`;
      owned_legacy_profile dropped its ServerMetadata-only
      metadata_id param; tests whose old upsert seeding relied on
      the created-handler's Unsynced→Synced transition now emit
      `CloudModelEvent::InitialLoadCompleted` after seeding to
      drive the equivalent reconcile_with_cloud_state_after_
      initial_load transition (the event stream production
      actually produces for initial-load objects; verified
      handle_ai_execution_profile_created and the reconcile
      handler perform the same state transitions), while tests
      that used the events-free update_objects_from_initial_load
      were re-seeded without any emit, preserving each test's
      original event semantics; the reconciles_... doc comment's
      reference to the deleted function name reworded.
      notebook_tests.rs: the `initial_load` helper was vestigial —
      all 4 call sites passed EMPTY vectors (no-ops with comments
      already saying so); helper + call sites + ServerNotebook
      import deleted, 5 tests kept. Final PCRE sweep over every
      deleted name: zero references remain.

      Deliberately left (per the 4fk plan): SLICE 2b —
      GenericServerObject (server_object.rs; ConflictStatus
      stays), the remaining 13 Server* aliases in
      cloud_object_models, server_cloud_object.rs (NOTE: its last
      consumer, upsert_from_server_cloud_object, died this round —
      2b just gets smaller, same as creation.rs after 4fl), the
      cloud_object/mod.rs TryFrom wall (:786-1017) + :228-262 gql
      ObjectType conversions, and GenericCloudObject::new_from_
      server + CloudObjectMetadata/CloudObjectPermissions::
      new_from_server (they keep ONE out-of-scope test caller, the
      mock_server_workflow helper in notebooks/editor/
      model_tests.rs:1860, which falls with 2b's Server* types).
      SLICE 3a — app conversion walls + pricing (PricingInfoModel
      verified NOT intake-shaped this round: a standalone
      always-None singleton model with no gql/intake entanglement
      — untouched). SLICE 3b — warp_graphql dead modules. SLICE 4
      — schema crate fold. Also untouched by design:
      UpdateManager's local write paths, the login flow, sqlite
      schema/migrations, ConflictStatus state machine.

      NEXT CANDIDATES (beyond the planned slices): (a) slice 2b
      exactly as scoped above — DESIGNATED NEXT; (b) a follow-up
      micro-slice candidate: CloudModelEvent::InitialLoadCompleted
      (variant + its 6 subscriber arms at cloud_object/model/
      view.rs:357, cloud_environments/catalog.rs:40, ai_document_
      model.rs:402, execution_profiles/profiles.rs:399/1868,
      drive/index.rs:1113) — emitter-less in production at HEAD;
      requires rewriting the 3 profiles_tests tests that emit it
      by hand to drive the reconcile handler directly; do it with
      or after 2b/3a; (c) step.rs cosmetic: the AuthEvent-pump
      comments (4fk landmine 12) still pending, not this endgame.

      Local-only safety: every deletion is a zero-production-
      caller function, a constructor-less event variant with its
      handler arm, or a cfg(test) helper — no reachable runtime
      path changed. UpdateManager's local write paths (upsert_
      event/bulk_upsert_event CONSTRUCTORS were never touched —
      note `bulk_upsert_event` the trait METHOD was only ever
      called from the deleted test helper; UpdateManager sends
      the singular `upsert_event`, which stays) untouched; sqlite
      schema.rs and migrations untouched (verified: no schema or
      migration file in the diff; the cloud_objects_refreshes
      table and sync-retry columns keep their no-migration
      status); the local warp-server login flow (AuthClientImpl →
      GetUser) untouched; persisted serde shapes never routed
      through the falling types (Owner/Revision/ids serde, the
      serde formats list in landmine 1 — all stay; the deleted
      ModelEvent variants were write-only events, not persisted
      shapes), so no stored bytes change. The rewritten tests
      assert identical end states: default nextest pass counts
      differ from baseline by exactly the 1 deleted test.

      Acceptance: (1) clippy baseline diff — captured FIRST at
      HEAD in both configs (12 warnings each, sort -u pairs:
      11 needless-return in terminal/input.rs at identical lines +
      1 single-element-loop in terminal/model/lifecycle/
      mod_tests.rs:272; input.rs untouched all round); post-change
      runs are WARNING-IDENTICAL to baseline in BOTH the default
      and `--no-default-features --features simplewarp` configs
      (12 = 12, machine-diffed on location+message pairs). (2)
      cargo check suite 0 errors: -p warp --lib --all-targets
      default AND simplewarp; --no-default-features --features
      simplewarp --bin simplewarp; --bin warp-oss; --all-targets
      -p integration (shows only the 2 pre-existing
      integration_testing/input/step.rs unused-import warnings);
      -p warp --lib --tests --features skip_login; -p
      cloud_objects --all-targets. (3) ./script/format — ran
      twice, idempotent, the 17-file working tree is already in
      formatted state. (4) nextest -p warp --lib --no-fail-fast:
      default 4,607 run = 4,608 baseline − 1 deleted test
      (test_update_with_deleted_objects), 4,607 passed, 3 skipped,
      0 failed, no flake encountered; simplewarp 4,606 run =
      4,607 baseline − 1, 4,606 passed, 3 skipped, 0 failed.
      Runtime smoke SKIPPED per round instructions (user away —
      app must not be launched; password prompt unanswerable);
      compile + test evidence stands in.

- [x] **endgame slice 2b — GenericServerObject, the Server* aliases,
      and the GraphQL conversion wall (4fn) — DONE 2026-09-24.**
      Executed the 4fk SLICE 2B with two scope adjustments
      discovered during verification: ServerAIConversationMetadata
      STAYS (see decision below), so ServerMetadata stays intact
      and ServerPermissions stays trimmed to its live fields —
      27 files changed, 51 insertions(+), 989 deletions(−), net
      −938 (2 files deleted outright:
      cloud_objects/src/cloud_object/server_object.rs,
      cloud_object_models/src/server_cloud_object.rs).

      PER-NAME VERIFICATION (all at HEAD b8efe85bd, pre-delete;
      repo-wide PCRE sweeps with lookbehind form
      `(?<![A-Za-z0-9_])Name` because `\b` is unreliable on this
      host). (1) GenericServerObject — only definers/users were
      server_object.rs, generic_cloud_object.rs's conflict_status
      field + new_from_server, the 13 cloud_object_models aliases,
      and server_cloud_object.rs's TryFromGql bridge; the bridge's
      TryFrom<warp_graphql::object::CloudObject> entry had zero
      callers repo-wide (as 4fk predicted). (2) All 13 Server*
      aliases (ServerFolder/ServerNotebook/ServerWorkflow/
      ServerPreference/ServerEnvVarCollection/ServerWorkflowEnum/
      ServerAIFact/ServerMCPServer/ServerTemplatableMCPServer/
      ServerAIExecutionProfile/ServerAmbientAgentEnvironment/
      ServerScheduledAmbientAgent/ServerCloudAgentConfig) +
      ServerCloudObject — after 4fm, ONLY server_cloud_object.rs
      consumed them (plus the model_tests mock_server_workflow
      test helper). (3) ServerGuestSubject/ServerObjectGuest/
      ServerLinkSharing — zero references outside
      cloud_objects/cloud_object/mod.rs; their only constructors
      were the wall's TryFrom impls; zero field readers. (4) The
      conversion wall in cloud_object/mod.rs — TryFrom<gql
      ObjectMetadata/ObjectPermissions/ObjectGuest/GuestSubject/
      LinkSharing/Container/Space>, From<Owner> for gql Owner,
      From<CloudObjectEventEntrypoint>/From<GenericStringObject
      UniqueKey>/From<UniquePer> for their gql twins, and the
      :226-260 ObjectType↔gql ObjectIdType/ObjectType
      conversions — a full enumeration of every
      `warp_graphql::`-importing file outside crates/graphql
      shows NO file can even name those gql target types (the
      only gql consumers elsewhere are warp_server_auth's login
      core, ServerTimestamp users, and slice-3a/3b surfaces:
      pricing/billing, workspaces/gql_convert, artifacts TryFrom,
      llm_provider, user_profile, input_context's GraphQLFormat).
      ZERO invocations → all deleted. (5) new_from_server trio +
      GenericCloudObject::new_from_server — only caller was the
      model_tests mock_server_workflow helper (rewritten, test
      kept). (6) One-hop dead chains found by parameter sweep:
      CloudModel::maybe_update_object_metadata(_internal) and
      update_object_permissions(_internal) (app
      cloud_object/model/persistence.rs) — the only remaining
      production takers of ServerMetadata/ServerPermissions —
      had ZERO callers (incl. tests, incl. integration_testing);
      they fell, orphaning CloudObjectMetadata::update_from_new_
      metadata_ts, CloudObjectPermissions::update_from_new_
      permissions_ts, and CloudObjectMetadata::update_revision_
      _from_server, all deleted.

      ConflictStatus state machine (landmine 8) PRESERVED with a
      re-typed payload: GenericCloudObject::conflict_status was
      ConflictStatus<GenericServerObject<K, M>> — the deleted
      type WAS the conflict snapshot. Re-pointed to
      ConflictStatus<Self> and moved ConflictStatus verbatim
      from server_object.rs into generic_cloud_object.rs (its
      only type-level consumer; the deleted file's glob re-export
      path stays valid via the generic_cloud_object glob). K
      needed a PhantomData<fn() -> K> marker — the deleted
      GenericServerObject had silently carried it. The two
      readers keep identical semantics: conflicting_object_
      revision reads object.metadata.revision (now Option on the
      local CloudObjectMetadata), and replace_object_with_
      conflict inlines update_revision_from_server's exact
      unconditional revision+last_editor_uid overwrite before
      set_model(object.model().clone()). Trait surface
      (has_conflicting_changes with its live
      active_notebook_data.rs:271 reader, conflicting_object_
      revision, clear_conflict_status, replace_object_with_
      conflict) untouched.

      ServerAIConversationMetadata DECISION: NOT deleted — the
      4fk/2b preconditions ("set_server_metadata has zero
      production callers; delete it here") do not hold at HEAD.
      set_server_metadata indeed has zero production callers
      (merge_cloud_conversation_metadata and set_server_metadata_
      _for_conversation are already caller-less) and there is no
      production constructor at all — every value is permanently
      None — but the TYPE has live production READERS: pane_impl
      selected_conversation_server_metadata (read by
      agent_icon.rs's terminal_view_agent_icon_variant for
      ambient_agent_task_id), history_model's get_server_
      _conversation_metadata read by context_menu.rs:212,
      workspace/view.rs:3679 (owner check via metadata.creator_
      _uid + permissions.space), and agent_conversations_model/
      entry.rs:350 (creator display), plus AIConversation's
      server_id()/orchestration_harness fallback/title update.
      Deleting it means restructuring live UI code (sharing
      dialog, context menu, conversation list, icon logic), not
      "rewriting test-only callers" — out of 2b scope. CONSEQUENCE:
      ServerMetadata STAYS intact (uid/creator_uid have live
      readers through it); ServerPermissions STAYS trimmed — its
      guests/anyone_link_sharing fields (typed by the falling
      trio) had zero readers and zero constructors once the wall
      fell, so the struct now holds only space +
      permissions_last_updated_ts (mock_personal updated; 1 test
      literal updated). Recorded under next-candidates as a
      future micro-slice: kill the whole always-None
      ServerAIConversationMetadata vertical including its UI
      readers.

      ONE-HOP CONSEQUENCES RESOLVED. (1)
      CloudModelEvent::ObjectPermissionsUpdated — its ONLY
      emitter was the deleted update_object_permissions; variant
      + its 5 subscriber arms deleted in the same edit set
      (exhaustive-match safe; env_var_collection.rs,
      drive/index.rs, cloud_environments/catalog.rs,
      ai_document_model.rs, cloud_object/model/view.rs). All
      other CloudModelEvent variants keep live emitters
      (verified per-variant). update_editor_interactivity keeps
      its other caller. (2) The GraphQL crate keeps
      From<GenericStringObjectFormat> for gql
      GenericStringObjectFormat in cloud_objects — input_context
      .rs:283 converts through it live (4fk landmine 4; slice 3b
      repoints). (3) WorkflowId/NotebookId/CloudFolder etc. stay
      (12+ live uses each; only the alias lines fell).

      TEST HANDLING. Zero tests deleted. model_tests.rs
      mock_server_workflow rewritten to construct CloudWorkflow
      locally (GenericCloudObject::new + CloudObjectMetadata::
      :mock + CloudObjectPermissions::mock_personal, same
      SyncId::ServerId id) — test_interleaving_command_and_
      embedding kept green with identical embedded ids;
      chrono::Utc/ServerWorkflow/ServerMetadata/ServerPermissions
      imports dropped. history_model_tests.rs'
      create_server_ai_conversation_metadata dropped the two
      trimmed ServerPermissions fields.

      Deliberately left (per the 4fk plan): SLICE 3a — app
      conversion walls + pricing (gql_convert.rs, pricing/,
      workspace.rs billing re-export, llm_provider From<gql
      LlmProvider>, artifacts TryFrom<gql AIConversationArtifact>
      — all confirmed still present at this HEAD). SLICE 3b —
      warp_graphql dead modules; NOTE the 2b deletions made
      these gql types consumer-less outside crates/graphql:
      object::{ObjectType, CloudObjectEventEntrypoint,
      ObjectMetadata, Container, Space, CloudObject(+WithDescendants)},
      object_permissions::{ObjectPermissions, ObjectGuest,
      GuestSubject, LinkSharing, Owner, AccessLevel} (AccessLevel
      now has zero non-graphql users), generic_string_object::
      {GenericStringObject, GenericStringObjectUniqueKey,
      UniquePer}, and user::PublicUserProfile's From target —
      they fall with 3b's module sweep. SLICE 4 — schema crate
      fold. Also untouched by design: UpdateManager (local),
      ToServerId (local), login flow, sqlite schema/migrations
      (no schema/migration file in the diff — verified),
      ConflictStatus state machine, From<GenericStringObjectFormat>
      for gql format (input_context.rs feeds on it until 3b),
      ServerMetadata + trimmed ServerPermissions (live fields of
      the staying ServerAIConversationMetadata).

      NEXT CANDIDATES: (a) slice 3a — DESIGNATED NEXT; (b) the
      ServerAIConversationMetadata always-None vertical
      micro-slice (see decision above; rewrite agent_icon/
      context_menu/workspace-view/entry readers, drop
      AIConversation.server_metadata + AIConversationMetadata
      .server_conversation_metadata + both dead loader fns +
      hydrate_remote_child_placeholder_with_cloud_transcript
      (zero production callers, 2 tests) — then ServerMetadata/
      ServerPermissions fall entirely); (c) InitialLoadCompleted
      micro-candidate from 4fm still open; (d) PRE-EXISTING
      break discovered: `-p warp --lib --features test-util`
      fails at HEAD too (input_model.rs:22 gates
      `use warpui::EntityId` behind #[cfg(test)] while its
      #[cfg(any(test, feature = "test-util"))] mock uses it —
      one-line gate fix, not caused by 2fn, left alone).

      Local-only safety: every deletion is a zero-caller
      function, a constructor-less type with zero field readers,
      a conversion impl whose gql target type is unnameable
      outside the deleted code, or an arm of a now
      emitter-less event. No reachable runtime path changed:
      the conflict state machine's runtime behavior is
      field-for-field identical, all rewritten test helpers
      produce the same model state, persisted serde shapes are
      untouched (the falling types were never persisted —
      landmine 1), UpdateManager's local write paths and the
      warp_server_auth login flow are untouched.

      Acceptance: (1) clippy baseline diff — captured FIRST at
      HEAD in both configs (12 warnings each, sort -u pairs:
      11 needless-return in terminal/input.rs at identical lines
      + 1 single-element-loop in terminal/model/lifecycle/
      mod_tests.rs:272); post-change runs are WARNING-IDENTICAL
      to baseline in BOTH default and `--no-default-features
      --features simplewarp` (machine-diffed, `diff` clean).
      (2) cargo check 0 errors: -p warp --lib --all-targets
      default AND simplewarp; --no-default-features --features
      simplewarp --bin simplewarp; --bin warp-oss; --all-targets
      -p integration (only the 2 pre-existing step.rs
      unused-import warnings); -p warp --lib --tests --features
      skip_login; -p cloud_objects --all-targets (default and
      test-util); -p cloud_object_models --all-targets (default
      and test-util). `--features test-util` on warp surfaces a
      PRE-EXISTING EntityId error verified identical at HEAD
      (candidate (d) above). (3) ./script/format — run to
      idempotency (git diff stable across reruns). (4) nextest
      -p warp --lib --no-fail-fast: default 4,607 run = 4,607
      baseline − 0 (no tests deleted; the only affected test was
      rewritten), 4,607 passed, 3 skipped, 0 failed, no flake;
      simplewarp 4,606 run = 4,606 baseline − 0, 4,606 passed,
      3 skipped, 0 failed. (The simplewarp baseline was
      re-captured cleanly at HEAD via stash after an initial
      capture raced the first edits; the default baseline was
      never contaminated.) Runtime smoke SKIPPED per round
      instructions (user away — app must not be launched;
      password prompt unanswerable); compile + test evidence
      stands in. DESIGNATED NEXT: slice 3a exactly as scoped in
      the 4fk plan (app conversion walls + pricing, ≈−2,000).

- [x] **endgame slice 3a — the app conversion walls + the pricing
      vertical (4fo) — DONE 2026-09-24.** Executed the 4fk SLICE 3A
      with one scope discovery: the pricing vertical's UI surface is
      bigger than the 4fk listing (PricingPromotionState had two LIVE
      view consumers), but every added deletion is provably the same
      always-None path — 25 files changed, 70 insertions(+),
      2,261 deletions(−), net −2,191 (6 files deleted outright:
      workspaces/gql_convert.rs, workspaces/gql_convert_tests.rs,
      pricing/mod.rs, pricing/pricing_tests.rs, ai/pricing_promotion.rs,
      ai/pricing_promotion_tests.rs).

      PER-NAME VERIFICATION (all at HEAD b124e6488, pre-delete;
      repo-wide PCRE sweeps with lookbehind form
      `(?<![A-Za-z0-9_])Name`, call syntax `.name(` / `Name::` /
      `: Name` / imports / re-export paths). (1) gql_convert —
      `gql_convert` appears ONLY in workspaces/mod.rs's module decl
      and its own two files (1,343 + 363 lines, self-referential as
      4fk predicted); every From/TryFrom impl consumes gql types no
      op can construct (GetUser is the only built op and selects none
      of them); the local Workspace/Team/TeamMember/TeamSettings it
      fed stay, constructed from literals (user_workspaces.rs
      setup_test_workspace) and BillingMetadata::default(). Deleted
      file+tests+mod line. (2) PricingInfoModel — `pricing_info` is
      None-initialized with NO setter (only `new()`); the sole live
      read is ai/pricing_promotion.rs:55
      `PricingInfoModel::as_ref(app).promotion_message()`, which can
      only return None — the exact always-None behavior preserved by
      deletion. The three `#[allow(dead_code)]` accessors had zero
      callers; the only other references were the cfg(test)
      registrations (6 files) and pricing_tests.rs's direct private-
      field construction. (3) PricingPromotionState — consumers:
      terminal_message_bar.rs (subscribe Updated + visible_message +
      dismiss via TerminalInputMessageBarAction::DismissPricingPromotion)
      and agent_message_bar.rs (subscribe + visible_message +
      record_visible_promotion→record_displayed + the
      UpgradePricingPromotion/DismissPricingPromotion pill actions).
      END-TO-END NO-OP TRACE: visible_message returns
      promotion_message() = always None → the terminal X button and
      the agent promo pill NEVER render (both are inside
      `if promotion_message.is_some()` / `.map()` over None) → the
      only dispatchers of dismiss/record_clicked/UpgradePricingPromotion
      are unreachable → PricingPromotionStateEvent::Updated never
      emitted → the subscriptions never fire → dismiss's
      private_user_preferences writes never run (the two
      pricing_promotion_*_dismissed keys were never written at any
      point) → record_visible_promotion always early-returns at the
      `.is_none()` check. Every UI branch deleted was unreachable;
      the local message bars render byte-identical output.
      (4) render_dismissible_promo_pill (zero_state_block.rs) — only
      caller was the deleted agent_message_bar pill block. (5)
      AuthManager::upgrade_url — sole caller was the deleted
      UpgradePricingPromotion arm (data_source.rs/workspace view
      upgrade links use UserWorkspaces::upgrade_link*, unrelated);
      generate_auth_state keeps its login_url caller. (6) the
      `From<gql workspace::LlmProvider>` impl in crates/ai/
      llm_provider.rs — the ONLY reference to that gql type outside
      crates/graphql (controller.rs's LlmProvider is
      warp_multi_agent_api's); deleted with its report_error import.
      (7) `TryFrom<gql AIConversationArtifact>` in ai/artifacts/
      mod.rs — zero production constructors of the gql type (only
      GetUser is built; artifact events come from
      api::message::artifact_event, whose From impls stay); deleted
      with sanitized_basename kept (live file_button_label caller).

      BILLING LOCALIZATION (landmine 5 enabler): workspace.rs's
      `pub use warp_graphql::billing::{AiCreditsUsageAndCostSubjectType,
      AiCreditsUsageAndCostType, AiCreditsUsageBucket,
      AiCreditsUsageSource}` + `use ...::{ServiceAgreement,
      ServiceAgreementType}` replaced with local pub types defined in
      workspace.rs, cynic derives dropped — ServiceAgreement trimmed
      to its one read field (`type_`, read by
      BillingMetadata::is_on_build_business_plan; the other five gql
      fields incl. the Time-scalar timestamps were never read and
      nothing constructs the struct — BillingMetadata::default() has
      an empty vec, and the field is `#[serde(skip)]`), enums keep
      their full variant vocabulary incl. Other(String). Same import
      paths keep working (crate::workspaces::workspace::*). crates/
      graphql's billing.rs now has ZERO consumers outside the crate.

      ONE-HOP CONSEQUENCES RESOLVED. (1) The two message-bar views
      were registered via `ctx.add_typed_action_view` (input.rs:2269,
      status_bar.rs:325) which requires the TypedActionView bound —
      both switched to plain `ctx.add_view` (handles are only stored/
      notified/ChildView'd, and neither view handles actions anymore,
      so action routing is identical: empty). (2)
      TerminalInputMessageBarAction + AgentMessageBarAction enums and
      their TypedActionView impls fell with their dispatch sites;
      TerminalMessageArgs lost its promotion_close_mouse_state field.
      (3) zero_state_block.rs: CREDITS_BANNER_FONT_SIZE const (only
      the pill used it) + 6 now-unused imports removed; compiler-
      verified. (4) agent_message_bar: the 5 record_visible_promotion
      call sites + method, the PricingPromotionState subscription,
      the two mouse-state fields, right_element collapses to None
      (render_standard_message_bar gets the same Option::None the
      wasm branch always produced). (5) Test registrations removed
      from 6 files (pane_group/mod_tests, workspace/view_tests,
      test_util/terminal.rs, ai/request_usage_model_tests,
      ai/blocklist/prompt/prompt_alert_tests, terminal/input_tests).

      TEST HANDLING: 10 tests deleted, 0 rewritten, 0 deleted-test
      subjects preserved: 7 in gql_convert_tests.rs (all exercised
      the deleted conversion wall itself — invite filtering /
      settings conversion / StripeSubscriptionPlan mapping /
      invite-link gating are converter logic with no local subject),
      promotion_message_is_exposed_verbatim (pricing_tests),
      agent_and_terminal_dismissals_are_independent
      (pricing_promotion_tests), converts_graphql_file_artifact
      (artifacts/mod_tests — the deleted TryFrom; the 15 remaining
      artifact tests are local serde/roundtrip/parse logic, kept).
      Final PCRE sweep over every deleted name: zero references
      remain (AIConversationArtifact/FileArtifact/gql LlmProvider/
      gql billing now appear only inside crates/graphql and the SDL).

      Deliberately left (per the 4fk plan): SLICE 3b — the
      warp_graphql dead-module sweep, which this round made strictly
      smaller: warp_graphql::billing (whole module),
      warp_graphql::workspace (whole module — LlmProvider was its
      last outside mention), and get_conversation_usage now have
      ZERO consumers outside crates/graphql; ai.rs retains only
      AgentTaskState/AgentHarness consumers (agent_sdk/driver.rs,
      driver/error_classification.rs + tests, harness_availability.rs);
      user::PublicUserProfile keeps its single From in
      cloud_object_models/user_profile.rs; object/object_permissions/
      generic_string_object unchanged from the 2b record
      (From<GenericStringObjectFormat> for the gql format still feeds
      input_context.rs:283 until 3b repoints it). SLICE 4 — schema
      crate fold. Also untouched by design: ServerAIConversationMetadata
      always-None vertical (2b's next-candidate (b), UI restructuring
      out of scope), CloudModelEvent::InitialLoadCompleted (4fm
      next-candidate (c)), UpdateManager (local), login flow,
      sqlite schema/migrations (no schema or migration file in the
      diff — verified), ConflictStatus state machine.

      Local-only safety: every deletion is a zero-caller conversion
      impl (its gql input type unconstructable by any op), a
      None-initialized singleton with no setter whose single read
      collapses to the identical always-None value, UI branches
      unreachable because the value they test is always None, or
      test registrations of those singletons. No reachable runtime
      path changed: message bars render identical output, the two
      dismissal preference keys were never written by any reachable
      path (and remain in the preferences namespace untouched), no
      persisted serde shape routes through a falling type
      (service_agreements is #[serde(skip)]; BillingCycleUsageEntry
      has no serde derives; the localized enums replace types that
      were never serialized — the serde formats in landmine 1 are
      untouched), UpdateManager and the warp_server_auth login flow
      (AuthClientImpl → GetUser) are untouched.

      Acceptance: (1) clippy baseline diff — captured FIRST at HEAD
      in both configs (12 warnings each, sort -u location+message
      pairs: 11 needless-return in terminal/input.rs at identical
      lines + 1 single-element-loop in terminal/model/lifecycle/
      mod_tests.rs:272); post-change runs are WARNING-IDENTICAL in
      BOTH default and `--no-default-features --features simplewarp`
      (machine-diffed, `diff` clean; the 2 transient warnings that
      surfaced mid-round — unused `me`, dead CREDITS_BANNER_FONT_SIZE
      — were resolved by deleting the dead code, not suppressed).
      (2) cargo check 0 errors: -p warp --lib --all-targets default
      AND simplewarp; --no-default-features --features simplewarp
      --bin simplewarp; --bin warp-oss; --all-targets -p integration
      (only the 2 pre-existing integration_testing/input/step.rs
      unused-import warnings); -p warp --lib --tests --features
      skip_login; -p cloud_objects / cloud_object_models / ai
      --all-targets. `--features test-util` on warp surfaces the
      PRE-EXISTING EntityId cfg-gate error, verified byte-identical
      via stash at HEAD (the 4fn-recorded input_model.rs gate, now
      reported at :295); ai/cloud_objects/cloud_object_models
      test-util clean. (3) ./script/format — run to idempotency
      (diff hash stable across reruns). (4) nextest -p warp --lib
      --no-fail-fast: default 4,597 run = 4,607 baseline − 10 deleted
      tests, 4,597 passed, 3 skipped, 0 failed, no flake;
      simplewarp 4,596 run = 4,606 baseline − 10, 4,596 passed,
      3 skipped, 0 failed. Runtime smoke SKIPPED per round
      instructions (user away — app must not be launched; password
      prompt unanswerable); compile + test evidence stands in.
      DESIGNATED NEXT: slice 3b exactly as scoped in the 4fk plan
      (warp_graphql dead modules, ≈−2,100 now that billing/workspace/
      get_conversation_usage are fully consumer-less), with the
      input_context.rs GraphQLFormat repoint done first.

- [x] **endgame slice 3b — the warp_graphql dead modules, shrunk to
      the login core (4fp) — DONE 2026-09-24.** Executed the 4fk
      SLICE 3B; the round was RESUMED after two network-killed
      attempts — the working tree held uncommitted partial work on
      top of f0b249d11 which was re-verified from scratch (every
      deletion's evidence re-derived at HEAD, all five assessment
      points confirmed sound) and then completed: 27 files changed,
      1 insertion(+), 2,263 deletions(−), net −2,262 (14 files
      deleted outright: api/{billing.rs, billing_tests.rs, error.rs,
      folder.rs, generic_string_object.rs, mcp_gallery_template.rs,
      notebook.rs, object.rs, object_actions.rs, user.rs, workflow.rs,
      workspace.rs, ai_tests.rs, queries/get_conversation_usage.rs}).

      PER-MODULE DELETION EVIDENCE (re-verified at HEAD f0b249d11;
      repo-wide PCRE sweeps with lookbehind form
      `(?<![A-Za-z0-9_])Name` over gql-qualified paths
      `warp_graphql::<module>` and unqualified type names, excluding
      the SDL). (1) billing.rs (438) + billing_tests.rs (60) —
      zero references to `warp_graphql::billing` or any of its types
      (AiCreditsUsageBucket, StripeSubscriptionPlan, ServiceAgreement)
      outside crates/graphql at HEAD; 3a's workspace.rs localization
      removed the last consumer. (2) workspace.rs (428) — zero
      `warp_graphql::workspace` references at HEAD (LlmProvider's
      From impl fell in 3a; the billing re-export was localized in
      3a). (3) get_conversation_usage.rs (329) — types-only, the op
      was never built by anything; zero references. (4) object.rs
      (116), folder.rs (18), notebook.rs (34), workflow.rs (10) —
      zero external references (2b's wall was their last consumer;
      confirmed again by sweep: `CloudObjectEventEntrypoint` outside
      crates/graphql appears only as the LOCAL cloud_objects enum).
      (5) user.rs (17) — PublicUserProfile's single external consumer
      was cloud_object_models/user_profile.rs's
      From<PublicUserProfile> impl, deleted with it; the impl had
      zero live feeders (the gql type has zero literal constructions
      outside crates/graphql and no op selects it — GetUser selects
      FirebaseProfile). (6) error.rs (70), response_context.rs (6),
      mcp_gallery_template.rs (27), object_actions.rs (41) — zero
      importers each, exactly as 4fk predicted. (7)
      generic_string_object.rs (79) — its one live consumer
      (input_context.rs:283) repointed (decision 2 below); the
      From<GenericStringObjectFormat> conversion impl in
      cloud_objects/cloud_object/mod.rs deleted with the module. (8)
      ai.rs (246 → 29) — shrunk in place to AgentTaskState +
      AgentHarness; the dead remainder (AIConversationArtifact ×8,
      FileArtifact, AICreditAvailability*, PlatformErrorCode, the
      persistence::model From impls) had zero external references at
      HEAD (the FileArtifact hits elsewhere are the live MAA
      api::message::artifact_event type, a different type).

      THE TWO IN-SLICE DECISIONS. (1) ai.rs STAYS in crates/graphql
      (the 4fk "shrink to the two enums" option, not the move-to-
      crates/ai option) — zero churn to the four live consumer files'
      imports (agent_sdk/driver.rs:34, driver/error_classification.rs
      :1 + its tests, harness_availability.rs:137); the enums remain
      cynic vocabulary typed against the schema link. (2)
      input_context.rs repoint: `object_type:
      string_object.generic_string_object_format().to_string()` on
      the LOCAL cloud_objects enum (its `impl ToString`) — the
      deleted hop built the gql format ("what the server expects")
      and Display'd that. The DriveObjectPayload::GenericStringObject
      .object_type STRING CONTENT changes from schema vocabulary
      (e.g. "JsonAIFact") to the local sqlite-prefix form
      (GENERIC_STRING_JSON_AI_FACT); verified safe: the field is a
      free-form `string object_type` in the multi-agent attachment
      proto (attachment.proto:107), no enum constraint on the wire,
      and no test asserts the old value (only a comment mention in
      warp_agent_page.rs, describing local GSO storage, not the
      payload). The LOCAL GenericStringObjectFormat serde and sqlite
      paths are byte-identical (the repoint touches only this one
      call site).

      ALSO FELL (one-hop cascades). (1)
      create_anonymous_user.rs: the CreateAnonymousUser op machinery
      (variables/input/fragment/result inline fragments/
      CreateAnonymousUserOutput, AnonymousUserExpirationType, the
      define_operation) — zero builders at HEAD (GetUser is the only
      op built anywhere); only the SDL mentions those names. The file
      keeps AnonymousUserType, which warp_server_auth's TryFrom
      (user.rs:40-51) consumes — login-core, intact. (2)
      object_permissions.rs shrunk to OwnerType + its Display:
      AccessLevel, UserGuest, PendingUserGuest, TeamGuest,
      GuestSubject, ObjectPermissions, ObjectGuest, LinkSharing and
      the gql Owner InputObject — at HEAD every one appeared outside
      crates/graphql ONLY in the SDL; warp_server_auth's actual usage
      today is OwnerType only (auth_state.rs:9, credentials.rs:11) +
      get_user types + ServerTimestamp + client + AnonymousUserType.
      The ObjectPermissions hits in cloud_object_persistence are the
      LOCAL diesel struct (schema::object_permissions table), a
      different type. (3) crates/graphql Cargo.toml: dropped deps
      with zero src references at HEAD (websocket, graphql-ws-client
      both targets, ws_stream_wasm, futures, futures-util,
      async-channel — leftovers from the deleted subscription stack)
      and persistence (fell with ai.rs's dead half); dropped the
      [dev-dependencies] http_client[test-util] and the now-consumer-
      less test-util feature (their consumers ai_tests/billing_tests
      fell this round). (4) crates/ai dropped its warp_graphql dep —
      zero src references at HEAD (a 3a leftover: llm_provider's From
      impl was the last use). (5) cloud_object_models dropped its
      warp_graphql dep — user_profile's From impl was the last use.

      WHAT SURVIVED (the login identity core, per the 4fk terminal
      state): client.rs UNTOUCHED (Operation trait/send_request/
      RequestOptions/get_request_context/define_operation; wasm cfg
      gates verbatim), api/queries/get_user.rs (the only op),
      api/mutations/create_anonymous_user.rs (AnonymousUserType),
      api/object_permissions.rs (OwnerType), api/experiment.rs,
      api/request_context.rs, scalars/ (Time via ServerTimestamp,
      Uint32 via impl_scalar), lib.rs + api/mod.rs + the
      `pub use warp_graphql_schema::schema` link (SDL file untouched
      — slice 4 moves it). Remaining external consumers, exactly the
      terminal surface: warp_server_auth (login core), ServerTimestamp
      users (app cloud_object model/mod + update_manager +
      request_usage_model, cloud_objects, cloud_object_persistence),
      the two AI enums (agent_sdk driver + error_classification +
      tests + harness_availability), and auth/user_properties
      (GetUser UserOutput).

      TEST HANDLING: zero -p warp tests touched — the two deleted
      test files (ai_tests 153 lines, billing_tests 60) were
      -p warp_graphql tests, so -p warp counts are expected flat;
      warp_graphql now contains 0 tests (suite runs green with none).

      Deliberately left (per the 4fk plan): SLICE 4 — schema crate
      fold (move api/schema.graphql + the schema module into
      crates/graphql, delete crates/warp_graphql_schema + workspace
      member + the TS download tooling; hand-compile — build.rs
      changes), plus AGENTS.md's stale GraphQL section (landmine 11:
      "Schema and client code generation from
      crates/warp_graphql_schema/api/schema.graphql"; note
      "TypeScript types generated for frontend integration" is also
      stale — no TS codegen remains in the repo) and the terminal
      tidy. Also untouched by design: ServerAIConversationMetadata
      always-None vertical (2b next-candidate (b)),
      CloudModelEvent::InitialLoadCompleted (4fm next-candidate
      (c)), UpdateManager (local), login flow, sqlite
      schema/migrations (no schema or migration file in the diff —
      verified), ConflictStatus state machine.

      Local-only safety: every deletion is a module/type with zero
      consumers outside crates/graphql at HEAD (callgraph evidence —
      pub items give no dead-code warnings), the one deleted op was
      never built by any code, and the single live-behavior touch is
      the content of a free-form AI-attachment label string whose
      consumer is a proto string field. The warp_server_auth login
      flow (AuthClientImpl → GetUser) untouched; client.rs untouched;
      persisted serde shapes untouched (the falling types were
      wire-shaped only; the landmine-1 serde formats all stay);
      UpdateManager and ConflictStatus untouched.

      Acceptance: (1) clippy — post-change runs WARNING-IDENTICAL to
      the 4fo-recorded baseline in BOTH default and
      `--no-default-features --features simplewarp` (12 warnings
      each: 11 needless-return in terminal/input.rs at identical
      lines + 1 single-element-loop in terminal/model/lifecycle/
      mod_tests.rs:272; both files untouched all round; machine-diff
      on location+message pairs, `diff` clean). (2) cargo check
      0 errors: -p warp --lib --all-targets default AND simplewarp;
      --no-default-features --features simplewarp --bin simplewarp;
      --bin warp-oss; --all-targets -p integration (only the 2
      pre-existing integration_testing/input/step.rs unused-import
      warnings); -p warp --lib --tests --features skip_login;
      -p warp_graphql/warp_server_auth/ai/cloud_objects/
      cloud_object_models --all-targets --all-features (test-util
      included, clean). The -p warp --features test-util failure was
      not re-run (known pre-existing EntityId cfg-gate error,
      recorded at 4fn/4fo as pre-existing at HEAD). (3)
      ./script/format — run twice, idempotent (27 changed paths
      stable across reruns). (4) nextest -p warp --lib --no-fail-fast:
      default 4,597 run = 4,597 baseline − 0, 4,597 passed, 3
      skipped, 0 failed, no flake; simplewarp 4,596 run = 4,596
      baseline − 0, 4,596 passed, 3 skipped, 0 failed;
      nextest -p warp_graphql: 0 tests (both test files fell with
      the round), crate compiles green. Runtime smoke SKIPPED per
      round instructions (user away — app must not be launched;
      password prompt unanswerable); compile + test evidence stands
      in. DESIGNATED NEXT: SLICE 4 (schema crate fold + AGENTS.md
      GraphQL section update + terminal tidy) exactly as scoped in
      the 4fk plan.

- [x] **endgame slice 4 — schema crate fold + terminal tidy, the
      endgame CLOSED (4fq) — DONE 2026-09-24.** Executed the 4fk
      SLICE 4; with it the 4ca item (8) endgame is DONE: 15 files
      changed, 7 insertions(+), 2,735 deletions(−), net −2,728
      (numbers are code+config before this ledger entry; 1 whole
      crate deleted: crates/warp_graphql_schema, 9 tracked files
      incl. the 2,480-line yarn.lock), and the SDL moved into
      crates/graphql (+4,808 lines there, rename-tracked, not new
      content).

      FOLD MECHANICS. api/schema.graphql moved VERBATIM to
      crates/graphql/api/schema.graphql — git recorded a rename,
      sha256 identical pre/post (d2d7e30cf…), 4,808 lines; the
      cynic derives' contract is untouched bytes. The schema link
      came home: api/mod.rs's
      `pub use warp_graphql_schema::schema;` replaced by a local
      `#[cynic::schema("warp-server")] pub mod schema {}` — all
      six in-crate consumers `use crate::schema;` and needed no
      edits, and `warp_graphql::schema` still resolves through the
      lib.rs `pub use api::*` glob (sweep: zero external users of
      that path existed). build.rs repointed from
      `../warp_graphql_schema/api/schema.graphql` to
      `api/schema.graphql`, dropping the stale "even though the
      code is generated in the schema crate" comment; verified in
      the cynic-codegen 3.8.0 source that
      register_schema().from_sdl_file() itself emits
      `cargo:rerun-if-changed` for the SDL, so the pure path
      repoint keeps rebuild wiring complete. Cargo.toml: dropped
      `warp_graphql_schema.workspace = true` from crates/graphql
      (the [build-dependencies] anyhow + cynic-codegen stay —
      build.rs uses both) and the workspace-dependency line from
      the root Cargo.toml (membership was via the `crates/*` glob
      — no members entry to edit). Cargo.lock regenerated by
      cargo: zero warp_graphql_schema entries remain.

      TS TOOLING DELETION EVIDENCE. Deleted with the crate:
      package.json (a `graphql-codegen -r ts-node/register`
      generate script), graphql.config.js (staging + local
      projects re-downloading the SDL through a custom loader),
      api/client-schema.ts (an introspection loader filtering the
      schema to 76 mutations / 27 queries / 1 subscription — an
      op vocabulary the workspace stopped having before Phase 4
      began), yarn.lock (2,480 lines), and the crate .gitignore
      (node_modules/). The no-invoker claim was verified BEFORE
      deleting: rg for graphql-codegen|graphql.config|
      client-schema across script/ and .github/ finds nothing
      relevant (the only yarn hit in .github is Corepack setup
      for building TS command signatures) — the SDL refresh was a
      manual dev flow against staging, as the 4fk predicted, and
      GetUser is the only op the client ever sends.

      TERMINAL TIDY. Post-edit repo-wide `rg warp_graphql_schema`:
      zero hits outside the plan.md ledger and the specs/ archive
      (3 historical TECH.md files — QUALITY-731, REMOTE-1545,
      managed-mcp-cli-resolution — left verbatim per repo
      convention: specs/ is a historical task-spec archive no
      Phase 4 round has ever touched; 38 of its specs still
      reference warp_tui, deleted rounds ago). `cargo tree -i
      warp_graphql_schema` errors (package gone from the graph).
      Fold-exposed leftovers in crates/graphql: none — no
      include_str paths, no other schema-crate mentions, every
      remaining [dependencies] entry still has src users (anyhow
      via scalars/time.rs); the crate .gitignore entries
      (node_modules/, src/api/v1) predate the fold and were left.
      AGENTS.md's GraphQL section (4fk landmine 11) rewritten to
      two factual lines: the SDL lives at
      crates/graphql/api/schema.graphql, registered for the cynic
      derives by the crate's build.rs; the client is hand-
      maintained around the login flow (GetUser is the only
      operation sent) — the "TypeScript types generated for
      frontend integration" line is gone (no TS codegen remains
      anywhere, as the 4fp ledger noted).

      ENDGAME VERDICT (the 4fk per-crate terminal-state decision,
      all verified at this HEAD): (1) cloud_objects STAYS — the
      local cloud-object model substrate (ids, object
      types/formats, owner/metadata/permissions/statuses,
      GenericCloudObject + upsert events, GenericStringModel,
      drive, sharing), 87 importer files. (2) warp_graphql STAYS
      as the login identity core, final inventory 13 .rs files /
      582 lines (client.rs 220, queries/get_user.rs 109,
      experiment.rs 78, ai.rs 29 — only AgentTaskState +
      AgentHarness, object_permissions.rs 21 OwnerType,
      request_context.rs 21, mutations/create_anonymous_user.rs
      10 AnonymousUserType, scalars 59, mod/lib plumbing 22,
      build.rs 13) + the 4,808-line SDL now owned in-crate.
      (3) warp_graphql_schema FOLDED AWAY — crate deleted.
      NOTHING workspace-wide can reach a server except the login
      flow's GetUser: the only GraphQL operation built anywhere
      is get_user.rs's GetUser, sent by warp_server_auth's
      AuthClientImpl; ServerApiProvider is {get_http_client,
      get_auth_client} + the AuthEvent pump.

      Acceptance: (1) clippy baseline diff — the 4fp-recorded
      baseline used unchanged (12 warnings: 11 needless-return in
      app/src/terminal/input.rs at identical lines + 1
      single-element-loop in terminal/model/lifecycle/
      mod_tests.rs:272; neither file touched this round);
      post-change runs are WARNING-IDENTICAL in BOTH default and
      `--no-default-features --features simplewarp` (12 = 12
      location+message pairs machine-diffed, `diff` clean, zero
      new warnings). (2) cargo check 0 errors — this round changed
      build.rs wiring, so -p warp_graphql --all-targets was
      hand-compiled FIRST (clean), then: -p warp --lib
      --all-targets default AND simplewarp; --no-default-features
      --features simplewarp --bin simplewarp; --bin warp-oss;
      --all-targets -p integration (only the 2 pre-existing
      integration_testing/input/step.rs unused-import warnings);
      -p warp --lib --tests --features skip_login; -p
      warp_server_auth --all-targets; a full --workspace
      --all-targets check also ran clean. `--features test-util`
      is vacuous on the only touched crate: warp_graphql declares
      no features. `cargo tree -i warp_graphql_schema` errors as
      required. (3) ./script/format — run twice, idempotent
      (working-tree status identical across reruns; zero format
      changes to apply). (4) nextest -p warp --lib --no-fail-fast:
      default 4,597 run = 4,597 baseline − 0, 4,597 passed, 3
      skipped, 0 failed, no flake; simplewarp 4,596 run = 4,596
      baseline − 0, 4,596 passed, 3 skipped, 0 failed. Runtime
      smoke SKIPPED per round instructions (user away — app must
      not be launched; password prompt unanswerable); compile +
      test evidence stands in.

      NEXT CANDIDATES (endgame closed; all pre-existing records,
      none touched this round): (a) the ServerAIConversation-
      Metadata always-None vertical (2b next-candidate (b):
      rewrite the agent_icon/context_menu/workspace-view/entry UI
      readers, drop AIConversation.server_metadata +
      AIConversationMetadata.server_conversation_metadata + both
      dead loader fns + hydrate_remote_child_placeholder_with_
      cloud_transcript — then ServerMetadata/ServerPermissions
      fall entirely); (b) CloudModelEvent::InitialLoadCompleted,
      emitter-less in production (4fm next-candidate (c): variant
      + 6 subscriber arms, 3 profiles_tests emitters to rewrite);
      (c) step.rs's AuthEvent-pump comments still mention
      ServerApi (4fk landmine 12, cosmetic); (d) the pre-existing
      `-p warp --lib --features test-util` EntityId cfg-gate
      error at input_model.rs (4fn discovery, one-line gate fix).

      The 4fk survey entry above is now marked [x]: all six
      slices it planned (1, 2a, 2b, 3a, 3b, 4) are executed and
      recorded (4fl, 4fm, 4fn, 4fo, 4fp, 4fq).

- [x] **AuthEvent-pump comment tidy + test-util EntityId cfg-gate
      fix (4fr) — DONE 2026-09-25.** The two small leftovers from
      the 4fq next-candidates list, items (c) and (d), combined
      into one tidy round: 4 files changed, 7 insertions(+),
      6 deletions(−) including this ledger entry (code diff
      alone: 3 files, +7/−5).

      COMMENT TIDY (4fq candidate (c), 4fk landmine 12). A fresh
      repo-wide `rg 'ServerApi' --type rust` minus ServerApiProvider
      found exactly two comment-only sites — the remembered
      "step.rs" location was stale (4fk-era); the pump has lived in
      app/src/server/server_api.rs since the ServerApi dissolution
      in 4fj, so sites were located by content. (1) the
      AuthEvent::NeedsReauth arm's rationale comment in the pump
      spawned by ServerApiProvider::new ("AuthManager depends on a
      reference to ServerApi, so ServerApi can't easily hold a ref
      to AuthManager. To get around this, we emit an event on
      ServerApi...") rewritten to name ServerApiProvider and the
      real mechanism: the auth client emits on the channel,
      ServerApiProvider's pump calls AuthManager — the
      circular-ref rationale itself re-verified still true
      (auth_manager.rs does `ServerApiProvider::as_ref(ctx).
      get_auth_client()`). (2) the `get_access_token_ignoring_
      validity` doc in crates/warp_server_auth/src/auth_state.rs
      pointed its rustdoc link at the dead
      `[ServerApi::get_or_refresh_access_token]`; retargeted to
      the living `crate::auth_client::AuthClient::
      get_or_refresh_access_token` (the trait method whose own doc
      describes exactly the refresh-if-expired behavior the note
      recommends). Comments only, zero code-semantics changes;
      post-edit sweep clean: no `ServerApi` outside
      ServerApiProvider anywhere in Rust.

      CFG-GATE FIX (4fq candidate (d), 4fn discovery). The red was
      reproduced at HEAD BEFORE editing: `cargo check -p warp --lib
      --features test-util` failed E0433 "use of undeclared type
      `EntityId`" at app/src/ai/blocklist/input_model.rs:295 —
      `EntityId::new()` inside `BlocklistAIInputModel::mock`, which
      is gated `#[cfg(any(test, feature = "test-util"))]`, while
      its import at line 21 was gated `#[cfg(test)]` only, so any
      test-util build without cfg(test) compiled the use site
      without the import. Fix: the one-line gate correction
      `#[cfg(test)]` → `#[cfg(any(test, feature = "test-util"))]`
      on that single `use warpui::EntityId;`. Minimal diff, nothing
      else touched.

      Acceptance: (1) `cargo check -p warp --lib --features
      test-util` GREEN (headline deliverable; red at HEAD per the
      reproduction above). (2) ./script/format run twice,
      idempotent — identical working-tree status across runs,
      format applied no changes of its own. (3) clippy
      WARNING-IDENTICAL to the HEAD baseline captured BEFORE
      editing, in BOTH default and `--no-default-features
      --features simplewarp`: 12 = 12 location+message pairs
      machine-diffed, `diff` clean on both feature sets, zero new
      warnings (the 12 pre-existing: 11 needless-return in
      app/src/terminal/input.rs + 1 single-element-loop in
      terminal/model/lifecycle/mod_tests.rs:272; neither file
      touched this round). (4) `cargo check --no-default-features
      --features simplewarp --bin simplewarp` green. (5) nextest
      -p warp --lib --no-fail-fast: default 4,597 run / 4,597
      passed / 3 skipped / 0 failed; simplewarp 4,596 run / 4,596
      passed / 3 skipped / 0 failed — identical to the 4fq
      post-round baselines, test counts unchanged, no flakes.
      Runtime smoke SKIPPED per standing round policy (no GUI
      tests, app not launched).

      NEXT CANDIDATES (both pre-existing records, untouched this
      round): (a) the ServerAIConversationMetadata always-None
      vertical (2b next-candidate (b): rewrite the agent_icon/
      context_menu/workspace-view/entry UI readers, drop
      AIConversation.server_metadata +
      AIConversationMetadata.server_conversation_metadata + both
      dead loader fns + hydrate_remote_child_placeholder_with_
      cloud_transcript — then ServerMetadata/ServerPermissions
      fall entirely); (b) CloudModelEvent::InitialLoadCompleted,
      emitter-less in production (4fm next-candidate (c): variant
      + 6 subscriber arms, 3 profiles_tests emitters to rewrite).

- [x] **InitialLoadCompleted event vertical deleted (4fs) — DONE
      2026-09-25.** The 4fr next-candidate (b), scoped by 4fm's
      next-candidate (c): 7 files changed, 32 insertions(+),
      44 deletions(−) including this ledger entry.

      DUAL-CONFIRM. (a) Production-emitter sweep: PCRE-lookbehind
      `rg -P '(?<![A-Za-z0-9_])InitialLoadCompleted'` over all Rust
      found ZERO production emitters of
      `CloudModelEvent::InitialLoadCompleted` — the only `ctx.emit`
      sites were the 3 profiles_tests.rs shortcuts (re-seeded in
      4fm); the server initial-load that once emitted the event died
      with the sync path. Disambiguation: the identically-named
      `CloudPreferencesSyncerEvent::InitialLoadCompleted` is a
      DIFFERENT, live vertical (production emitter at
      cloud_preferences_syncer.rs:561; subscribers at
      one_time_modal_model.rs:52 and profiles.rs:389) — untouched
      throughout. (b) Full arm inventory, located by symbol per
      drift protocol: persistence.rs:71-72 (variant + "initial bulk
      load from the server" doc); view.rs:356 (no-op `=> ()`
      or-pattern member — arm did nothing); drive/index.rs:1112
      (no-op `=> {}` or-pattern member — arm did nothing);
      cloud_environments/catalog.rs:40 (or-pattern member sharing
      `catalog.refresh(ctx)` with 7 other variants);
      execution_profiles/profiles.rs:399-401 (`if matches!` block
      in the imports_legacy_profiles CloudModel subscription calling
      `migrate_settings_profiles` — production equivalent survives
      via the syncer-event subscription at :385-394);
      execution_profiles/profiles.rs:1868-1870 (arm calling
      `reconcile_with_cloud_state_after_initial_load`); ai/document/
      ai_document_model.rs:402-404 (arm calling
      `reconcile_all_document_server_backing`).

      DELETIONS. Variant + doc gone; all 6 subscriber sites updated
      (4 or-pattern memberships dropped, 1 if-matches block dropped,
      1 arm dropped — the pre-existing `_ => {}` in
      handle_cloud_model_event predates the round and was not
      extended to paper over anything). No wildcard added anywhere.

      TEST REWRITES (3, all semantics-preserving, assertions
      untouched). (1) `completed_migration_is_not_reapplied_and_
      legacy_ids_restore_after_restart`: the emit became a direct
      `model.reconcile_with_cloud_state_after_initial_load(ctx)` +
      the already-present explicit `migrate_settings_profiles` call
      (subscription order preserved: reconcile then migrate; the
      reconcile is provably a no-op post-migration — Unsynced-arm
      can't match, both profiles already tracked). (2)
      `reset_without_explicit_collection_reimports_the_next_
      accounts_legacy_profile`: emit → direct reconcile + migrate
      on profile_model between the CloudModel add_object and
      complete_cloud_initial_load, mirroring event-flush order.
      (3) `reconciles_unsynced_default_profile_with_cloud_after_
      initial_load`: emit → direct reconcile call (the flag-off
      subscription path made the event drive ONLY the reconcile
      here); test + fn doc comments rewritten to describe the
      bulk-load reconciliation without naming the dead event.

      ONE-HOP ORPHAN VERDICTS (call-syntax `\bname\s*\(` searches).
      `reconcile_with_cloud_state_after_initial_load`: arm was its
      only caller; 3 test callers remain → KEPT per test-only-user
      precedent with `#[cfg_attr(not(test), allow(dead_code))]`
      (house pattern: cloud_preferences_syncer.rs:225, llms.rs:468,
      code_review_view.rs:572) so non-test lib builds stay
      warning-clean. `migrate_settings_profiles`: live production
      callers (AuthComplete sub :380, syncer-event sub :390,
      construction catch-up :465). `reconcile_all_document_
      server_backing`: live production caller
      (`publish_documents_for_conversation` :301). Zero deletions
      beyond the vertical itself.

      Acceptance: (1) ./script/format run twice, idempotent —
      identical 7-file working-tree status across runs, format
      applied no changes of its own. (2) clippy WARNING-IDENTICAL
      to the HEAD baseline captured BEFORE editing, BOTH default
      and `--no-default-features --features simplewarp`: 12 = 12
      location+message pairs machine-diffed (json span capture,
      `diff` clean both sets, zero new warnings; the 12
      pre-existing: 11 needless-return in app/src/terminal/input.rs
      + 1 single-element-loop in terminal/model/lifecycle/
      mod_tests.rs:272; neither file touched this round). (3)
      `cargo check --no-default-features --features simplewarp
      --bin simplewarp` green. (4) `cargo check -p warp --lib
      --features test-util` green (fixed in 4fr, stayed green).
      (5) nextest -p warp --lib --no-fail-fast: default 4,597 run /
      4,597 passed / 3 skipped / 0 failed; simplewarp 4,596 /
      4,596 / 3 / 0 — identical to the 4fq/4fr baselines, ZERO
      test-count delta (3 rewritten, 0 deleted), no flakes. Runtime
      smoke SKIPPED per standing round policy (no GUI tests, app
      not launched).

      NEXT CANDIDATES (one item left, pre-existing record, untouched
      this round): the ServerAIConversationMetadata always-None
      vertical (2b next-candidate (b): rewrite the agent_icon/
      context_menu/workspace-view/entry UI readers, drop
      AIConversation.server_metadata +
      AIConversationMetadata.server_conversation_metadata + both
      dead loader fns + hydrate_remote_child_placeholder_with_
      cloud_transcript — then ServerMetadata/ServerPermissions
      fall entirely).

- [x] **ServerAIConversationMetadata always-None vertical deleted (4ft) — DONE
      2026-09-25.** The 4fr next-candidate (a) / 4fn "restructure-not-delete"
      record, the last queued 4fq item: 24 files changed, 246 insertions(+),
      1,631 deletions(−) including this ledger entry (code diff alone:
      23 files, +64/−1,631).

      DUAL-CONFIRM (both at HEAD, pre-edit). (a) Writers of
      AIConversation.server_metadata: PCRE sweeps `server_metadata\s*:`
      + `\.server_metadata` + lookbehind `(?<![A-Za-z0-9_])
      ServerAIConversationMetadata` found exactly two struct-literal
      writers (conversation.rs `server_metadata: None` in AIConversation::
      new and new_restored — both permanently None; AIConversation is
      Debug+Clone only, no Serialize/Default, so no serde/derive
      spelling can populate it) plus ONE Some-writer:
      AIConversation::set_server_metadata. set_server_metadata's complete
      caller census: set_server_metadata_for_conversation (history_model
      :852, ZERO callers), merge_cloud_conversation_metadata
      (conversation_loader :247, ZERO callers), hydrate_remote_child_
      placeholder_with_cloud_transcript (:2597, ONLY history_model_tests
      callers — its doc-named production caller pane_group::
      hydrate_remote_child_transcript_in_place no longer exists), and
      direct test calls (conversation_details_panel_tests, convert_
      conversation_tests). Writers of AIConversationMetadata.
      server_conversation_metadata: From<&AIConversation> (clones the
      always-None conversation field), from_server_metadata (called only
      by the dead merge_cloud_conversation_metadata), a None literal in
      initialize_historical_conversations, set_server_metadata_for_
      conversation (dead), and update_cached_metadata_for_conversation
      (mirrors the always-None conversation field). PREMISE HELD: no
      live production writer anywhere.

      READER REWRITES (each inlined to its permanent None arm). (1)
      agent_icon.rs terminal_view_agent_icon_variant: dropped the
      server_ambient_task_id lookup; is_cloud = is_cloud_agent_session()
      && !is_local_child (the `|| server_ambient_task_id.is_some()`
      disjunct is dead); its feeder pane_impl::selected_conversation_
      server_metadata deleted (sole caller). (2) context_menu.rs
      conversation_server_token: dropped the .or_else(get_server_
      conversation_metadata) token fallback — loaded-conversation branch
      only. (3) workspace/view.rs open_cloud_conversation_from_server_
      token: the ownership gate read (creator_uid + permissions.space)
      can never establish ownership, so both branches of the fn ended in
      the transcript-viewer failure path; the fn collapsed to that call
      and was DELETED, its FromCloudConversationId arm calling
      load_cloud_conversation_into_new_transcript_viewer directly
      (root_view.rs's open_cloud_conversation_in_existing_window keeps
      the action-arg contract `_: &ServerConversationToken` — uri/mod.rs
      dispatches the token; the warp://conversation deep-link chain that
      can now only ever toast an error is left for a future round). (4)
      entry.rs: conversation_creator reduced to the current-user
      principal (principal_from_user_profile deleted, sole caller);
      ambient_agent_task_id = None; harness = Some(Harness::Oz);
      has_ambient_run = false; can_share fell with can_conversation_
      be_shared — and the AgentConversationCapabilities::can_share FIELD
      itself had zero readers repo-wide (write-only since the share
      dialog died), so the field fell too (agent_icon_tests literal
      updated). (5) block.rs user_avatar_info_for_ai_block deleted
      (server-metadata creator lookup); both call sites use
      current_user_avatar_info directly; user_avatar_info_for_
      conversation_creator + helpers kept for their block_tests callers
      under #[cfg_attr(not(test), allow(dead_code))] (house pattern).
      (6) conversation_details_panel.rs from_conversation: creator/
      conversation_id stay None (the Some arms deleted), harness =
      Some(Harness::Oz); the creator rewrite orphaned PrincipalInfo
      (struct + all three ctors, sole-constructor type), the creator +
      executor render sections, and the executor_agent_link mouse state
      — all deleted; the unused `app` param removed from from_conversation
      (three call sites updated: terminal/view.rs, wasm_view.rs, tests).
      Also rewritten: history_model From<&AIConversation> has_cloud_data
      = server_conversation_token.is_some() only; get_local_
      conversations_metadata filter drops the is_ambient_agent_
      conversation check (permanently false; fn deleted, sole caller);
      update_cached_metadata_for_conversation + apply_conversation_title
      drop their server-metadata mirror blocks; orchestration_harness()
      drops the server-metadata Harness fallback; update_conversation_
      title drops the metadata.title write-back.

      DELETIONS with caller censuses (all pre-delete). AIConversation::
      server_metadata (field), server_id() (ZERO callers — every other
      `server_id()` hit is the drive/exec-profile ServerId type),
      server_metadata() accessor (all callers listed above), set_server_
      metadata (callers above); ServerAIConversationMetadata (type);
      AIAgentHarness (sole value source was the type's harness field;
      harness_display.rs's From<AIAgentHarness> + PartialEq impls —
      only invocation sites were the two rewritten readers — fell with
      it); AIConversationMetadata.server_conversation_metadata (field),
      from_server_metadata (sole caller the dead merge), is_ambient_
      agent_conversation (sole caller the rewritten filter); set_server_
      metadata_for_conversation, get_server_conversation_metadata (all
      four callers rewritten), get_server_conversation_metadata_by_
      server_token (ZERO callers at HEAD), can_conversation_be_shared,
      hydrate_remote_child_placeholder_with_cloud_transcript +
      merged_remote_child_placeholder_conversation_data (sole caller);
      conversation_loader.rs merge_cloud_conversation_metadata,
      CLIAgentConversation (ZERO constructors repo-wide — it required a
      ServerAIConversationMetadata field nobody could build) +
      CloudConversationData::CLIAgent (never constructed; enum keeps the
      single Oz variant), the AIConversationMetadata::merge impl (sole
      caller the dead merge); load_ai_conversation.rs Conversation-
      RestorationInNewPaneType::HistoricalCLIAgent (constructed only in
      pane_group's dead CLIAgent arm) + restore_cli_agent_block_snapshot
      (sole callers the CLIAgent arms); pane_group replace_loading_pane_
      with_terminal + fetch_conversation + agent_view.rs CLIAgent match
      arms rewritten (agent_view's matches!(Oz) branch was permanently
      taken — the Box conditional collapsed to the Oz closure); block.rs
      user_avatar_info_for_ai_block. THEN crates/cloud_objects:
      ServerMetadata + ServerPermissions (+ ServerPermissions::
      mock_personal) deleted — post-edit repo-wide lookbehind sweeps
      find ZERO remaining rust references to either name (the last
      non-test consumers were the type's two fields; the 4fn-trimmed
      ServerPermissions had no other reader); cloud_object_persistence
      hand-checked (no reference — server response types were never
      persisted, 4fk landmine 1 confirmed); ServerGuestSubject/
      ServerLinkSharing already gone since 4fn; FolderId import dropped
      from cloud_object/mod.rs (ServerMetadata was its last local
      consumer; the id type itself stays alive elsewhere).

      ONE-HOP VERDICTS (call-syntax searches). find_conversation_id_
      by_server_token KEPT (live: agent_conversations_model.rs:529,
      block/view_impl/output.rs:999, 13 test sites — the workspace-view
      call that fell was not its only caller). resolve_open_action
      KEPT (entry.rs, conversation_list/view.rs x2, tests — only the
      dead ownership-gated call site fell). restore_conversation_and_
      directory_context KEPT (workspace/view.rs + agent_view.rs).
      parse_orchestration_harness_type KEPT (orchestration_harness).
      usage_metadata_indicates_usage KEPT (new_restored :587). Cloud
      ConversationData keeps a single Oz variant (collapsing the enum
      would touch 8 more call sites — recorded below as a candidate).

      TEST CHANGES (−6 default, −6 simplewarp; all six exercised deleted
      behavior). convert_conversation_tests.rs: the three set_server_
      metadata semantics tests (keeps_known_baseline, stale_snapshot_
      never_regresses, zero_usage_keeps_footer_hidden) + test_server_
      metadata/empty_restored_conversation helpers deleted. conversation_
      details_panel_tests.rs: test_from_conversation_prefers_server_
      creator_profile + create_test_server_metadata deleted (the
      surviving local-fields test asserts the now-permanent server_
      conversation_id.is_none()). history_model_tests.rs:
      test_ambient_agent_conversations_excluded_from_list (premise was
      the deleted ambient filter) and hydrate_remote_child_placeholder_
      with_cloud_transcript_preserves_placeholder_identity (tested the
      deleted fn; its stale pane_group doc-name was already a ghost)
      + create_server_ai_conversation_metadata/server_metadata_with_
      ambient_task helpers deleted; test_metadata literal dropped the
      server_conversation_metadata field; the child-agent exclusion
      test covers the surviving filter. agent_icon_tests.rs can_share
      literal dropped with the field.

      Acceptance: (1) ./script/format run twice, idempotent — identical
      23-file working-tree status across runs, format applied no
      changes of its own. (2) clippy WARNING-IDENTICAL to the HEAD
      baseline captured BEFORE editing, BOTH `cargo clippy -p warp
      --lib --all-targets` and `--no-default-features --features
      simplewarp`: 23 = 23 json location+message pairs machine-diffed
      per set, `diff` clean on both, zero new warnings (the 12
      pre-existing: 11 needless-return in app/src/terminal/input.rs +
      1 single-element-loop in terminal/model/lifecycle/mod_tests.rs:
      272; neither file touched this round). (3) `cargo check
      --no-default-features --features simplewarp --bin simplewarp`
      green. (4) `cargo check -p warp --lib --features test-util`
      green (mock_current_user rides Owner/CloudObjectPermissions::
      mock_personal, not ServerPermissions — unaffected). (5) nextest
      -p warp --lib --no-fail-fast: default 4,591 run = 4,597 baseline
      − 6 documented deletions, 4,591 passed, 3 skipped, 0 failed;
      simplewarp 4,590 = 4,596 − 6, 4,590 passed, 3 skipped, 0 failed;
      no flakes. Runtime smoke SKIPPED per standing round policy (no
      GUI tests, app not launched).

      NEXT CANDIDATES: the 4fq queue is now EMPTY — per the plan's
      post-endgame section, feature-flag rounds are the next phase.
      Residue noticed this round (all pre-existing-or-newly-exposed,
      none blocking): (a) the warp://conversation URI deep-link chain
      (uri/mod.rs UriHost::Conversation → root_view:open_conversation_
      viewer / open_cloud_conversation_in_existing_window → New
      WorkspaceSource::FromCloudConversationId, whose conversation_id
      field is now write-only) can only ever surface the "Failed to
      load conversation data" toast; (b) CloudConversationData is now a
      single-variant (Oz) enum whose match sites could collapse to the
      bare Box<AIConversation>; (c) ConversationDetailsData's copy_
      link_url field and PanelMode::Conversation's server_conversation_
      id are permanently None with their render rows still in place.

- [x] **deletion-policy reaffirmation — local features are never
      deletion targets (4fu) — RECORDED 2026-09-25.** User decision,
      plan-only round (no code changes; commit contains only this
      ledger, verified with `git show --stat`). The scope rule at the
      top of this file (2026-09-15: "Delete only what **requires a
      remote service**. A feature that works fully locally stays, even
      when its flag is constant-false in the simplewarp build") is
      REAFFIRMED as standing policy: nothing that runs locally is a
      deletion candidate in this effort, ever. Consequence for the
      queue: the "feature-flag rounds for local-but-disabled flags"
      phase is REMOVED from this cleanup's queue — EditableMarkdown-
      Mermaid, ImeMarkedText, and ITermImages are exactly what the
      scope rule always said they were: enable-in-simplewarp
      candidates, a separate per-feature product decision OUTSIDE this
      deletion effort (precedent: JupyterNotebookRendering, enabled
      2026-09-15 as its own decision). They appear here only in
      historical round records, which stay untouched. The deletion
      queue after this round consists solely of remote-service-shaped
      residue, all from 4ft's next-candidates note: (a) the warp://
      conversation deep-link chain (UriHost::Conversation → root_view
      open_conversation_viewer/open_cloud_conversation_in_existing_
      window → WorkspaceSource::FromCloudConversationId with its
      write-only conversation_id), which can only ever surface the
      "Failed to load conversation data" toast — it exists to load
      conversation data from the gone server; (c) ConversationDetails
      Data's copy_link_url field and PanelMode::Conversation's
      server_conversation_id render rows, permanently None because
      their server feeders are gone; and (b) the CloudConversation
      Data single-variant (Oz) collapse — not a feature deletion but
      dead structure exposed by the sync removal (collapsing deletes
      no local behavior; optional, lowest priority). Nothing else
      queued. Next round id: 4fv.

- [x] **permanently-None conversation link/id render residue deleted
      (4fv) — DONE 2026-09-25.** 4ft next-candidate (c), the smallest
      queued item: 3 files changed, 3 insertions(+), 105 deletions(−)
      (code only; + this ledger entry).

      DUAL-CONFIRM (both at HEAD, pre-edit). (1) ConversationDetails
      Data.copy_link_url: PCRE sweeps — lookbehind `(?<![A-Za-z0-9_])
      copy_link_url`, writer spellings `copy_link_url\s*:\s*Some` and
      `copy_link_url\s*=[^=]` and `set_/with_copy_link_url` — found
      ZERO Some-writers, ZERO assignments, ZERO setters repo-wide. The
      field is private with exactly one non-Default writer: from_
      conversation's `copy_link_url: None`; ConversationDetailsData is
      Debug+Clone+Default-derive only (no serde, no Default impl that
      could smuggle Some). Downstream: ActionButtonsConfig has exactly
      two construction forms repo-wide — for_conversation (sole caller
      the panel's action_buttons_config_from_data, passing the always-
      None field) and derive-Default; no struct literals, no field
      assignments — so ActionButtonsConfig.copy_link_url was equally
      permanently None. (2) PanelMode::Conversation.server_conversation_
      id: PanelMode is a private enum whose only constructors are
      Default (None) and from_conversation (None); the single repo-wide
      `server_conversation_id: Some(id)` hit is an `if let` PATTERN in
      the CopyConversationId handler, not a writer. Same-named hits in
      agent/telemetry.rs, blocklist controller/fetch_conversation,
      ai_document_model are fields/locals of unrelated structs —
      untouched. Premise held; no STOP condition.

      DELETIONS with caller censuses (all pre-delete). Conversation-
      DetailsData.copy_link_url (field + doc + from_conversation None
      init). ActionButtonsConfig.copy_link_url (pub field, doc,
      is_empty conjunct, for_conversation param/doc/init — for_
      conversation's sole caller rewritten to 2 args). The "Copy link
      to run" surface the field fed (its render gate `copy_link_url.
      is_some()` could never be true): ConversationActionButtonsRow.
      copy_link_button (field + construction + struct-init + render
      row + the CopyLink handler arm with its checkmark Timer),
      AgentDetailsAction::CopyLink (sole constructor the deleted
      button), AgentDetailsButtonEvent::CopyLink (sole emitter the
      deleted arm; sole handler the deleted panel match arm — which
      also contained a dead empty `if let PanelMode::Conversation`
      block), COPY_FEEDBACK_DURATION import (sole use in the deleted
      arm). PanelMode::Conversation.server_conversation_id (field +
      doc + both constructor inits). The "Conversation ID" row it fed:
      the render block (render_field_with_copy row), Conversation-
      DetailsPanelAction::CopyConversationId (sole constructor the
      deleted row; its handler arm deleted with it — terminal/view.rs's
      same-named action is a different, live type), CopyButtonKind::
      ConversationId (surviving users after the row + handler fell:
      only the mouse_state_for_copy_button arm), PanelMouseStates.
      copy_conversation_id (sole reader that arm). The mode-specific
      render match collapsed to
  `PanelMode::Conversation { directory, .. }` — still exhaustive
  (single variant, no wildcard arm).

      NEW RESIDUE FOUND (queued, not touched): PanelMode::Conversation.
      ai_conversation_id is ALSO permanently None — same two private
      constructors write None. The surviving test comment claimed it is
      "populated only by the management view path (`from_conversation_
      metadata`)" — that function no longer exists repo-wide (ghost
      since 4ft); comment corrected this round. Consequence: action_
      buttons_config_from_data bails at `ai_conversation_id.as_ref()?`
      and local_continuation_info does the same, so the panel's
      action-buttons row (Open/Fork) and the "Continue locally" button
      can never activate from ConversationDetailsData.

      TEST CHANGES: zero deletions, zero count delta. conversation_
      details_panel_tests.rs: the local-fields test's PanelMode
      destructure dropped the server_conversation_id binding and its
      is_none assertion (field gone); the ghost from_conversation_
      metadata comment replaced with an accurate one-liner.

      Acceptance: (1) ./script/format run twice, idempotent —
      formatter applied no changes either run; working tree held
      exactly the 3 code files. (2) clippy WARNING-IDENTICAL to the
      HEAD baseline captured BEFORE editing, BOTH `cargo clippy -p
      warp --lib --all-targets` and `--no-default-features --features
      simplewarp`: 12 = 12 json location+message pairs machine-diffed
      per set, `diff` clean on both, zero new warnings (the 12
      pre-existing: 11 needless-return in app/src/terminal/input.rs +
      1 single-element-loop in terminal/model/lifecycle/mod_tests.rs:
      272; neither file touched). (3) `cargo check --no-default-
      features --features simplewarp --bin simplewarp` green (exit 0).
      (4) `cargo check -p warp --lib --features test-util` green
      (exit 0). (5) nextest -p warp --lib --no-fail-fast: default
      4,591 run / 4,591 passed / 3 skipped / 0 failed; simplewarp
      4,590 / 4,590 / 3 / 0 — both byte-identical to the 4ft
      baselines, zero deltas (no tests deleted); no flakes. (6)
      DEVELOPER_DIR unset; no GUI launch, no integration suite (per
      standing round policy).

      NEXT CANDIDATES: (a) the warp://conversation URI deep-link chain
      (unchanged from 4ft: UriHost::Conversation → root_view open_
      conversation_viewer / open_cloud_conversation_in_existing_
      window → WorkspaceSource::FromCloudConversationId with its
      write-only conversation_id; can only surface the "Failed to load
      conversation data" toast); (b) CloudConversationData single-
      variant Oz collapse (optional structural refactor, not a
      feature deletion); NEW (c) the always-None ai_conversation_id
      vertical above (details-panel action-buttons row + Continue-
      locally button unreachable; includes DetailsPanelLocalContinua-
      tionInfo, continue_locally_button, and possibly the whole row
      integration — verify render reachability before deleting, since
      ConversationActionButtonsRow itself stays live for the
      management view toolbelt). Next round id: 4fw.

- [x] **warp://conversation deep-link chain deleted (4fw) — DONE
      2026-09-25.** 4ft next-candidate (a): 3 files changed, 13
      insertions(+), 109 deletions(−) (code only; + this ledger entry).

      DUAL-CONFIRM (premise verified at HEAD before editing). Entry
      census: warp:// URIs enter via lib.rs handle_incoming_uri call
      sites (OS scheme event, single-instance manager, Linux activation)
      → validate_custom_uri → UriHost::from_str → UriHost::handle. The
      UriHost::Conversation arm parsed the trailing path segment into a
      ServerConversationToken and dispatched exactly two actions:
      "root_view:open_cloud_conversation_in_existing_window" (existing
      window) or global "root_view:open_conversation_viewer" (new
      window). (1) open_conversation_viewer's sole body constructed
      NewWorkspaceSource::FromCloudConversationId { conversation_id }
      → open_new_with_workspace_source → workspace view.rs's arm called
      load_cloud_conversation_into_new_transcript_viewer, whose entire
      body (post-4ft) is report_error! + the "Failed to load
      conversation data." toast. (2) open_cloud_conversation_in_
      existing_window IGNORED its token argument (`_`) and called the
      same loader directly. conversation_id field verified write-only:
      sole construction root_view.rs:742, all three match arms bind
      `{ .. }`, no other reader repo-wide (NewWorkspaceSource is
      Clone-derive only, no serde). Every traversal of the chain ends
      at the toast; no local resolution exists anywhere in it. STOP
      check — one adjacent LOCAL path found and preserved:
      WorkspaceAction::OpenConversationTranscriptViewer (dispatched
      from local surfaces: conversation list, orchestration links,
      terminal input) focuses an existing pane for ambient agent tasks
      before its fallback; only its no-local-pane fallback could toast.

      DELETIONS with caller censuses (pre-delete). uri/mod.rs:
      UriHost::Conversation variant + FromStr arm + handle arm +
      window_behavior_hint arm-list + validate_custom_uri arbitrary-
      path entry + ServerConversationToken import (only uses). root_
      view.rs: open_conversation_viewer fn + global-action
      registration; RootView::open_cloud_conversation_in_existing_
      window + registration (no Command Palette entries existed —
      string sweep found none outside the two files); NewWorkspace-
      Source::FromCloudConversationId variant (+ its write-only
      conversation_id field) + team_uid arm entry + ServerConversation-
      Token import (only uses). workspace/view.rs: the FromCloud-
      ConversationId match arm + both cfg(vertical-tabs-panel) arm
      entries; load_cloud_conversation_into_new_transcript_viewer
      deleted — it had one surviving NON-chain caller (the OpenConver-
      sationTranscriptViewer fallback), so its 13-line toast body was
      inlined there verbatim, preserving behavior while deleting the
      misleading load-plumbing name. Post-delete PCRE sweeps: zero
      references repo-wide to any deleted symbol.

      KEPT with verdicts. find_conversation_id_by_server_token: KEEP —
      21 live call-site references (agent_conversations_model entry_
      for_server_token, blocklist block view open-conversation link,
      plus history-model tests); resolves against the LOCAL history
      model, fully locally runnable. resolve_open_action: KEEP — live
      callers in terminal input, conversation-list view (x2), orches-
      tration_conversation_links, plus tests; same local-model nature.
      OpenConversationTranscriptViewer action: KEEP — local focus_pane
      success path for ambient agent tasks. Other UriHosts (launch,
      tab_config, drive, settings, session, etc.): untouched local
      features per standing policy.

      TEST CHANGES: none — uri_tests.rs and all test files reference
      none of the deleted items; zero deletions, zero count delta.

      Acceptance: (1) ./script/format twice, idempotent (no changes
      either run; tree held exactly the 3 code files). (2) clippy
      WARNING-IDENTICAL to the pre-edit HEAD baseline on BOTH sets
      (cargo clippy -p warp --lib --all-targets, and --no-default-
      features --features simplewarp): 12 = 12 location+message pairs
      machine-diffed, diff clean both, zero new warnings (12
      pre-existing: 11 needless-return terminal/input.rs + 1 single-
      element-loop lifecycle/mod_tests.rs:272; neither file touched) —
      also the orphan sweep: no dead_code surfaced. (3) cargo check
      --no-default-features --features simplewarp --bin simplewarp
      green. (4) cargo check -p warp --lib --features test-util
      green. (5) nextest -p warp --lib --no-fail-fast: default 4,591
      run / 4,591 passed / 3 skipped / 0 failed; simplewarp 4,590 /
      4,590 / 3 / 0 — byte-identical to the 4ft/4fv baselines, zero
      deltas; no flakes. (6) DEVELOPER_DIR unset; no GUI launch, no
      integration suite.

      NEXT CANDIDATES: (b) CloudConversationData single-variant (Oz)
      collapse (unchanged); (c) 4fv's always-None ai_conversation_id
      vertical (details-panel action-buttons row + Continue-locally
      button unreachable); NEW (d) OpenConversationTranscriptViewer's
      conversation_id field is now write-only (handler binds `..`;
      both constructors feed it) and its fallback arm can only toast —
      collapses with (c) if that round takes it; also the wasm web-
      intent producer WebIntent::ConversationView (web_intent_parser
      "conversation" arm, matched in wasm-gated open_url_on_desktop /
      set_context_flags / browser_url_handler) still rewrites server-
      hosted web URLs into warp://conversation/... which now lands in
      the generic unknown-host error — wasm-only, unverifiable in this
      fork's acceptance, left for a wasm-aware round. Next round id:
      4fx.

- [x] **CloudConversationData single-variant Oz enum collapsed to bare
      Box<AIConversation> (4fx) — DONE 2026-09-25.** 4ft next-candidate
      (b), carried through 4fv/4fw: 6 files changed, 26 insertions(+),
      55 deletions(−) (code only; + this ledger entry). Pure dead-
      structure refactor — zero behavior change, no deletion target.

      DUAL-CONFIRM (premise verified at HEAD before editing). PCRE
      census with lookbehind `(?<![A-Za-z0-9_])CloudConversationData`:
      19 textual references across 6 files, ZERO in test files.
      Definition: conversation_loader.rs `pub enum CloudConversationData
      { Oz(Box<AIConversation>) }` — genuinely single-variant at HEAD,
      NO derives at all (no Clone/Debug, no serde): a pure in-memory
      carrier, so the collapse cannot alter any serialized bytes — no
      STOP condition. Census remainder: 1 re-export (history_model.rs),
      4 imports (fetch_conversation.rs, pane_group/mod.rs, load_ai_
      conversation.rs, workspace/view.rs), conversation_loader's 2
      constructions + 4 type positions (box_future return + bound,
      load_conversation_data, load_conversation_by_server_token), 1
      turbofish (fetch_conversation ActionExecution::<Option<…>>::
      InvalidAction), 5 destructures/matches (fetch_conversation map-
      closure, pane_group restoration match, load_ai_conversation ref-
      destructure + closure match, workspace fork let-else). Variant
      spellings swept separately: every other `Oz` hit repo-wide is the
      unrelated Harness::Oz; no type aliases, no `as` re-imports, no
      doc-link references.

      COLLAPSE. The payload type is Box<AIConversation> everywhere the
      enum stood: load_conversation_data / load_conversation_by_server_
      token / box_future now return BoxFuture<'static, Option<Box<
      AIConversation>>> (memory hit Some(Box::new(conversation.clone())),
      DB hit .map(Box::new)); PaneGroup::replace_loading_pane_with_
      terminal and TerminalView::restore_conversation_and_directory_
      context take the Box by value; resolve_dir_restoration_state takes
      &AIConversation (deref-coerced at the call site — a literal &Box<T>
      param would trip clippy::borrowed_box) with its destructure
      replaced by a direct initial_working_directory() call; the fork
      spawn's let-else drops the wrapper pattern and hands the Box
      straight to create_local_fork, whose Box<AIConversation> param
      predates this round and is untouched. Both unwrap-only matches
      (pane_group restoration, load_ai_conversation closure) collapsed
      to direct construction/call. fetch_conversation.rs: the closure's
      map-unbox shuffle deleted by widening materialize_conversation to
      Option<Box<AIConversation>>; the InvalidAction turbofish became
      Option<Box<AIConversation>> — SpawnableOutput is only a Send/
      Unrestricted marker alias, so Option<Box<…>> satisfies it exactly
      as the enum did. Post-collapse sweep: zero references repo-wide.

      ORPHAN VERDICTS. The enum had NO impl block, so no conversion or
      match helpers existed to fall; box_future (the only enum-adjacent
      helper) survives with its retyped signature — 3 live call sites.
      Clippy dead-code (both feature sets) surfaced nothing; call-syntax
      sweeps of every changed-signature function enumerate only live
      non-test callers: load_conversation_data x6 (agent_view, workspace
      view x4, conversation_loader self-call), load_conversation_by_
      server_token x1 (fetch_conversation), restore_conversation_and_
      directory_context x2 (agent_view, workspace view),
      replace_loading_pane_with_terminal x2 (workspace view),
      resolve_dir_restoration_state x1 (in-file).

      TEST CHANGES: none — no test file referenced the enum or any
      changed signature (census + call-syntax sweeps); zero deletions,
      zero count delta.

      Acceptance: (1) ./script/format twice, idempotent — the git-diff
      hash identical across runs; tree held exactly the 6 code files.
      (2) clippy WARNING-IDENTICAL to the pre-edit HEAD baseline
      (captured via stash before editing) on BOTH `cargo clippy -p warp
      --lib --all-targets` and `--no-default-features --features
      simplewarp`: 12 = 12 unique location+lint+message pairs machine-
      diffed per set, diff clean on both, zero new warnings (the 12
      pre-existing: 11 needless-return terminal/input.rs + 1 single-
      element-loop lifecycle/mod_tests.rs:272; neither file touched).
      (3) cargo check --no-default-features --features simplewarp --bin
      simplewarp green (exit 0). (4) cargo check -p warp --lib
      --features test-util green (exit 0). (5) nextest -p warp --lib
      --no-fail-fast: default 4,591 run / 4,591 passed / 3 skipped / 0
      failed; simplewarp 4,590 / 4,590 / 3 / 0 — byte-identical to the
      4ft–4fw baselines, zero deltas; no flakes. (6) DEVELOPER_DIR
      unset; no GUI launch, no integration suite (per standing round
      policy).

      NEXT CANDIDATES: (c) the ai_conversation_id permanently-None
      vertical (4fv): details-panel action-buttons row + Continue-
      locally button unreachable, now folded together with 4fw's
      finding that OpenConversationTranscriptViewer.conversation_id is
      write-only with a toast-only no-local-pane fallback arm — one
      round, verify render reachability before deleting (Conversation-
      ActionButtonsRow itself stays live for the management view
      toolbelt); (d) the wasm-gated WebIntent::ConversationView
      producer (web_intent_parser "conversation" arm feeding
      open_url_on_desktop / set_context_flags / browser_url_handler)
      rewrites server-hosted web URLs into warp://conversation/...
      which now lands in the generic unknown-host error — EXPLICITLY
      PARKED, not queued: wasm-only code is unverifiable in this fork's
      acceptance (no wasm target in presubmit/nextest), so touching it
      could not be validated; revisit only in a wasm-aware round. Next
      round id: 4fy.

- [x] **ai_conversation_id always-None vertical deleted (4fy) — DONE
      2026-09-25.** 4fx next-candidate (c) folded with 4fw's (d)-adjacent
      OpenConversationTranscriptViewer.conversation_id write-only field
      (the wasm WebIntent producer itself stays PARKED): 10 files changed,
      12 insertions(+), 492 deletions(−) (code only; + this ledger entry).

      DUAL-CONFIRM (premise verified at HEAD before editing). PCRE census
      with lookbehind `(?<![A-Za-z0-9_])ai_conversation_id`: 38 hits across
      11 files, censused per owning type. (1) `Block::ai_conversation_id()`
      (interaction_mode.rs:188) — a live LOCAL method (terminal block's
      in-memory conversation id) with live callers in terminal/view.rs,
      use_agent_footer, queued_query, cli_controller, context_menu, and
      local-variable bindings in conversation_list/view.rs feeding
      ForkAIConversation / ShowDeleteConfirmationDialog — LIVE, untouched.
      (2) doc comments (rich_content.rs, block_list_viewport.rs). (3) The
      target: `PanelMode::Conversation.ai_conversation_id` on Conversa-
      tionDetailsData (conversation_details_panel.rs). Writer census: both
      private constructors write None (Default + from_conversation); the
      upstream APP-3595 commit (564ea2ae5) states the field "is populated
      only by the management view path (from_conversation_metadata)" — that
      feeder died with the 4ft ServerAIConversationMetadata vertical. Zero
      Some-writers repo-wide, production AND test (the test asserted
      is_none). Reader census: action_buttons_config_from_data (bails at
      `ai_conversation_id.as_ref()?` → row config always default → row
      always empty), local_continuation_info (same bail → Continue-locally
      button never rendered), one EMPTY `if let ... Some(_conversation_id)`
      telemetry stub, the test pin. Companion field `ConversationDetails-
      Data.open_action`: also always-None (sole non-Default writer is
      from_conversation's `open_action: None`); only readers were the
      dying row config and its never-shown Open button. OpenConversation-
      TranscriptViewer.conversation_id: handler bound `..` (write-only);
      sole constructors agent_conversations_model.rs:453 (ServerToken
      subject — variant is #[allow(dead_code)], constructed only by tests)
      and :517 (resolve_entry_open_action tail) — the tail is production-
      unreachable: the sole production entry constructor (entry_for_
      conversation_parts) always sets local_conversation_id=Some and
      ambient_agent_task_id=None, and BOTH production AIConversationMetadata
      constructors set has_local_data=true, so resolve_entry_open_action
      always returns RestoreOrNavigateToConversation before the tail.

      BOUNDARY between dead server-shaped gating and live local paths.
      KEPT: the action's focus_pane success path — OpenConversation-
      TranscriptViewer variant + ambient_agent_task_id field + the
      find_pane_with_ambient_agent_conversation/focus_pane handler body
      survive verbatim (mandated by 4fw; it is the action's one live local
      success path). ForkAIConversation + fork_ai_conversation: untouched —
      many live local dispatchers (conversation list, slash commands,
      terminal view, context menu, command palette, input). Block::ai_
      conversation_id() and all its call sites: untouched. resolve_open_
      action's ServerToken arm LOCAL resolution (entry_for_server_token →
      find_conversation_id_by_server_token against the local history
      model): kept — find_... retains its live blocklist output.rs:999
      caller and history-model tests (4fw verdict preserved). Artifacts
      row, status/harness/skill/source sections, header close button,
      cancel_task_with_toast (live terminal_pane caller): kept.

      DELETIONS with caller verdicts. conversation_details_panel.rs: mode
      ai_conversation_id field (+Default+from_conversation inits), the
      always-None open_action field, DetailsPanelLocalContinuationInfo,
      local_continuation_info, ConversationDetailsPanelAction::Continue-
      Locally + handler arm, continue_locally_button + its AISettings
      subscription (existed only to re-render that button), action_buttons
      row handle + subscription + set_action_buttons + action_buttons_
      config_from_data + handle_action_buttons_event (incl. the empty
      telemetry stub — sole place reading Some(_conversation_id)), the
      header's action-buttons/continue-locally blocks (collapsed to the
      close button), show_open_button param/field (sole use fed the dying
      config; both construction sites passed false anyway) — new() now
      (initial_width, ctx), updated at terminal/view.rs + wasm_view.rs.
      Orphan cascade: agent_management/details_action_buttons.rs deleted
      ENTIRE (ConversationActionButtonsRow, ActionButtonsConfig,
      AgentDetailsButtonEvent, AgentDetailsAction — sole consumer was the
      panel; the management-view toolbelt user referenced in 4fx's note no
      longer exists at HEAD — drift confirmed by import census) + mod decl;
      agent_management_model untouched (live lib.rs/workspace users).
      workspace/action.rs: ContinueConversationLocally variant + cfg +
      should_save_app_state_on_action entry + ServerConversationToken
      import (sole use was the deleted field). workspace/view.rs: the
      ContinueConversationLocally handler arm (sole dispatcher was the
      dead panel button; its body is a thin fork_ai_conversation wrapper,
      that local capability stays reachable via ForkAIConversation's many
      dispatchers); OpenConversationTranscriptViewer arm rewritten to
      focus-only — the toast fallback arm deleted per policy (it existed
      to load conversation data from the gone server; no local traversal
      reaches it: both action constructions were production-dead as census
      above); report_error!/DismissibleToast keep their many other users.
      agent_conversations_model.rs: the ServerToken arm's or_else fallback
      (dispatched a guaranteed-toast action) and the resolve_entry_open_
      action server-token tail (would dispatch a guaranteed-toast action;
      function now returns None for cloud-only entries — unreachable in
      production, constructible only in tests). Post-delete sweeps: zero
      repo-wide references to any deleted symbol; OpenConversationTrans-
      criptViewer now has zero constructors anywhere — precedent Open-
      AgentManagementView shows unconstructed pub-enum variants don't
      dead-code-lint, and both clippy sets confirmed zero new warnings.

      TEST CHANGES: zero deletions, zero count delta. conversation_
      details_panel_tests.rs: PanelMode destructure dropped the ai_
      conversation_id binding + is_none assertion (field gone). agent_
      conversations_model_tests.rs: test_resolve_open_action_handles_
      server_token_subject_without_entry re-pinned from expecting the
      toast-bound Some(OpenConversationTranscriptViewer) to expecting
      None (unresolvable server token now has no local open action);
      WorkspaceAction import dropped (sole use was that assertion). Both
      tests verified passing by name.

      Acceptance: (1) ./script/format run twice, idempotent — git-diff
      shasum identical across runs; tree held exactly the 10 code files
      (9 modified + 1 deleted). (2) clippy WARNING-IDENTICAL to the
      pre-edit HEAD baseline (captured BEFORE editing) on BOTH `cargo
      clippy -p warp --lib --all-targets` and --no-default-features
      --features simplewarp: 12 = 12 location+lint+message pairs machine-
      diffed per set, diff clean on both, zero new warnings, no dead_code
      surfaced (12 pre-existing: 11 needless_return terminal/input.rs +
      1 single_element_loop lifecycle/mod_tests.rs:272; neither file
      touched) — verified again post-format. (3) cargo check --no-
      default-features --features simplewarp --bin simplewarp green
      (exit 0). (4) cargo check -p warp --lib --features test-util green
      (exit 0). (5) nextest -p warp --lib --no-fail-fast: default 4,591
      run / 4,591 passed / 3 skipped / 0 failed; simplewarp 4,590 / 4,590
      / 3 / 0 — byte-identical to the 4ft–4fx baselines, zero deltas (no
      tests deleted); no flakes. (6) DEVELOPER_DIR unset; no GUI launch,
      no integration suite (per standing round policy).

      NEXT CANDIDATES: the queue is otherwise EMPTY — only the PARKED
      wasm-gated WebIntent::ConversationView producer (4fx's (d))
      remains, still parked pending a wasm-aware round. NEW residue
      observed, not deletion targets per 4fu policy (locally runnable):
      AgentConversationNavigationSubject::ServerToken + entry_for_server_
      token are now test-only-reachable producers whose arm still resolves
      locally against the history model (kept; find_conversation_id_by_
      server_token has the live blocklist link caller), and OpenConver-
      sationTranscriptViewer now sits constructor-less as the mandated-kept
      focus_pane carrier. Next round id: 4fz.
