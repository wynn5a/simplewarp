# SimpleWarp — Plan: OSC 7501 Program Status Protocol

Spec: [Mitchell Hashimoto, "A Terminal Protocol for Program Status (OSC 7501)"](https://mitchellh.com/writing/program-status-osc7501),
full text at [superlogical.com/rex/docs/build/program-status](https://www.superlogical.com/rex/docs/build/program-status).
Published 2026-10-07; Tuist already ships it.

## Goal

Let any program running in a SimpleWarp pane report what it is doing, natively through the
terminal, and show that on the tab pill and the pane header. No plugin, no socket, no side channel:
the program writes one escape sequence.

This is an *additive* feature for the local build — nothing here depends on a remote service, so it
does not conflict with the deletion scope in [plan.md](plan.md).

## Decisions

| Topic | Decision | Date |
| --- | --- | --- |
| UI scope | **Tab pill + pane header only.** No inbox panel, no block banner, no desktop notification. | 2026-10-09 |
| OSC 9;4 | **In this round.** Bridge ConEmu progress into the root record; stop bridging once a real 7501 report arrives on that terminal. | 2026-10-09 |
| `done`/`error` dismissal | **On keypress.** A keystroke into the terminal drops them; they survive process exit until then. | 2026-10-09 |
| Record model | Per-terminal store keyed by `EntityId` of the terminal view, mirroring `CLIAgentSessionsModel`. | 2026-10-09 |
| Feature flag | New `FeatureFlag::ProgramStatusProtocol`, cargo feature `program_status_protocol`, in both `default` and `simplewarp`. | 2026-10-09 |
| State namespace | New module; do **not** reuse `CLIAgentSessionStatus`. A plugin-backed agent's rich status wins over program status; a CLI session *without* rich status shows the 7501 status as its badge. | 2026-10-09 |
| Threading | The store is a UI-thread singleton; the PTY-thread handler never touches it. **Every** mutation (report, lifetime drop, reset) travels as an `Event` → `ModelEvent` and is applied in `view.rs`. | 2026-10-09 |
| Icon | `IconWithStatusVariant::Neutral` gains `status: Option<ConversationStatus>`; no new variant. Program status is **not** an agent signal and never goes through `terminal_view_agent_icon_variant`'s `Some(_)` path. | 2026-10-09 |

## Protocol recap (what we must implement)

Wire form: `ESC ] 7501 ; <key=value pairs> ST`, pairs joined by `:`; ST = `ESC \` (0x1B 0x5C) or BEL (0x07).
Values contain no `:` or `;`, so no escaping exists. Unknown keys are ignored (forward compat);
an unknown `state` drops the whole report; a malformed report is skipped.

| Key | Type | Notes |
| --- | --- | --- |
| `state` | required | `idle`, `working`, `done`, `blocked`, `error` |
| `id` | path | `/`-separated hierarchy, `build/test` is a child of `build`; parent need not exist; absent = root record; ≤128 B total |
| `kind` | enum, only with `blocked` | `permission`, `question`, `auth` |
| `progress` | int 0–100 | only meaningful with `working`; absent = indeterminate |
| `app` | `[A-Za-z0-9_.+-]{1,32}` | stable machine-readable name |
| `title`, `msg` | base64 of UTF-8 | `msg` decoded ≤2048 B; MUST refuse a report whose decoded text contains a control character |
| `clear` | not a state | removes the addressed record **and every record beneath it** |

Semantics that drive the code:
- Each report **completely replaces** its record (no field merging).
- Records are per pseudo-terminal; hard cap **256 records**; hard cap **4096 B** for the whole sequence.
- Process exit MUST drop `working`/`blocked`; the shell prompt does the same. `done`/`error` survive.
- Full reset clears every record; soft reset does not. Switching to/from the alternate screen has no effect.
- No heartbeat requirement.
- Feature detection: a program sends the body `?`; a supporting terminal replies `OSC 7501 ; ? ST`; silence means unsupported. Terminfo: extended string `Pst=\E]7501;%p1%s\E\\`.
- OSC 9;4 says only busy, a percentage, or error. A terminal MAY map it to the root record **until** a 7501 report is seen, then stop mapping until reset.

## Where it lands today

Parser / dispatch — `app/src/terminal/model/ansi/mod.rs`:
- `Performer::osc_dispatch` at **:772**; `let writer = &mut self.writer;` already at **:773** (usable for the detection reply); empty-param guard at **:796**; unrecognized falls to `unhandled()` (**:788**) which only `debug!`s — that already satisfies "well-behaved terminals ignore unknown OSCs".
- Arms are `match params[0]` on byte strings: `b"0" | b"2"` **:803**, `b"7"` **:824** (rejoins `params[1..]` with `;` — the pattern to copy), `b"8" if FeatureFlag::OscHyperlinks.is_enabled()` **:858** (the flag-gated-arm pattern), `b"9"` **:883**, `b"133"` **:998**, `b"777"` **:1011**, `b"1337"` **:1043**, `WARP_OSC_MARKER` **:1095**.
- **OSC 9;4 is currently dropped**: `b"9"` returns early when `params[1]` is purely numeric (**:885-887**). The comment at **:874-878** already names `9;4` progress and `9;9` CWD.

Handler interface — `app/src/terminal/model/ansi/handler.rs`:
- `pluggable_notification` **:404-408** is the neighbour to add the new methods next to.
- Write-back methods already exist as precedent: `identify_terminal` **:61**, `report_xtversion` **:64**, `device_status` **:67**, implemented in `app/src/terminal/model/grid/ansi_handler.rs` **:397 / :437 / :443-456**.
- Lifetime hook facts (verified): `TerminalModel::reset_state` (`terminal_model.rs:2214`) is reached **only** from `ESC c` (RIS, `ansi/mod.rs:1530`) — a genuine full reset; soft reset (`CSI ! p`) is not implemented at all. `exit_shell` is the remote-shell DProto hook and `TerminalModel` does not override it, so it is not the local "process exit" signal. These handlers run on the PTY thread under the model lock with no `AppContext`.
- View-side signals that already exist in `model_events.rs`: `Precmd` **:359**, `AfterBlockCompleted` **:267**, `Exit` **:289**, `TerminalClear` **:287**.

Event plumbing (the path to copy) — flag-gated emit in
`app/src/terminal/model/terminal_model.rs` **:3050-3055** (`delegate!` macro at **:3073** shows the grid-delegation style) → `Event::PluggableNotification` in `app/src/terminal/event.rs` **:111-120** (Debug redaction at **:441**) → `model_events.rs` mapping **:213-215** and variant **:343-347** → consumed in `app/src/terminal/view.rs` **:9336-9370**.

Status display — the pieces we reuse, not rebuild:
- `app/src/ui_components/agent_icon.rs` **:29** `terminal_view_agent_icon_variant` is the single place that derives the icon from data sources and enforces cross-surface consistency (documented at **:21-28**). Inputs struct at **:59-71**.
- `app/src/ui_components/icon_with_status.rs` **:82 / :108 / :144** `render_icon_with_status`.
- Tab pill: `app/src/workspace/view/vertical_tabs.rs` **:3276** (variant builder), **:3289** (agent branch), **:3546** (pane-kind summary), **:1048**.
- Pane header: `app/src/terminal/view/pane_impl.rs` **:213** (`render_icon_with_status`), **:231** (`terminal_view_agent_icon_variant`).
- Per-terminal singleton pattern: `app/src/terminal/cli_agent_sessions/mod.rs` **:15-25** (`CLIAgentSessionStatus`), **:243-262** (`HashMap<EntityId, _>` + `SingletonEntity`), registered at `app/src/lib.rs` **:212 / :1165**, added in tests at `app/src/test_util/terminal.rs` **:71**.
- Keypress entry for dismissal: `app/src/terminal/view.rs` **:6700** `keydown_on_terminal`, dispatched at **:20255** (`KeyDown(chars)`); **:20256** `TypedCharacters`.
- `ProgressBar` exists at `crates/warpui_core/src/ui_components/progress_bar.rs` **:4** if a percentage ever needs a bar; base64 is already a dependency (root `Cargo.toml` **:104**, used at `grid/ansi_handler.rs` **:1178**).

Feature flag wiring: `crates/warp_features/src/lib.rs` `OscHyperlinks` **:113-115**, `PluggableNotifications` **:357-359**; `app/Cargo.toml` `default` list **:391** (`osc_hyperlinks` at **:394**), `simplewarp` list **:513** (`osc_hyperlinks` at **:516**), feature declarations `osc_hyperlinks = []` **:846**; `app/src/features.rs` **:16** `enabled_features()` with the `#[cfg(feature = …)]` mapping at **:294-297**, pinned by the test at **:318-332**.

## Design

### 7501.1 — Protocol layer (pure, no UI)

New `app/src/terminal/program_status/protocol.rs`:
- `enum ProgramState { Idle, Working, Done, Blocked, Error }`, `enum BlockedKind { Permission, Question, Auth }`.
- `struct RecordPath` — parsed from `id`, `:`-free, `/`-segmented, ≤128 B; `root()` when `id` absent; `is_descendant_of()`.
- `struct ProgramStatusReport { state, id: Option<RecordPath>, kind: Option<BlockedKind>, progress: Option<u8>, app: Option<String>, title: Option<String>, message: Option<String>, clear: bool }`.
- `fn parse(body: &[u8]) -> Option<ProgramStatusReport>` — split pairs on `:`, split each on the first `=`; ignore unknown keys; return `None` on unknown `state`, over-limit sequence, non-UTF-8, or a decoded `msg`/`title` containing a control char. Base64: try padded `STANDARD`, then `NO_PAD` (shell `base64 | tr -d '\n'` produces both shapes depending on platform).
- Limit consts from the spec: `MAX_SEQUENCE_BYTES = 4096`, `MAX_MSG_BYTES = 2048`, `MAX_ID_BYTES = 128`, `MAX_RECORDS = 256`.

Pinned parsing rules (each one gets a test; confirm the ones marked * against the spec text before coding):
- `kind` with a state other than `blocked` → field ignored*. `progress` with a state other than `working` → field ignored*; non-integer or out of 0–100 → clamp to 0–100 if numeric, ignore the field if not an integer*.
- `id`: reject empty segments (`a//b`, leading/trailing `/`)* and anything over `MAX_ID_BYTES`.
- `title` is capped like `msg` (`MAX_MSG_BYTES`), and both are refused if the decoded text contains a C0/C1 control character or a bidi control (U+202A–202E, U+2066–2069, U+200E/F).
- A `;` inside the rejoined body is a malformed report → `None` (values contain no `;`).

Exhaustive `match` only — no wildcard arms, per AGENTS.md.

### 7501.2 — Parse arm, handler methods, detection reply

- `ansi/mod.rs`: new arm `b"7501" if FeatureFlag::ProgramStatusProtocol.is_enabled()`. Rejoin `params[1..].join(&b';')` before parsing (OSC 7 precedent). Body `?` → `self.handler.program_status_query(writer)`, which writes `\x1b]7501;?\x1b\\`; otherwise parse → `self.handler.program_status(report)`. Malformed bodies are dropped silently: **never** fall through to `unhandled(params)`, which `debug!`s the raw bytes (and `msg` is untrusted, never-logged text). With the flag off the arm's guard fails and the generic fallthrough would log — add an explicit flag-off `b"7501" => ()` arm below it.
- `ansi/handler.rs`: `fn program_status(&mut self, _report: ProgramStatusReport) {}` and `fn program_status_query<W: io::Write>(&mut self, _writer: &mut W) {}`, placed with `pluggable_notification`.

### 7501.3 — Store + singleton

New `app/src/terminal/program_status/store.rs` + `mod.rs`:
- `ProgramStatusStore { records: BTreeMap<RecordPath, ProgramStatusRecord>, saw_program_status: bool }`; replace-on-report; `clear` removes the addressed record and its subtree (segment-wise `is_descendant_of`, so `clear build` never touches `buildx`); a report that would create record #257 is **rejected** — updates to existing records are still accepted. `saw_program_status` lives beside the map so deleting the last record does not reset it.
- `ProgramStatusRecord { state, kind, progress, app, title, message, updated_at }`.
- `ProgramStatusModel` singleton, `HashMap<EntityId, ProgramStatusStore>` with `Entity`/`SingletonEntity` and `Event = ProgramStatusModelEvent`; registered in `app/src/lib.rs` beside **:1165** and in `app/src/test_util/terminal.rs` beside **:71**.
- Public reads: `status(terminal_view_id) -> Option<&ProgramStatusRecord>` (root record, what the pill shows) and `records(terminal_view_id)` for future surfaces.
- Mutations: `apply_report`, `drop_running` (removes `working`/`blocked`), `drop_finished` (removes `done`/`error`), `reset`, and `remove_terminal(terminal_view_id)`, called when the terminal view closes (precedent: `CLIAgentSessionsModel::remove_session`, `cli_agent_sessions/mod.rs:324`) so the map cannot leak.

### 7501.4 — Lifetime

All mutations reach the store through events, so ordering relative to reports is the PTY channel's FIFO order and the PTY thread never needs `AppContext`.

- `Event::ProgramStatus { report }` → `ModelEvent::ProgramStatus` → `view.rs` calls `ProgramStatusModel::apply_report` for `self.view_id` and `ctx.notify()`, following **:9336-9370**. `Event`/`ModelEvent` Debug output redacts the payload (`event.rs` **:441** pattern), as does the store's `Debug`.
- Process exit / prompt drops `working`/`blocked` (`drop_running`), driven by existing view-side events, no new emitters: `ModelEvent::Precmd`, `ModelEvent::AfterBlockCompleted` (covers `command_finished`; the outer shell's precmd also covers ssh/subshells with no Warp hooks), and `ModelEvent::Exit` (PTY gone). `done`/`error` survive. Do **not** hook `exit_shell`.
- Full reset: `TerminalModel::reset_state` emits a new `Event::ProgramStatusReset` → `ModelEvent::ProgramStatusReset` → `ProgramStatusModel::reset` (clears records **and** `saw_program_status`). Only RIS reaches it; soft reset does not exist in this terminal, so there is nothing to guard. Alt-screen enter/leave does not touch the store (the handler is not routed through the grid/alt-screen delegates).
- Keystroke dismissal (`drop_finished`, `done`/`error` only; `idle`/`working`/`blocked` untouched; focused terminal only) must cover the three real input paths, because at an idle prompt typing goes to the input editor, not through the terminal view:
  1. `keydown_on_terminal` (`view.rs` **:6700**) — control/non-printable keys, both branches (long-running and not).
  2. `typed_characters_on_terminal` (`view.rs` **:6740**) — printable keys written to the PTY.
  3. The input editor's buffer-edited event as observed by the terminal view — printable keys typed at the prompt. Locate the existing subscription during implementation and drop on the first edit event.

### 7501.5 — OSC 9;4 bridge

- `b"9"` arm: a numeric `params[1]` of `4` is no longer silently returned; parse `9;4;<st>;<pr>`:
  `0` → clear the root record; `1` → `working` with `progress = pr` (clamped 0–100); `2` → `error`; `3` → `working`, indeterminate; `4` → `working` (ConEmu "paused" has no PSP equivalent — mapped to working, no progress); any other value keeps today's ignore behaviour. `9;9` and unknown numeric subcommands stay ignored exactly as now.
- Missing/non-numeric `<st>` ignores the sequence exactly as today; missing `<pr>` on `1` yields indeterminate `working`.
- The whole numeric-`4` branch sits behind `FeatureFlag::ProgramStatusProtocol`: flag off keeps today's drop-everything behaviour.
- Mapping is suppressed per terminal from the first **parsed-and-accepted** 7501 report (a `clear` counts; a malformed report does not) until a full reset — `saw_program_status` on the store (7501.3). The bridge goes through the same `Event::ProgramStatus` path, as a root-record report, so it obeys the same lifetime rules.

### 7501.6 — UI (tab pill + pane header)

- `icon_with_status.rs`: `IconWithStatusVariant::Neutral { icon, icon_color, status: Option<ConversationStatus> }`; the `Neutral` arm of `render_icon_with_status` wraps the neutral circle in `render_with_optional_status_badge` when `status` is `Some`. Existing `Neutral` constructors (`vertical_tabs.rs` **:3293–3319**, `agent_icon_tests.rs` **:54**) gain `status: None`. Check the badge overhang against a full-size neutral circle visually.
- New `program_status_badge(terminal_view_id, app) -> Option<ConversationStatus>` in `agent_icon.rs`. Mapping: `working` → `InProgress`; `blocked` → `Blocked { blocked_action: String::new() }` (the string is ignored by `status_icon_and_color`; `msg` is never put there); `done` → `Success`; `error` → `Error`; `idle` → `None`.
- `terminal_view_agent_icon_variant` keeps its contract (`Some` ⇒ agent surface) with one change: in the `CLIAgent` branch, when the session has no rich status (`!(has_listener && supports_rich_status)`), use `program_status_badge` as the `status`. A plugin-backed session's own status still wins.
- Plain terminals: `resolve_icon_with_status_variant` (`vertical_tabs.rs` **:3282**) builds `Neutral { icon: Terminal, icon_color: main_text, status: program_status_badge(..) }` in its fallback arm.
- Pane header (`pane_impl.rs` **:213–231**): the non-conversation branch calls `render_terminal_mode_indicator`, which has no status today. Pass the same `program_status_badge` through it so header and pill derive from one function.
- `app`/`msg` appear only as hover text; `msg` is untrusted: never logged, never markup. Confirm the pill and header already have a tooltip mechanism; if not, hover text is a follow-up and the badge alone ships.

## Change list

| File | Change |
| --- | --- |
| `crates/warp_features/src/lib.rs` | `ProgramStatusProtocol` variant near **:359** |
| `app/Cargo.toml` | `program_status_protocol = []` near **:846**; entry in `default` (**:394** block) and `simplewarp` (**:516** block) |
| `app/src/features.rs` | `#[cfg(feature = "program_status_protocol")]` mapping near **:296**; extend the pin test at **:318** |
| `app/src/terminal/mod.rs` | `pub mod program_status;` in the **:24-63** list |
| `app/src/terminal/program_status/{mod,protocol,store}.rs` | new; `protocol_tests.rs`, `store_tests.rs` per the `${filename}_tests.rs` convention |
| `app/src/terminal/model/ansi/mod.rs` | `b"7501"` arm; `b"9"` numeric-branch rework for `9;4` |
| `app/src/terminal/model/ansi/handler.rs` | `program_status`, `program_status_query` defaults |
| `app/src/terminal/model/terminal_model.rs` | implement both; `program_status` emits the flag-gated event; `reset_state` emits `ProgramStatusReset`. No store access from this file |
| `app/src/terminal/event.rs` | `Event::ProgramStatus { .. }`, `Event::ProgramStatusReset` near **:117**; redaction near **:441** |
| `app/src/terminal/model_events.rs` | mappings near **:213**; variants near **:344** |
| `app/src/terminal/view.rs` | consume both near **:9336**; drop `working`/`blocked` on `Precmd`/`AfterBlockCompleted`/`Exit`; `remove_terminal` on view close; keystroke dismissal in `keydown_on_terminal` **:6700**, `typed_characters_on_terminal` **:6740** and the input-editor edit subscription |
| `app/src/ui_components/icon_with_status.rs` | `status` on `Neutral`; badge wrap in the `Neutral` arm |
| `app/src/ui_components/agent_icon.rs` | `program_status_badge`; CLI-branch fallback status; tests |
| `app/src/workspace/view/vertical_tabs.rs`, `app/src/terminal/view/pane_impl.rs` | `Neutral { .. status }` call sites; plain-terminal fallback and pane-header indicator use the badge |
| `app/src/lib.rs`, `app/src/test_util/terminal.rs` | register `ProgramStatusModel` (**:1165**, **:71**) |
| `app/src/terminal/model/ansi/mod_tests.rs` | `MockHandler` **:21** gains `program_status_reports` + a `program_status_query` capture; new 7501 tests; update `parse_osc9_numeric_subcommand_ignored` **:1152** so `9;4` is no longer asserted ignored while `9;9` still is |

## Tests

- Protocol: every state; unknown state drops the report; unknown key ignored; `progress` clamp/absent → indeterminate; `id` hierarchy and 128 B cap; base64 with and without padding; decoded control char refused; 4096 B and 2048 B edges; `clear` subtree.
- Parser (`mod_tests.rs`): ST and BEL terminators both dispatch; `\x1b]7501;state=working:app=cargo:progress=40\x1b\\` yields one report; `\x1b]7501;?\x1b\\` writes exactly the acknowledgement and no report; malformed/empty body yields neither; flag off → no dispatch and no acknowledgement (pattern: hyperlink flag-off test at **:1084**).
- Store: 256-record cap (257th rejected, update of existing accepted); replace-not-merge; segment-wise subtree clear (`build` vs `buildx`); `drop_running` keeps `done`/`error`; `drop_finished` keeps the rest; `reset` clears `saw_program_status`; `remove_terminal`; `saw_program_status` survives deleting the last record and gates the 9;4 bridge (a `clear` report sets it, a malformed one does not).
- Parser: malformed and flag-off 7501 bodies produce no log output containing the payload.
- Icon: `program_status_badge` mapping table; CLI session without rich status shows the 7501 badge; plugin-backed session ignores it; plain terminal `Neutral` carries the badge. Event ordering: a report followed by `Precmd` leaves no `working` record.
- Integration (`gui-integration-test`): write a 7501 sequence to the PTY and assert `ProgramStatusModel`, then type at the prompt and assert `done` is dismissed.
- Manual, in the built app: `printf '\e]7501;state=blocked:kind=permission:app=terraform:msg=%s\e\\' "$(printf 'Apply 3 to add, 1 to change' | base64 | tr -d '\n')'`, then a keypress; `printf '\e]9;4;1;40\a'` for the bridge; `printf '\e]7501;?\e\\'` for detection.

## Verification per round

Same as [plan.md](plan.md#verification-per-round): `./script/format` idempotent; the three
`script/presubmit` clippy runs with `-D warnings`; `cargo check` for the default and `simplewarp`
builds clean; `cargo nextest run -p warp --lib` in both configs. Then `cargo run` and check the pill
and pane header for real — a type check does not prove the escape sequence renders.

## Out of scope

Inbox/aggregate panel over all terminals; block-level banners; desktop notifications for `blocked`;
shell function + `Pst` terminfo entry shipped in `script/`; rate limiting beyond the hard caps;
surfacing child records (`id`) anywhere other than the store.

## Risks

- **Keystroke dismissal spans three input paths.** Missing any one makes `done`/`error` stick (editor path) or vanish early (PTY path). It only drops `done`/`error`, only for the focused terminal.
- **Two status systems can disagree.** A CLI-agent plugin and a 7501 report can both claim one pane. The CLI-branch rule in `agent_icon.rs` (rich status wins, else 7501) is the single tie-break and must stay single-sourced.
- **Untrusted text in UI and logs.** `msg`/`title` are attacker-controlled bytes from any process writing to the PTY; control/bidi refusal, no-logging (including the `unhandled()` fallthrough) and Debug redaction are the whole defence.
- **vte feature unification.** The workspace pins vte with `default-features = false`, so `osc_raw` is an unbounded `Vec` and our 4096 B cap is the only limit. If any crate in the graph enables vte's `no_std` feature the buffer becomes 1024 B and long `msg` values truncate silently into a still-parseable report; check `cargo tree -e features -i vte` once.
- **Hookless sessions.** With no Warp hooks (e.g. a nested shell), nothing but the outer shell's precmd clears a stale `working`; there is no heartbeat in the protocol.
