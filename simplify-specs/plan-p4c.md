# SimpleWarp — Plan, part 4c: post-P4b fixpoint survey (2026-10-19)

Companion to `plan-p4b.md` (closed 2026-10-18). HEAD `d0192422b`, working tree clean.
**This is a survey — no code was changed** (until round P4c-1 was executed; see §7). Scope: the
crates-side half of the P4-9d fixpoint sweep (never done; P4-9d covered `app/src` only), a fresh
`allow(dead_code)` pass, a fresh `cargo machete` run, and the residue items P4b/P4-9 left open.
Method/repro in §6.

## 0. Totals

| Item | Count | Notes |
| --- | --- | --- |
| Definition-line-only `pub fn` candidates | 106 | ≈870 lines excl. the 217-line proc-macro entry; 12 flagged `#[cfg]`-gated (test/platform) |
| `allow(dead_code)` sites | 105 | 40 multiline platform/test `cfg_attr`; ~8 macro/comment false positives; ~55 plain |
| `cargo machete` rows | 12 | all macro/attribute false-positive classes except app `rust-embed` (tied to the dead `release_bundle` feature) |
| Cargo features enabled by no build path | 2 | `release_bundle` (16 cfg sites), `standalone` (3 cfg sites) |
| Orphan script pair | 1 | `script/patch_font_with_warp_glyph` + `script/warp.svg` (referenced only by each other) |
| Open `todo!()`/`unimplemented!()` (non-test) | 7 sites / 6 files | PowerShell + WSL bootstrap, banner actions, debug-only paths, an infinite-width bound — no new action proposed |

The 106 candidates are functions whose name occurs nowhere else in any `.rs` file under
`app/` + `crates/` (12,040 `pub fn` defs scanned); non-Rust files (resources, scripts, docs,
CI) were searched too and hold no references (one unrelated Python `output_dir` hit). Names
are also checked against the `concat_idents!`/`paste!` generator sites (settings macros emit
`register_events_for_*` and test names; none of the candidates match those shapes).

## 1. Fixpoint candidates

### 1.1 `crates/warpui_core` — 49 (328L)

- elements/gui: `container.rs:221 with_overdraw_left` (4); `drag/draggable.rs:56
  cursor_offset_within_element` (9), `:296 with_drag_threshold` (4); `event_handler.rs:185
  on_mouse_out` (7); `hoverable.rs:325 on_back_click` (7), `:333 on_forward_click` (7),
  `:373 with_reset_cursor_after_click` (4); `mod.rs:382 with_vertical` (5), `:388
  with_horizontal` (5), `:489 with_horizontal_border_gradient` (9), `:666 try_rect` (3);
  `new_scrollable/mod.rs:1302 with_remap_cross_axis_wheel_to_main_axis` (4); `stack/mod.rs:114
  with_event_dispatch_mode` (4); `table/mod.rs:465 invalidate_intrinsic_width` (6), `:476
  invalidate_all_intrinsic_widths` (6); `clipped_scrollable.rs:236 vertical_centered` (19)
- core: `core/app.rs:232 last_active_timestamp` (3); `core/mod.rs:664 has_pending_retries` (3);
  `core/model/context.rs:165 unsubscribe_from_view` (40)
- text/fonts: `fonts.rs:81 matches_custom_weight` (6); `text/words.rs:15
  split_at_next_word_start` (16); `text_layout.rs:129 remove_line` (3), `:133
  remove_text_frame` (3), `:702 hyperlink_bounds` (39)
- integration: `integration/overlay.rs:111 get_overlay_log` (3);
  `integration/video_recorder.rs:374 output_dir` (7)
- keymap: `keymap.rs:714 with_standard_action` (4), `:840 is_shift_tab` (3);
  `keymap/matcher.rs:71 set_keymap` (4, test-gated)
- misc: `accessibility.rs:193 with_frame` (4); `image_cache.rs:453 should_retain_aspect_ratio`
  (6); `notification.rs:52 with_data` (4); `platform/file_picker.rs:141 with_default_filename`
  (4); `presenter.rs:529 clear_highlighted_view` (3); `time.rs` whole module —
  `get_current_time` ×2 (`cfg(not(test))` + `cfg(test)`) + `test_offset_time`, zero callers of
  either (14)
