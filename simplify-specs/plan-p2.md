# SimpleWarp — Plan, part 2: what still depends on a remote service

Survey date: 2026-09-29, HEAD `67c4e70e6` (after 4id). Read-only scan of `app/` and `crates/`
(tests, `crates/integration`, `integration_testing` and comments skipped), plus shipped assets.
Build under test: `--no-default-features --features simplewarp` (`app/Cargo.toml:531`).

"Depends on a remote service" means one of:

1. **Server-owned** — the Warp server used to do the work; the client still sends the request,
   and the local adapter (`crates/local_inference`) ignores it. No traffic, but the feature is
   broken.
2. **Automatic third-party traffic** — the app reaches a host other than the user's AI provider
   without an explicit user action.
3. **User-initiated third-party traffic** — works only online, triggered by a click or command.
4. **Dead remote shell** — UI or code whose data used to come from the server and is now always
   empty.

The goal (plan.md) allows exactly one kind of traffic: the AI provider the user configures. User
content (their MCP servers, their git remote, their links) is theirs to point anywhere.

## Headline

- **No code path contacts a Warp host.** No AIClient, AuthClient, ServerApi client, GraphQL,
  websocket, telemetry, crash reporter or updater remains. `mcp_static_config` is `None` in
  every bin. `reqwest` is built directly only in `http_client`, `local_inference`, `mcp` and
  `asset_cache`; the only listeners are loopback (`http_server` 127.0.0.1:9277 profiling,
  MCP OAuth callback).
- **The largest remaining gap is category 1**: much of the agent UI still emits request types
  and context that only the Warp server understood (§1).
- **Three sources of automatic non-provider traffic** remain (§2).

---

## 1. Server-owned agent behavior the local adapter drops

### 1a. Inputs other than a plain user query are dropped