- ui_components: `components.rs:104 set_position` (4), `:120 set_border_style` (4);
  `link.rs:125 with_hovered_style` (6), `:132 with_clicked_style` (6); `radio_buttons.rs:62
  rich_element` (3), `:442 with_button_diameter` (4); `slider.rs:151,156,161,166,171 with_snap/
  thumb/track builders` (20)
- util: `util.rs:12 save_as_file` (9)

### 1.2 `app/src` — 21 (195L)

- `ai/blocklist/controller.rs:105 new_with_session_type_for_test` (7, gated), `:1157
  get_ambient_agent_task_id` (3, gated); `ai/llms.rs:614 add_agent_mode_model_for_test` (3,
  gated), `:779 is_available_agent_mode_llm` (3, gated); `global_resource_handles.rs:79
  set_model_event_sender_for_test` (3, gated)
- `context_chips/prompt.rs:409 remove_chip` (3), `:413 add_chip_to_end` (3)
- `integration_testing/agent_mode/assertions.rs:22 assert_latest_exchange_text` (18), `:113
  assert_latest_task_is_done` (42)
- `notebooks/editor/model.rs:165 new_unbound` (3), `:268 set_file_link_resolution_context` (6),
  `:1542 any_notebook_command_submodel_focused` (5)
- `pane_group/mod.rs:2248 add_terminal_pane_ignoring_default_session_mode` (18), `:3959
  add_pane_sibling` (16), `:4247 original_session_if_swapped` (9); `pane_group/pane/view/
  header/mod.rs:256 is_overlay_open` (3, gated)
- `terminal/event_listener/testing.rs:33 with_pty_bytes_read_tx` (7)
- `util/git.rs:746 get_diff_for_pr` (25), `:773 get_branch_commit_messages` (11)
- `view_components/dismissible_toast.rs:214 has_toasts` (3), `:348 with_keystroke` (4)

Note: P4-9d's app-side re-scan reported 0 remaining; every entry above was re-verified as
zero-ref today. Six are `#[cfg]`-gated helpers (outside that sweep's ungated shape); the rest
are leftovers of the same shape that the sweep did not surface.

### 1.3 `crates/warp_core` — 17 (126L)

- `ui/builder.rs:420 custom_progress_bar` (7), `:924 retry_button` (13), `:938
  left_chevron_button` (13), `:1007 info_button_with_tooltip` (14), `:1022
  help_icon_with_tooltip` (15), `:1078 span_with_tooltip` (24)
- `ui/theme/color.rs:189 active_highlighted_text_color` (3), `:262 green_overlay_1` (4), `:267
  green_overlay_2` (4), `:397 ansi_bg_magenta` (3), `:405 ansi_bg_yellow` (3), `:438
  accent_hover` (5), `:550 fg_overlay_7` (3), `:554 accent_bg_strong` (4), `:570 accent_fg` (5)
- `execution_mode.rs:51 can_fetch_agent_runs_for_management` (3); `ui/appearance.rs:244
  set_monospace_font_size_test` (3, gated)

### 1.4 `crates/editor` — 11 (119L)

- `content/buffer.rs:645 set_embedded_item_conversion` (6), `:661 set_line_ending_mode` (3),
  `:1093 previous_block_marker_of_type` (35), `:1131 next_block_marker_of_type` (39), `:1278
  color_at_offset` (4)
- `content/selection_model.rs:85 selections_mut` (3); `multiline.rs:340
  normalize_to_linefeed` (3)
- `render/model/mod.rs:1365 chars_zero` (3), `:2695 set_render_mermaid_diagrams` (8), `:5438
  new_with_cursor_bias` (11)
- `test_data/test_rust_file.rs:4325 into_unique` (4) — fixture *content* (a fake `Box` impl);
  trimming it edits a test asset, not product code.

### 1.5 Smaller crates

- `crates/warpui`: `platform/app.rs:66 enable_headless_microphone_access_query` (5),
  `platform/linux/mod.rs:40 user_windowing_system` (14), `platform/mod.rs:30
  create_system_clipboard` (13) — winit/Linux paths; not compiled on macOS, so deletion is
  text-verified only.
- `crates/warp_completer`: `completer/context/mod.rs:135 for_windows` (6), `meta.rs:42
  for_char` (6).
- `crates/warp_cli`: `lib.rs:160 app_args` (3).
- `crates/integration`: `src/test/history.rs:259
  test_command_search_loads_history_from_nondefault_histfile_path` (52) — `bin/integration.rs`
  registers only `test_command_search_loads_history` (line 305) and its sibling tests; this
  52-line test is unregistered and never runs.
- `crates/settings_value_derive`: `lib.rs:39 derive_settings_value` (217) — **KEEP**, the
  proc-macro entry point (scanner false positive).

### 1.6 Caveats

- `#[cfg]`-gated candidates (12) need their gate checked before deletion (`cfg(test)` helpers
  can go; platform gates may need the round's compile runs twice).
- `crates/editor/test_data/test_rust_file.rs` is fixture content; leave it out of any purge
  unless the fixture is being rewritten anyway.
- Winit/Linux files (warpui platform, winit backends) are not compiled by the macOS dev loop;
  their zero-ref status is text evidence only.

## 2. `allow(dead_code)` survey — 105 sites

Breakdown: 40 sites are `cfg_attr(...platform/test..., allow(dead_code))` (the attribute is
often split across lines, so a naive grep over-counts "plain" ones); ~8 are macro-generated
items (`settings/src/macros.rs` ×4), test-schema internals (`schema_tests.rs` ×4) or
comment/fixture text; the rest are plain annotations, including documented keeps.

Already-checked actionables:

- `app/src/ai/agent_sdk/driver/terminal.rs:357 current_directory` — still present, zero
  callers (P4b §6(b) listed it; the annotation hides it from the lint).
- `app/src/ai/mcp/mod.rs:119 items_to_hashmap` — **alive** (callers at `mcp/mod.rs:277,282`);
  the annotation is stale → remove.
- `app/src/ai/blocklist/action_model/execute/call_mcp_tool.rs:18 terminal_view_id` — written
  in the ctor, never read; the sole ctor caller is `execute.rs:234` → field + ctor param
  deletable together.
- `crates/persistence/src/model.rs:441-458 PaneBranch` / `PaneNode` — the structs themselves
  have no queries anywhere (only their defs and `NewPane*` insert structs); their unread
  fields carry the annotations → decide: delete the row structs or keep.
- `crates/virtual_fs/src/lib.rs:23 Stub::FileWithContent` — constructed in app tests; the
  enum is `pub` at a lib root, so the annotation is not doing anything → remove.
- `crates/warp_terminal/src/model/grid/cell.rs:344 flags_mut` — callers exist
  (`grid/flat_storage/row_iterator.rs:148,153`, plus the app grid) → annotation looks stale;
  verify by removal (the `TODO(visibility)` comment above it stays valid).
- `crates/warpui_core/src/core/app.rs:672 spawned_futures` — keep (holds spawned futures
  alive); annotation is load-bearing.
- `crates/warpui/src/windowing/winit/app.rs:57 AppEvent::Clipboard` + `:97 ClipboardEvent` —
  never constructed; the only handler (`event_loop/mod.rs:1670`) is fed solely by that
  variant. Winit-only path (not compiled on macOS) → decision: cut or keep for the platform
  backends.
- `crates/warpui/src/windowing/winit/fonts.rs:617 try_read_face_source`,
  `fonts/font_handle.rs:95 data()` (file-level `cfg_attr` with a "unused on macOS/Windows"
  comment) — zero callers under the module's gates → decision (winit-only).
- `crates/warp_terminal/src/model/escape_sequences.rs:18 mod C0` — module-level annotation;
  `ESC`/`DLE` are used, most constants are not (alacritty control-char vocabulary) →
  decision: trim the module or keep the annotation.
- `app/src/ai/ambient_agents/mod.rs:59 AmbientConversationStatus::Cancelled` — constructed in
  `terminal_status_from_last_exchange` (line ~133) → annotation suspicious; verify by removal.
- `app/src/code/editor/element/gutter_button.rs:116 CommentButton` — all four variants are
  constructed (`element.rs:1104/1106/1120`, `default()` at `view.rs:475`) → annotation looks
  stale; verify by removal (P4b §8 kept it as a MEDIUM vocabulary item).
- `app/src/code/editor/view.rs:116 DiffHunkContextAdded.line_range` — constructed
  (`view/actions.rs:1056`) and read (`code_review_view.rs:5352`) → annotation suspicious;
  verify by removal.
- Documented/test-infra keeps (leave alone): `warp_features set_enabled`, `warpify
  new_with_defaults`, `outline new_for_test`, `theme_chooser themes`, editor
  `layout_mermaid_block_for_test`, `block.rs background_executor` (read under `cfg(not(test))`),
  the settings macro sites, `image_cache evict_size` (TODO APP-3877), the winit Linux window
  manager TODO (CORE-3034), `platform/menu.rs` Mac-only reads, `TestDevice::Metal`.
- Handle-lifetime keeps: `terminal/view.rs` legacy suggestions model, `terminal_manager` pty
  poller, `skill_manager` watcher, MCP edit page `database_connection`, `config_file host`
  (serde tombstone).

Round method (fixpoint, same spirit as P4-9d): delete the plain annotations in one commit
(list recorded above) and let the compiler classify every new warning; dead → cut with
cascades, macro/derived/serde → restore with a comment, other-platform → convert to
`cfg_attr`. The 40 `cfg_attr` sites are only touched where the other-platform caller is
provably gone.

## 3. Residue items

- **Orphan tool pair**: `script/patch_font_with_warp_glyph` — a font-patching utility for the
  bundled Warp glyph; zero references anywhere (build, CI, docs). Its only companion,
  `script/warp.svg`, is referenced only by the script itself. Decision: delete the pair, or
  keep as a maintenance tool with a README note.
- **`cargo machete` (v0.9.2) re-run — 12 rows, all false-positive classes**:
  `http_client/log` (macro: `report_error!` expands to `log::log!` in the caller — live at
  `http_client/src/lib.rs:175`); `cloud_object_models/serde_regex` and `app/serde_regex`,
  `app/serde_bytes` (`#[serde(with = "…")]` attribute paths); `warp_core/{strum,
  concat-idents}` and `warpui_core/{strum, num-traits}` (derive/macro expansions; warp_core
  uses `define_settings_group!` which expands an unqualified `concat_idents::concat_idents!`
  in the caller); `app/asset_cache` (re-export path `warpui::assets::asset_cache`);
  `app/concat-idents` (39 `define_settings_group!` uses). The only genuine question is
  `app/rust-embed` below.
- **`release_bundle` is enabled by no build path.** The feature (`app/Cargo.toml:550` +
  `warp_core:10`, 16 `cfg` sites: `bin/simplewarp.rs` windows subsystem, `lib.rs` ×2,
  `profiling.rs`, `app_services/linux` ×4 — D-Bus single-instance arg forwarding,
  `app_services/windows` ×7, `channel/state.rs:64`) is set by nothing: `script/run` uses
  `--features simplewarp`, `script/bundle_simplewarp` passes none, and `flake.nix` defines
  `buildFeatures = ["release_bundle"]` that **nothing consumes** (`cargoBuildFlags` has no
  `--features`; `inherit buildFeatures;` on `buildRustPackage` is an inert attr). App default
  features are `["simplewarp"]`, so every shipped build runs with the flag off. With it off,
  `restart`/single-instance forwarding on Linux never happens and `RELEASE_FLAGS` are never
  applied. Decision: wire the flake (`cargoBuildFlags … ++ [ "--features" "release_bundle" ]`)
  or delete the feature + its 16 sites + `app/rust-embed` (which exists only to flip
  `rust-embed/debug-embed` for `warp_assets` when the feature is on).
- **`standalone` is enabled by no build path.** `app/Cargo.toml:557` →
  `warp_assets/standalone` (`exclude = "async/**"`), `warp_core/standalone`
  (`paths.rs:383` unbundled-binary resource path), `lib.rs` CLI-detection read; the comment
  references a bundle script that no longer exists. Decision: delete or keep for a future
  standalone CLI.
- **`MenuItemFields.timestamp`** (`app/src/menu.rs:362`): every constructor writes `None`
  (`:415`, `:445`, `:478`, `:512`, `:542`), the only read copies it (`:587`), and the render
  path at ~`:1099` (`if let Some(timestamp)`) is therefore never taken (P4-9d follow-up).
  Deletion chain: field, the copy, and whichever render helpers are only reachable from the
  `Some` branch.
- **`wayne5a-progress.md`** at the repo root — personal log; P4-8/P4-9 deliberately left it.
  Decision: delete or move out of the repo root.
- **`todo!()`/`unimplemented!()`** (non-test): `terminal/bootstrap.rs:294` (PowerShell
  bootstrap, PLAT-750), `terminal/local_tty/shell.rs:739` (WSL), `pane_group/mod.rs:3575`
  (banner actions), `terminal/view.rs:15846` (debug-only), `editor/view/element.rs:1491`
  (infinite width), `editor/view/model/buffer/text.rs:215,242` (`Bound::Excluded`),
  `integration_testing/terminal/util.rs:30,34,37` (test infra). No new action: these are
  existing, recorded product gaps, not dead code.
- **Stale knife found while sweeping**: `crates/settings/src/macros.rs` still exports
  `concat_idents` for callers (line 177) and expands an unqualified `concat_idents::` macro
  (line 716) — the reason app/warp_core must keep the dep; worth a comment, not a change.

## 4. Proposed rounds

| Round | Content | Size |
| --- | --- | --- |
| P4c-1 | `crates/warpui_core` candidates (mostly UI builder leftovers) | 49 items / 328L |
| P4c-2 | `crates/warp_core` + `crates/editor` + `crates/warpui` + `crates/warp_completer` + `crates/warp_cli` | 34 / ~292L |
| P4c-3 | `app/src` (incl. the unregistered integration test) + cascade checks | 22 / ~247L |
| P4c-4 | `allow(dead_code)` fixpoint: annotations out, compiler classifies, dead items cut | ~55 plain sites |
| P4c-5 | Decisions applied: `release_bundle`/`standalone`, script+svg, `MenuItemFields.timestamp`, winit clipboard/font items, `C0`, `CommentButton`, `PaneBranch`/`PaneNode` | user rulings |

Per round: `./script/format`, the three presubmit clippy gates (`-p warp`, workspace excl.
`warp_completer`, `-p warp_completer`), `cargo nextest run --workspace`; for gated items also
`cargo check --features integration_tests` (P4-9d precedent). The compiler surfaces cascades
after each cut; the sizings above are brace-matched bodies before cascades.

## 5. Decisions needed before P4c-5

1. `release_bundle` — wire it into the flake, or delete the feature + 16 sites + `app/rust-embed`?
2. `standalone` — delete, or keep for a future standalone CLI?
3. `script/patch_font_with_warp_glyph` + `script/warp.svg` — delete or keep?
4. `wayne5a-progress.md` — delete or move?
5. Winit-only dead paths (`AppEvent::Clipboard`/`ClipboardEvent`, `try_read_face_source`,
   `font_handle::data`, `create_system_clipboard`, `user_windowing_system`) — cut or keep the
   platform backends untouched?
6. Vocabulary keeps: `CommentButton`, `C0`, `PaneBranch`/`PaneNode` — trim or keep?

## 6. Method / repro

- Candidate scan: every `.rs` under `app/` + `crates/`, name tokens counted repo-wide
  (definition lines included); a function is a candidate iff its name occurs nowhere else.
  `#[test]`-attributed functions are excluded; `#[cfg]` gates, `#[proc_macro*]` lookback and
  enclosing `impl` recorded per item; bodies brace-matched for sizing. Non-Rust files
  (`resources/`, `script/`, `docs/`, `.github/`) searched for every candidate name — no refs.
- `allow(dead_code)` scan: every site with the next item line and the attribute shape
  (`cfg_attr` vs plain) extracted; macro/test/comment sites classified by inspection; the
  §2 list re-checked by hand.
- Dependency scan: `cargo machete` v0.9.2; each row verified by grep against the macro/derive
  expansion explanation.
- Feature scans: `grep -rn -- "--features|buildFeatures|standalone|release_bundle"` across
  `script/`, `.github/`, `flake.nix`, `app/Cargo.toml`.
- Spot re-verification this round: `context_chips/prompt.rs`, `util/git.rs`,
  `warpui_core/time.rs`, `warpui/src/platform/linux/mod.rs`, `agent_sdk/driver/terminal.rs`,
  `ai/mcp/mod.rs`, `integration/src/test/history.rs`, `menu.rs`.

## 7. Execution log

### P4c-1 — `crates/warpui_core` candidates (all 49 items)

All §1.1 items were re-verified zero-ref at HEAD and deleted: 31 files, +2/−462. Every deleted name
was re-grepped afterwards (0 hits, incl. both `get_current_time` definitions and `test_offset_time`;
the `time` module and its `pub mod time;` declaration are gone).

Compiler-classified cascades:

- `text_layout.rs` `TextCache::remove` — its only callers were the deleted `remove_line`/`remove_text_frame`.
- `video_recorder.rs` `PathBuf` import trimmed (only user was `output_dir`).

Direct orphans of the deleted items (zero-ref and unconstructible/unused afterwards; cut in the same
commit and re-verified):

- `radio_buttons` `Label::Rich` + the `RichLabelFn` type + its render arm — the deleted `rich_element`
  was the variant's only constructor. The arm's `_` fallback became an explicit `Label::CustomItem(_)`
  (the enum's only other variant; `custom_item` has a live app caller).
- `fonts` `CustomWeightConversion` trait + its `CustomWeight` impl and `Weight::is_normal` — the
  deleted `matches_custom_weight` was the only caller of both.

Kept with evidence:

- `VIDEO_DIR_ENV_VAR` (`integration/video_recorder.rs:25`) is zero-ref now that `output_dir` is gone,
  but it is the documented recording-pipeline knob
  (`.warp/skills/gui-integration-test-video/SKILL.md`, "`WARP_INTEGRATION_TEST_VIDEO_DIR`").
- `ui_components/components.rs` `set_position`/`set_border_style` had zero callers, but the fields stay
  live: app struct literals still write `border_style: Some(..)`
  (`coding_entrypoints/glowing_editor.rs:136`, `ai/blocklist/summarization_cancel_dialog.rs:77`), so no
  field cascade applies.

New always-None follow-ups (fields whose only setter was deleted; compiler-invisible because the fields
are still read). Behavior-preserving cuts for a later round — not taken here:

- `event_handler` `mouse_out` (the two `dispatch_callback` reads in `MouseMoved`/`LeftMouseDragged`).
- `hoverable` `back_click_handler`/`forward_click_handler` (`BackMouseDown`/`ForwardMouseDown` arms),
  `reset_cursor_after_click` (click path).
- `container` `Overdraw.left` (bounding-rect math reads; `Overdraw` is only built via `Default`).
- `clipped_scrollable` `fill_min_main_axis` (main-axis constraint path).
- `new_scrollable` `remap_cross_axis_wheel_to_main_axis` (cross-axis remap in `handle_event`).
- `slider` `snap_values` (+ the snap-resolution path) and `thumb_size`/`thumb_fill`/`track_fill`/
  `track_height` (now always the `DEFAULT_*` constants).
- `radio_buttons` `button_diameter_override` (diameter computation reads it).

Verification: `./script/format --check`, the three presubmit clippy gates
(`--workspace --exclude warp_completer`, `-p warp`, `-p warp_completer`, all `-D warnings`),
`cargo check -p warp --features integration_tests --all-targets`, and `cargo nextest run --workspace`
(6340 run, 6338 passed, 87 skipped). Both nextest failures are pre-existing and unrelated:
`warp::terminal::view::open_in_warp::tests::test_single_code_file` references `app/src/bin/oss.rs`,
deleted at HEAD by `9b875bd3d`, and the `integration` clone-slash-command test flakes only under
full-workspace parallel load (its post-clone step times out waiting for the next command block);
run in isolation on this diff it passes (2/2 runs, ~5.6 s). `./script/check_no_inline_test_modules`
fails at HEAD for two reasons unrelated to this round: the local `.cargo-home/` vendored registry
(pruned only for `.git`/`target`; `.cargo-home` is in `.git/info/exclude`, so CI never sees it) and
the pre-existing inline `#[cfg(test)] mod tests` at `app/src/features.rs:307`.

Committed on master as `3c75ce38a` (code, 31 files) plus this log entry; not pushed.

### P4c-2 — `warp_core` + `editor` + `warpui` + `warp_completer` + `warp_cli` candidates (all 33 items + 1 unregistered test)

All §1.3–§1.5 items were re-verified zero-ref at HEAD (`3c75ce38a`) and deleted, plus the
unregistered `crates/integration` test from §1.5: 15 files, +7/−456. `crates/editor/test_data/
test_rust_file.rs` `into_unique` was excluded per §1.6. Three names required disambiguation before
deletion: `retry_button` (a `retry_button_mouse_state` field elsewhere), `for_char` (a same-name
test fn) and `app_args` (an unrelated `into_app_args` caller) are all definition-line-only for the
candidate itself.

Compiler-classified cascades:

- `warp_core/ui/builder.rs`: `icon_with_tooltip` (private helper) lost all three callers
  (`info_button_with_tooltip`, `help_icon_with_tooltip`, `span_with_tooltip`) → deleted; the
  four path constants only used by the deleted methods also went (`INFO_SVG_PATH`,
  `LEFT_CHEVRON_PATH`, `HELP_SVG_PATH`, `RETRY_SVG_PATH` — note `bundled/svg/retry.svg` remains
  as an asset). Imports trimmed: `ConstrainedBox`, `vec2f`, `platform::Cursor`.
- `editor/content/buffer.rs`: `color_count_at_offset` lost its only caller (`color_at_offset`) →
  deleted; `SyntaxColorId` import trimmed. (`ColorU` stays: three non-related uses.)
- `warpui/platform/headless/app.rs`: `enable_microphone_access_query` lost its only caller
  (`enable_headless_microphone_access_query`) → deleted; reachability of the `AppBackend::Headless`
  arm in that public method wrapper no longer applies.
- `warpui/platform/linux/mod.rs`: `windowing::WindowingSystem` import trimmed (only the deleted
  `user_windowing_system` named the type; `windowing::System` matching stays).

New always-None follow-ups (compiler-invisible because the fields are still read):

- `headless/delegate.rs` `query_microphone_access` is now only ever `false` (its only setter was
  the deleted `enable_microphone_access_query`), so `microphone_access_state` always returns
  `Denied` and its macOS query branch is unreachable; the ctor parameter (`app.rs:48`) can drop
  with it. Behavior-preserving cut for a later round.

Verification: `./script/format --check`, the three presubmit clippy gates (all `-D warnings`),
`cargo check -p warp --features integration_tests --all-targets`, `cargo check --workspace
--all-targets` (covered the workspace's `integration_tests` feature unification), and
`cargo nextest run --workspace` (6340 run, 6338 passed, 87 skipped). The two failures are the
same pre-existing pair as P4c-1: `warp::terminal::view::open_in_warp::tests::test_single_code_file`
(`app/src/bin/oss.rs` deleted at HEAD) and the integration clone-slash-command test under
full-workspace parallel load (passes in isolation, 1/1 in ~5.5 s here, same as the P4c-1 record).