**Mostly done (P2 slice 2, 2026-10-08).** `local_inference::inputs` carries the prompts the server
used to supply, and `convert.rs` expands each input into a user turn (and again when the stored
form comes back in the task history). Done: `SummarizeConversation` (`/compact`; the reply is a
`Summarization` message that is appended while it streams and finished with a duration, and a
finished summary replaces everything before it on replay), `InvokeSkill`, `InitProjectRules`,
`ResumeConversation`, `CodeReview`, `CreateNewProject`, `CloneRepository`, and
`QueryWithCannedResponse` (the chip's own text; the canned reply is dropped, the model answers).
The emitter stores each as the message the client already ignores when it draws blocks
(`SystemQuery`, `InvokeSkill`, `CodeReview`; `/init` is stored as a question). Not done on
purpose: `TriggerPassiveSuggestion` / `AutoCodeDiffQuery` (stopped by P3), `CreateEnvironment`
(remote concept, P4). **Not verified against a live model or in the app** — the tests are unit
tests over the proto shapes; a real `/compact`, `/init` and skill run still need a session with a
provider key.

Original finding:

`crates/local_inference/src/convert.rs:140` (`push_input`) reads only `UserInputs`
(UserQuery / ToolCallResult) and the deprecated UserQuery / ToolCallResult; everything else hits
`_ => {}`. A new conversation then fails with `Error::NoInput` (`stream.rs:23`); an existing one
replays history with no new instruction, on the user's key.

| Dropped input | Entry point (visible in simplewarp) | Built at |
| --- | --- | --- |
| `SummarizeConversation` | `/compact`, `/compact-and`, `/fork-and-compact` | `ai/blocklist/controller/slash_command.rs:274` |
| `InvokeSkill` | `/<skill>`, `/skills` | `slash_command.rs:293` |
| `InitProjectRules` | `/init` | `slash_command.rs:248`, `terminal/view.rs:10244,10282` |
| `CreateEnvironment` | `/init` "create an environment / run cloud agents" step | `terminal/view/init_project/model.rs:149`, `mod.rs:704` |
| `CreateNewProject` | `/create-new-project`, "Create new project" button | `slash_command.rs:240`, `coding_entrypoints/project_buttons.rs:224` |
| `CloneRepository` | clone-repo entry point | `terminal/input.rs:10452`, `slash_command.rs:243` |
| `CodeReview` | "Send diff comments to Agent" | `terminal/view.rs:17475` |
| `ResumeConversation` | AIResumeButton | `ai/blocklist/controller.rs:1329` |
| `AutoCodeDiffQuery` | passive code diffs | `controller.rs:1401` |
| `TriggerPassiveSuggestion` | ~~unit-test suggestion after every successful `git commit` (**automatic**, setting on by default)~~ — stopped by P3 | `controller.rs:1433`, `passive_suggestions/legacy.rs:459` |
| `QueryWithCannedResponse` | zero-state chips Install / Code / Deploy / Something else | `terminal/view/inline_banner/prompt_suggestions.rs:75` |

### 1b. Attached context never reaches the model

**Partly done (P2 slice 1, 2026-10-08).** `local_inference::context` now renders the request
context: the environment (cwd, home, OS, shell, time, git repo/branch/PR, codebases), project
rules and the skills list go into the system prompt on every request (so a replayed history gets
them again); terminal blocks, files, selected text, diffs and the query's referenced attachments
join the user's turn. The user's own rules (Knowledge page) are read from `CloudModel` in
`RequestParams::new` and handed over as a project-rules entry with no root path. Still open:
images (providers are called with text only, so the model is told it cannot see them), the
`LspServersContext`, and `rules_enabled` / per-conversation attachments in stored history.

Original finding:

`convert.rs` reads `task_context` only; `request.input.context` and `referenced_attachments` are
never read, and `prompt.rs:16` is a static system prompt. Lost: attached blocks, files, images,
selections, diff sets, conversations, rules / AGENTS.md, cwd / OS / shell, skills list, MCP
context (all built in `app/src/ai/agent/api/impl.rs:59-138`). Affected flags (all on): AIRules,
ImageAsContext, SelectionAsContext, DiffSetAsContext, ConversationsAsContext,
AgentViewBlockContext, AIContextMenu*, ListSkills, BundledSkills, MCPGroupedServerContext.

### 1c. Only 7 tools are offered

`crates/local_inference/src/tools.rs:14` `SUPPORTED` + `wire_name` (`:34`): run_shell_command,
read_files, apply_file_diffs, grep, file_glob, read_shell_command_output,
write_to_long_running_shell_command. Everything else the client advertises is filtered out.
UI that the model therefore can never trigger:

- web search / web fetch (WebSearchUI, WebFetchUI, profile "Call web tools" toggle at
  `settings_view/execution_profile_view.rs:360`) — these were executed server-side
- SearchCodebase, AskUserQuestion, TransferControlTool, ReadSkill, InsertReviewComments
  (PRCommentsV2), create/edit/read documents, SuggestPrompt
- MCP tool calls (servers start and connect, the agent cannot call them) — Phase 3b
- **computer use** (LocalComputerUse, AgentModeComputerUse, BackgroundComputerUse)

### 1d. Server-authored output is never produced

The adapter emits text, tool calls and a default `StreamFinished` (`emit.rs:205`) — no usage,
cost, suggestions or citations. Always empty/zero:

- credits: `/cost` (`search/slash_command_menu/static_commands/commands.rs:416`),
  `ai/blocklist/usage/conversation_usage_view.rs:209`, "Credits used" in
  `ai/conversation_details_panel.rs:748`, "requires AI credits" tooltip in
  `code_review/comment_list_view.rs:939`
- ContextWindowUsageV2 / Breakdown
- SuggestedRules chips (`ai/blocklist/block.rs:2183`) and the Knowledge page "rule suggestions"
  toggle (`settings_view/knowledge_page.rs:43,156`)
- `AIAgentCitation::WarpDocumentation` → `https://docs.warp.dev/{path}` (`terminal/view.rs:15282`),
  reachable only from old restored conversations

### 1e. Other server-owned AI features

**Settings cleanup (2026-10-08).** Deleted, each with its setting/flag/plumbing: custom model
routers (settings section, editor pane, YAML loader, picker entries, request field, flag, cargo
feature) and the feature-intro popover that only announced them; AI commit/PR generation (the
toggle, setting and the dialog's autogen path); Suggested Rules (toggle, footer chips, modal,
event chain, setting, flag); the profile "Call web tools" rows (the `web_search_enabled` field and
the rendering of old web messages stay); agent attribution; the web-only "Open links in desktop
app" setting; and seven settings with no readers. Then the Codex modal and `simplewarp://codex`
deeplink (and `preferred_codex_model_id`), the remote codebase search stub (a remote session now
answers `CodebaseNotIndexed` directly), and the legacy Warp AI panel (panel, transcript, requests,
the right-panel slot, toolbar button, warm welcome, focus navigation and its integration test).
`ai_assistant/` keeps only `AskAIType` and the execution-context types, which the agent still uses.

**P13/P14 cleanup (2026-10-08).** Deleted: the MCP gallery (`MCPGalleryManager`, gallery cards, the
"Shared from Warp" section, `ServerCardItemId::GalleryMCP`, `MCPServerUpdate::Gallery`, the
`?autoinstall=` deeplink and the install-origin gating that only existed for it); the "Get Figma
MCP" install chip (the "Enable Figma MCP" chip and the bundled Figma skills stay, because the
status comes from a user-added `mcp.figma.com` server, not from the gallery); `WellKnownMcpIds`
(flag, `MCPSpec::WellKnown`, CLI and config-file handling); the web-font fallback in `warpui_core`
(`ExternalFontFamily`, `FallbackFontModel`, the fetch-and-redraw path and the app's URL provider);
the zero-reference flags `SoloUserByok`, `KnowledgeSidebar`, `MultiWorkspace`,
`DefaultAdeberryTheme`, `ForceLogin` and their cargo features; the unread `BlockOnboarding`
experiment layer; and `AIWorkflowOrigin::LegacyWarpAI`. Kept on purpose: `TipAction::WarpAI`
(its values are stored in old users' defaults and must not be reused) and the persisted
`GalleryData` field on installed MCP templates.

- **Custom model routers** (CustomModelRouters, on): "+ Add router"
  (`settings_view/warp_agent_page.rs:757`) and a feature-intro modal
  (`workspace/view/feature_intro_modal/view.rs:76`) are live; `custom_model_routers` is sent but
  never read (`local_inference/src/config.rs` uses only `custom_model_providers`, `:86`), and
  `auto` router ids are rejected (`config.rs:70`).
- **AI commit message / PR generation**: `code_review/git_actions.rs:68` always bails
  ("unavailable in this build"), but the "Commit & Pull Request Generation" toggle is still shown
  (`warp_agent_page.rs:340,2081`).
- **Codex modal / deeplink** (`uri/mod.rs:266` → `workspace/view/codex_modal.rs`): "Use Codex"
  always errors "No preferred codex model found" (`preferred_codex_model_id: None`,
  `ai/llms.rs:489`, `workspace/view.rs:13155`; the id came from the server's model list). Copy at
  `codex_modal.rs:139` still promises "agent session sharing".
- **Legacy Warp AI panel**: `ai_assistant/requests.rs:35` always answers "technical
  difficulties". Unreachable (AgentMode always on).
- **Remote codebase search**: `ai/get_relevant_files/remote_search/native.rs` always returns
  `CodebaseNotIndexed`. Unreachable (tool not offered, 1c).

## 2. Automatic non-provider traffic (no explicit user action)

| # | What | Where | Destination | Trigger |
| --- | --- | --- | --- | --- |
| 2a | GitHub PR info polling via `gh repo view` / `gh pr view` | `code_review/github_repo_model/model.rs:19,134-151` (60 s timer), `util/git.rs:672,694,747` | api.github.com via the user's `gh` | on creation, on branch change and every 60 s, whenever the PR chip shows (`github_pr_prompt_chip` on) **or** AI input is active in a git repo (`terminal/view.rs:3790` `needs_pr_info_for_agent_context`); silent no-op without `gh` |
| 2b | Completion generators run during autosuggestion validation | `terminal/input.rs:7230` → `ai/predict/next_command_model.rs:666-720` (`is_command_valid`, 150 ms timeout, ValidateAutosuggestions on) | whatever the spec's generator runs — the pinned `warpdotdev/command-signatures` specs include `curl registry.npmjs.org` (npm/yarn/pnpm/bun), cdn.deno.land, package.elm-lang.org, api.sdkman.io, api.github.com, `gh pr list`, `aws`/`gcloud`/`heroku` list commands | while typing, for history-predicted commands; also on Tab |
| 2c | Remote images in markdown / notebooks | `crates/editor/src/content/edit.rs:80,866` → `crates/asset_cache/src/lib.rs:148` (`reqwest::get`, bypasses `http_client`) | any host named in the document | opening a markdown file or `.ipynb`; AI blocklist refuses http images (`ai/blocklist/block/view_impl/common.rs:2339`) |
| ~~2d~~ | Passive unit-test suggestion (**stopped by P3**) | §1a `TriggerPassiveSuggestion` | the user's AI provider (allowed host, but unrequested spend, and the input is dropped) | every successful `git commit` |
| 2e | Global MCP servers auto-start | `ai/mcp/file_based_manager.rs:328-374` | user-configured; `npx -y` / `uvx` stdio servers download packages | app launch, for `~/.warp/.mcp.json`; third-party global configs only if `file_based_mcp_enabled` (default false) |

2e is user-configured and allowed by the goal; listed for completeness.

## 3. User-initiated third-party traffic (works only online)

| What | Where | Destination |
| --- | --- | --- |
| LSP auto-install (rust-analyzer, clangd, gopls, pyright, typescript-language-server) | trigger: `ai/persisted_workspace.rs:597-628` from `code/local_code_editor.rs:1533`, `code_review/code_review_view.rs:863`, `/init` (`init_project/mod.rs:808`); code: `crates/lsp/src/install.rs:11,50,220`, `servers/{rust,clangd,go,pyright,typescript_language_server}.rs` | api.github.com releases + asset downloads; `go install …gopls@latest` (proxy.golang.org); registry.npmjs.org + `npm install` |
| Node download for the npm-based servers | `crates/node_runtime/src/lib.rs:122,158-215,491` | nodejs.org/dist, registry.npmjs.org |
| Code review git ops | `util/git.rs:610` (`git push`), `:916` (`gh pr create`); `code_review/diff_state/model.rs:1337` (`git fetch origin <branch>` when the base is missing) | the repo's remote / GitHub |
| Docker sandbox | `terminal/view/docker_sandbox/mod.rs:24,88` (`sbx`, image `None`) | Docker registry (via sbx) |
| External secrets picker | `external_secrets/mod.rs:19-44` (`op item list`, `lpass ls`) | 1Password / LastPass |
| MCP over SSE/HTTP + OAuth | `crates/mcp/src/runtime.rs`, `sse_transport/*`, `oauth.rs:297,330` | user-configured server and its issuer |

`ServerApiProvider` (`app/src/server/server_api.rs:131`) now only hands out the shared
`http_client`, used solely by the LSP install path; its doc comment at `:153` still mentions
"standard Warp request headers".

## 4. Dead remote shells visible in the UI

| Surface | Why it is dead | Where |
| --- | --- | --- |
| MCP gallery cards in Settings | `MCPGalleryManager::new` starts empty, nothing fills it (gallery came from the server) | `ai/mcp/gallery.rs:95-114`, `settings_view/mcp_servers/list_page.rs:380` |
| `simplewarp://settings/mcp?autoinstall=` deeplink | always "Unknown MCP server" | `settings_view/mcp_servers_page.rs:320-328` |
| FigmaDetection "Add Figma MCP" button | `install_figma_from_gallery` only logs "Could not find Figma MCP server in gallery" | `terminal/input.rs:12297`, `agent_message_bar.rs:249`, `ai/mcp/templatable_manager/native.rs:1130`; bundled `resources/bundled/mcp_skills/figma/*` assume it |
| WellKnownMcpIds (on) | `agent run --mcp linear` etc. is accepted then always skipped; uninstalled UUID specs hard-fail | `warp_cli/src/mcp.rs:88`, `ai/agent_sdk/config_file.rs:116`, `ai/agent_sdk/driver.rs:628-645` |
| Custom model routers, credits/usage, SuggestedRules, AI commit/PR toggle, Codex modal | §1d, §1e | — |
| Web-font fallback provider | `set_fallback_font_fn` never called; no `ExternalFontFamily` built (was the wasm path) | `app/src/lib.rs:963`, `crates/warpui_core/src/fonts/external_fallback.rs:15` |
| `SoloUserByok`, `KnowledgeSidebar`, `MultiWorkspace`, `DefaultAdeberryTheme`, `ForceLogin` flags | zero references | `crates/warp_features/src/lib.rs` |

## 5. Stale remote-shaped copy and links

**Done (P15, 2026-09-29).** Tips: `@` context copy reworded, `/open-mcp-servers` no longer says
"share with your team", `/create-environment` and `/usage` tips removed, `oz` tip now points at
`simplewarp agent run`. The `/init` cloud-environment step is gone (step kind, block UI, the
`warp environment create` block-completion hook); `AIAgentInput::CreateEnvironment` and the
`/create-environment` request plumbing stay for P4. Conversation-list empty state no longer says
"ambient agents". The PS1 "Look incorrect? Let us know." link is removed. Palette entry is now
"Toggle workflows modal". `warp_cli`: `oz model list` / MAA / "Warp Agent" help strings reworded;
`--skill` was hidden from help (the arg is now deleted, §6). `QueuedPromptsV2`
doc fixed. The "inert strings" bullet is unchanged by design.

- Agent tips (`ai/agent_tips.rs`): `:96` "Warp Drive objects", `:146` "share MCP servers with
  your team", `:151` `/create-environment` "remote docker environment" (no such command),
  `:186` `/usage` "AI credits" (no such command), `:191` "`oz` command".
- `/init` offers the cloud-environment step (§1a).
- Conversation list empty state mentions "ambient agents"
  (`workspace/view/conversation_list/view.rs:762`).
- Onboarding PS1 block "Let us know." → `github.com/warpdotdev/Warp/issues/new`
  (`terminal/view/block_onboarding/onboarding_prompt_block.rs:231`).
- Command palette "Toggle team workflows modal" (`terminal/view/init.rs:394`) — local modal,
  stale label.
- `warp_cli`: `ModelArgs` help "use `oz model list`" (`crates/warp_cli/src/model.rs:~26`),
  "Warp Agent"/MAA doc strings (`agent.rs:111`); `--skill` is listed in `--help` but rejected at
  runtime (`ai/agent_sdk/mod.rs:105,336`).
- `QueuedPromptsV2` comment still says "Cloud Mode setup".
- Inert strings (no action needed): `releases.warp.dev` as Linux secure-storage key bytes
  (`crates/warpui_extras/src/secure_storage/linux.rs:109`), `*.firebaseapp.com` secret-redaction
  regex, `firebase_uid` schema columns, `Software\Warp.dev\` registry path, `dev.warp.WarpOss`
  id in the `oss` bin, Flex debug panics linking notion.so/warpdev.

## 6. Compiled out of simplewarp

**Done (2026-09-29).**

- **AgentHarness**: `agent_harness` is now in the `simplewarp` feature set, so `agent run
  --harness` is accepted. The harness list holds only Oz (`harness_availability.rs`), so the GUI
  picker stays hidden. Note that `--harness claude` still runs `setup_harness_plugins`
  (`driver.rs`), which shells out to `claude plugin marketplace add warpdotdev/claude-code-warp`
  (GitHub, via the claude CLI); it is CLI-initiated only.
- **OzPlatformSkills**: flag, cargo feature, the `--skill` CLI arg, `resolve_skill_spec.rs`
  (including the `git clone`), `SetupStep::SkillRepoClone` and `AgentDriverError::SkillResolutionFailed`
  are deleted. `warp_cli::skill::SkillSpec` stays (the conversation details panel still parses
  `skill_spec` from task metadata).
- **CloudRunners**: flag and cargo feature deleted, along with `runner_id` in the agent config
  file schema and `AgentConfigSnapshot`.
- **Voice input**: `crates/voice_input`, the `voice_input` cargo feature, the editor mic button
  and cursor icon, the CLI-agent footer voice flow, the `agents.voice.*` settings, the Voice
  settings section and the voice quota fields in `request_usage_model` are deleted (`gui` stays
  as an empty feature because the bundle scripts pass it). `AgentToolbarItemKind::VoiceInput` stays
  as a never-rendered variant so saved custom toolbar layouts still parse. Left alone: the
  microphone plist/entitlement strings (generic permission for child processes) and the warpui
  `microphone_access_state` API (no callers; cross-platform framework code).
- **Dogfood-only server hints**: `SummarizationViaMessageReplacement` (the request field is now a
  constant `false`), `GPTConfigurableContextWindow` (OpenAI models never expose a configurable
  context window, so the long-context pricing warning and `warning_box` are gone),
  `AgentModeAnalytics` (the `is_ai_ugc_telemetry_enabled` plumbing through the terminal model is
  gone) and `SuggestedAgentModeWorkflows` (the workflow suggestion chip, its modal and the event
  chain through terminal view, pane group and workspace are gone) are deleted.

## 7. Local bug found during the scan

**Fixed (P1, 2026-10-08).** `trash_object`, `untrash_object` and `delete_object_with_initiated_by`
in `app/src/server/cloud_objects/update_manager.rs` returned early unless the id was a
`ServerId`, so locally created objects (always `SyncId::ClientId`) could never be trashed,
untrashed or deleted. Reproduced with two unit tests that fail on the old code. The three
methods now work from the `SyncId` (`sqlite_uid_hash` for the sqlite key) and the
`ObjectOperationResult` carries `client_id` or `server_id` as appropriate; the old
`ServerId::from_string_lossy("Client-...")` would have panicked in debug builds. The
notebook `Trash`/`Untrash` handler in `active_notebook_data.rs` no longer `expect`s a server id.
`new_local` leaving `content_sync_status = InFlight(1)` is harmless: the trash/delete guards
read only the pending-change flags.

## 8. Corrections to plan.md

- Status note: "`RequestComputerUse`/`UseComputer` tools are advertised" is wrong —
  `local_inference/src/tools.rs` filters them out (§1c).
- Known non-targets: "`AgentHarness` flag (live by design)" — it was compiled out of simplewarp;
  it is now enabled (§6).

---

## Proposed queue

Per the deletion-scope decision, a feature is deleted only if it **requires** a remote service.
Server-owned features that can be done locally (§1) are *implement* candidates; which ones to do
is a product decision. Every item below needs a user decision before it becomes a round.

| ID | Item | Options | Recommendation |
| --- | --- | --- | --- |
| ~~P1~~ | §7 ClientId trash/delete | fix | **Done** (see §7). |
| P2 | **Slices 1–2 done (see §1a, §1b).** §1a/§1b core inputs: `SummarizeConversation`, `InvokeSkill`, `InitProjectRules`, `ResumeConversation`, `CodeReview`, `QueryWithCannedResponse`, attached context + rules | implement in `local_inference` / hide | Implement — they are local work the server merely orchestrated; highest user value. |
| ~~P3~~ | §1a passive requests: `TriggerPassiveSuggestion`, `AutoCodeDiffQuery` | stop sending | **Done (2026-10-08).** `AISettings::is_code_suggestions_enabled` is now constant `false`, which stops both senders in `passive_suggestions/legacy.rs` and the view-side "hide the banner while a diff is generated" path; the "Suggested Code Banners" settings section and its palette toggle are removed. The stored setting key stays so old configs load. Turn it back on together with P2. |
| P4 | §1a `CreateEnvironment`, `CreateNewProject`, `CloneRepository` | implement / hide | Drop the cloud-environment `/init` step (remote concept); implement or hide the other two. |
| P5 | §1c tools: MCP tool calls, computer use, AskUserQuestion, ReadSkill, SearchCodebase, InsertReviewComments, documents | implement / hide UI | MCP is Phase 3b; others per feature. |
| P6 | §1c web search / web fetch | hide UI / implement web fetch locally | Web search needs a hosted search API → hide; web fetch can be local (would be new provider-external traffic — decide). |
| P7 | §1d credits/usage, SuggestedRules, WarpDocumentation citation | derive usage from provider response / hide | Map provider token usage into `StreamFinished`; hide credits copy and SuggestedRules. |
| P8 | §1e custom model routers, AI commit/PR toggle, Codex modal, legacy AI panel, remote codebase search stub | delete / hide / implement | **Done (2026-10-08).** Routers, commit/PR toggle, Codex modal, legacy AI panel and remote search stub are deleted. |
| ~~P9~~ | §2a `gh` polling | keep / make on-demand / setting | **Done (2026-10-08).** Only a visible PR chip subscribes; the agent-context trigger is gone. |
| ~~P10~~ | §2b network completion generators | keep / skip generators in validation / setting | **Done (2026-10-08).** A generator arg is assumed valid; generators run only on Tab. The fallback completion that proposes an autosuggestion still runs them when history has no match. |
| ~~P11~~ | §2c remote markdown images | keep / click-to-load / setting | **Done (2026-10-08), but refused rather than click-to-load**: the editor's image renderer is a custom painter with no click handling, so remote `http(s)` images simply do not load, as in the AI blocklist. `asset_cache` lost `url_source`, the persistence path and `reqwest`; it only decodes `data:` URIs now. Click-to-load can be added later. |
| P12 | §3 LSP install downloads | keep (user-initiated) | Keep; optionally say in the Install UI that it downloads from GitHub/npm. |
| ~~P13~~ | §4 MCP gallery, autoinstall deeplink, FigmaDetection + figma skills, WellKnownMcpIds | delete | **Done (2026-10-08)** except the Figma detection/skills, which work from a user-added server and stay. |
| ~~P14~~ | §4 web-font fallback, zero-reference flags | delete | **Done (2026-10-08).** |
| ~~P15~~ | §5 stale copy/links, `--skill` in help | fix | **Done** (see §5). |
| P16 | §8 plan.md corrections | edit | Fold in with the next round. |

## 9. Local-only cleanup round (2026-10-08)

P9–P11 above, plus:

- **`app/src/experiments/`** deleted. Its one layer put 100% of users in the Tantivy palette-search
  arm, so `use_tantivy_search` is now a cargo feature in `default` and `simplewarp` (pinned by
  `features::tests`). The unused `ServerExperiment` diesel structs went too; the
  `server_experiments` table stays in the schema.
- **Computer use**: the mac/linux/windows backends, the `use_computer` bin and the native deps
  (~7,000 lines) are gone, and (round 4, below) so is everything else: `crates/computer_use`, the
  executors, the action/result types and block cards, the profile `computer_use` /
  `computer_use_model` rows, the `--computer-use` flags, `computer_use_enabled` in agent configs, the
  per-model preference and the three flags.
- **Upstream baggage**: `specs/`, `agents/specs`, `.agents/specs`, the `.github` issue/PR
  templates, triage config, dependabot and three bot workflows, ten agent skills, seven
  `.warp/workflows`. (Round 4 rewrote `README.md`, `CONTRIBUTING.md`, `FAQ.md` and `SECURITY.md`
  for simplewarp and dropped `images/`.)
- Verification: workspace nextest 6474 / 6485; the 11 failures are `integration` GUI tests that
  fail the same way on b79567e7c (8 settings/palette tests time out, 3 report "test not found").

### 9.1 Cloud-object sync residue (2026-10-08, second pass)

The cloud-object layer is now only the local store behind saved workflows, AI rules, MCP servers,
plan documents (notebooks), env var collections and folders. Removed:

- `CloudObjectSyncStatus` / `NumInFlightRequests` / `CloudObjectStatuses`
  (`pending_changes_statuses`), `ConflictStatus`, `should_update_after_server_conflict`, the
  sync-state icon and the in-flight counters. `update_manager` trash/untrash/delete no longer wait
  on "pending online-only" flags.
- `SyncId` is a newtype over `ClientId`; the `ServerId` arm, `has_server_id`, `is_on_server`,
  `ToServerId`, `ServerIdAndType` and `ObjectOperationResult.{client,server}_id` are gone
  (`ObjectOperationResult.id` instead). `ObjectOperation::Update` had no emitter, so its handlers
  went too (they `expect`ed a server id and would have panicked).
- Offline gating: trash/duplicate/rule-edit were hidden while `NetworkStatus` was offline.
- **Behaviour fixes this exposed**: the notebook and env-var header menus returned empty for any
  object that was not `is_on_server()` (all of them), the rule delete button required
  `has_server_id()` (never true), and embedded-workflow lookups parsed `Workflow-<server id>` hashes
  (never matched a client id). All three work now.
- **Old data**: rows that only have a `server_id` (and `SyncId` strings without the `Client-`
  prefix) are no longer loaded or deserialized. The `is_pending` and `server_id` columns stay as
  tombstones. `restored_notebooks`/`restored_workflows` fixtures were rewritten to client ids; the
  json-preference and shared-metadata-id restore tests (and their fixtures) were deleted, as was the
  workflow-pane palette test whose palette action no longer exists.

### 9.2 Sharing / team / welcome residue (2026-10-08, third pass)

Removed: `Owner::Team`, `CloudLinkSharing`, `CloudObjectGuest`, `ServerObjectContainer`,
`drive/sharing.rs` (`SharingAccessLevel`, `Subject`), and the `guests` / `anyone_with_link` /
`permissions_last_updated_ts` fields (`CloudObjectPermissions` is now just the owner; the sqlite
columns stay NULL). `CloudPreference` / `Preference` / `Platform` and `JsonObjectType::Preference`
are gone, as are `is_welcome_object` and `OperationSuccessType`. Rows owned by a team, or holding a
`PREFERENCE` object, are no longer loaded. `ModelEvent::UpdateMultiAgentConversation` now boxes its
`AgentConversationData`: shrinking the cloud metadata made it the odd one out for
`clippy::large_enum_variant`.

Not done, same family: the `ServerId`-backed `NotebookId` / `WorkflowId` / `FolderId` /
`GenericStringObjectId` marker types (also the plan-artifact `notebook_uid`, which only a
server-provided artifact could fill), and the other dead `JsonObjectType`s (`MCPServer`,
`CloudEnvironment`, `ScheduledAmbientAgent`, `CloudAgentConfig`, `AIExecutionProfile`).

Still open from the local-only survey: Windows/Linux platform code (user decision: **keep**).

## 10. Dead tool UIs (2026-10-09, round 4)

The local agent offers seven tools (`local_inference/src/tools.rs`), so every other tool's UI was
unreachable. Removed, each with its executor, inline view, speedbump/permission, flag and the code
that only it used:

- **Computer use** (see §9) incl. `StartRecording`/`StopRecording`.
- **Web search / web fetch**: the inline views, `WebSearchStatus`/`WebFetchStatus`, `WebSearchUI` /
  `WebFetchUI`, the profile `web_search_enabled` field and the request param.
- **AskUserQuestion**: executor, `AskUserQuestionView`, the session state machine in `crates/ai`, the
  number-shortcut/numbered-button/compact-input widgets, the speedbump footer and setting, the
  profile "Ask questions" permission, flag.
- **ReadSkill** and **SearchCodebase**: executors, cards, the codebase-search speedbump, the
  `GetRelevantFilesController` plumbing through `TerminalView`, `SearchCodebaseUI`.

- **Documents / plans** (round 5, 2026-10-09): `ReadDocuments`/`EditDocuments`/`CreateDocuments`
  and the whole plan subsystem they fed: `AIDocumentModel`, `AIDocumentView` and its pane
  (`IPaneType::AIDocument`, `LeafContents::AIDocument`, the deferred-pane restore pass), the
  `terminal/input/plans` menu and `InputSuggestionsMode::PlanMenu`, the plan chip in the todo chip,
  the "view plan" hint, `cmdorctrl-alt-p`, the "Execute this plan" suggestion, plan attachments
  (`<plan:id>`, the pending-document context), `AttachPlanAsContext`, the dirty orchestration events
  that rode on the document model, `crates/ai` `DocumentContext`/`AIDocumentVersion`. Old pane rows
  of kind `ai_document` fail to restore (like legacy MCP panes) and are dropped; the table stays in
  the schema. Notebooks keep their `ai_document_id` field (and the "Plan" label/icon) as data.
  `AIAgentAttachment::DocumentContent` stays because it is a persisted shape.

Saved conversations: calls and results of these tools restore as nothing (`NoClientRepresentation`
for calls, `None` for results). Persisted `PersistedAIAgentActionType` rows that hold one still
deserialize through empty tombstone variants (`UseComputer {}`, `RequestComputerUse {}`,
`GetRelevantFiles {}`, `AskUserQuestion {}`) and never restore. Old profile/config files that carry
the removed keys still load (profile files ignore unknown keys); `computer_use_enabled` in an agent
config file is now rejected (that file type denies unknown keys).

Verification: workspace nextest 6385 / 6395 after the documents round (6407 / 6417 before it); the
failing 10 are `integration::integration` GUI tests that time out the same way without these
changes (one of them flips between runs).

**Not done**, because each is tied to something still live: `InsertReviewComments` (the
`code_review/comments` import path and its flag), `TransferShellCommandControlToUser` (the
long-running-command controls in `shell_command.rs`/`cli_controller.rs`), `SuggestPrompt` (passive
suggestions), and the orchestration family (`RunAgents`, `SendMessageToAgent`, `WaitForEvents`,
`FetchConversation`, `OrchestrationConfigUpdate`, with the profile "run agents" permission). The todo
list (`UpdateTodos`, the todo chip and popup) is also unreachable, since no offered tool creates one.
MCP tool calls stay
(Phase 3b). Dogfood-only skills under `resources/channel-gated-skills` still mention computer use.
