# SimpleWarp — Plan, part 4: second dead-code sweep

Survey date: 2026-10-17, HEAD `74c21c93`. Methods, all run on the simplewarp feature set unless noted:

- `cargo machete` 0.9.2 for unused dependencies (heuristic; verified per candidate by grep, plus a
  build.rs check; the compiler is the final gate).
- Lint probe: temporarily flip `allow(dead_code)` -> `warn(dead_code)` in `app/src` (49 files) and
  `crates/` (36 files), run `cargo check -p warp --no-default-features --features simplewarp --lib`,
  collect rustc's dead-code warnings, `git checkout -- app/src crates`. The probes reported 75 app
  warnings and 13 in `crates/`. Note: `pub` items in lib crates are exempt from the lint, so this
  probes private items plus the intended allowances only — cross-crate pub API needs the hand checks
  below.
- Orphan-file scan (every `.rs` file vs `mod`/`#[path]`/`include!` references): zero true orphans
  (`impl.rs` is reached via `mod r#impl;`; benches/examples/tests are Cargo targets).
- Cargo-feature scan via `cargo metadata` + `cfg(feature)` references: zero never-referenced /
  never-enabled features left (the `da6ec447` sweep already took them all).

Not probed here: `pub` cross-crate API and data shapes (the audit in plan-p3 covered these classes;
this sweep is mechanical-only), and `#[cfg(test)]`-only uses of the items below — every deletion
candidate must be checked against test and `integration_tests` references before cutting.

## 1. Unused dependencies (cargo-machete, verified)

Verified false positives to keep: `serde_regex` / `serde_bytes` (used via `#[serde(with = ...)]`,
in `cloud_object_models` and `app`), `futures-lite` in `command` (doc examples only — deleting
requires editing the doc comments first).

| Crate | Removable deps |
| --- | --- |
| `vim` | `warp_core` |
| `syntax_tree` | `log` |
| `repo_metadata` | `cfg-if`, `once_cell`, `serde`, `serde_json`, `strum`, `strum_macros`, `walkdir`, `warp_core` |
| `http_client` | `log` |
| `local_inference` | `prost`, `serde`, `tracing` |
| `virtual-fs` | `typed-path` |
| `warp_completer` | `bincode`, `cfg-if`, `dirs`, `shellexpand`, `thiserror` |
| `integration` | `backtrace`, `diesel`, `rand`, `serde`, `simplelog`, `warp-command-signatures`, `warp-workflows` |
| `warp_files` | `thiserror`, `warp_core` |
| `warp_core` | `chrono`, `concat-idents`, `dashmap`, `enum-iterator`, `http`, `reqwest`, `shellexpand`, `strum`, `tokio`, `url`, `warp_util` |
| `warp_search_core` | `strum` |
| `input_classifier` | `instant`, `warp_errors` |
| `markdown_parser` | `thiserror` |
| `cloud_object_models` | `warp_core` |
| `lsp` | `cfg-if` |
| `warp_ripgrep` | `instant` |
| `warpui_core` | `async-broadcast`, `futures-lite`, `num-traits`, `similar`, `strum`, `unicode-segmentation` |
| `warpui` | `async-io`, `derive_more`, `sum_tree` |
| `warp_terminal` | `cfg-if`, `regex` |
| `ai` | `async-trait`, `bincode`, `dunce`, `notify-debouncer-full`, `string-offset`, `tokio` |
| `cloud_object_persistence` | `anyhow`, `bincode`, `chrono`, `log`, `serde`, `warp_core` |
| `command` | `futures-lite` (doc examples) |
| `node_runtime` | `async-trait`, `serde_json`, `sha2` |
| `warpui_extras` | `async-channel` |
| `warp_cli` | `cfg-if`, `chrono`, `serde_json` |
| `editor` | `mime_guess` |
| `warp` (app) | `asset_cache`, `async-stream`, `bitflags-serde-legacy`, `blocking`, `concat-idents`, `content_inspector`, `crc`, `email_address`, `env_logger`, `getset`, `inquire`, `iso8601-duration`, `log-panics`, `lz4_flex`, `memchr`, `natural_language_detection`, `num_cpus`, `oauth2`, `os_info`, `pin-project`, `qrcode`, `reqwest-eventsource`, `rust-embed`, `rustls`, `shlex`, `static_assertions`, `thousands`, `tokio-util`, `tracing-futures`, `validator` |

Notes: none of these are used by the crate's `build.rs`; no `[features] dep:` entry references them;
`asset_cache`/`blocking` in app only appear through `warpui::assets::asset_cache` and
`command::blocking`/`reqwest::blocking` (the direct deps are unused). Ground truth is
`cargo check` after removal.

## 2. Dead module: `code/file_tree/snapshot` (~913 lines)

`app/src/code/file_tree/snapshot.rs` (370) + `snapshot/iterator.rs` (92) + `snapshot_tests.rs`
(451) + the `pub mod snapshot;` declaration in `file_tree/mod.rs`. No production consumer exists:
the only references are inside the module and its own tests. The probe backs this (`FileEntry`,
`FileTreeSnapshot`, `PathKey`, `FileEntrySummary`, `ChildEntriesIter` "never constructed").
Cascade: `sum_tree::{Edit, SumTree::edit}` may become fully unused once `snapshot.rs` goes (only
other `Edit::` use is `sum_tree`'s own internals); check before deleting from `sum_tree`.

## 3. App dead-item cluster: context chips (~25 items)

`#[allow(dead_code)]` sits on `mod context_chips;` (`app/src/lib.rs:16`), and the probe found dead
items across the module:

- `display_chip.rs`: consts `DROP_SHADOW_COLOR`, `PROMPT_CHIP_DISPLAY_ID`; fns/helpers
  `new_with_keystroke`, `with_truncate_text`, `text`, `em_width`, `render_keystroke_with_label`;
  fields `next_chip_kind`, `agent_view_controller`; enums `SelectAction`, `MenuItemType`; struct
  `MenuItem`; method `branch_name`; action variants `OpenConversationHistory` /
  `OpenCommandPaletteFiles`; widgets `ClassicPromptChipHintButton`, `UdiPromptChipHintButton`,
  `EnterAgentViewButton`.
- `renderer.rs`: consts `CORNER_RADIUS_PIXELS`, `ICON_MARGIN_RIGHT`, `LABEL_MARGIN_BOTTOM`;
  `is_disabled`.
- `display_menu.rs`: `action_data`, variant `CodeReview`, field `chip_menu_type`, `select_index`.
- `prompt.rs`: `chip`; `update` / `handle_ai_input_model_change`.
- `prompt_snapshot.rs`: `same_line_prompt_enabled`, `separator`.
- `builtins.rs`: `time12_with_seconds`, `time24_with_seconds`.
- `display.rs`: `PROMPT_CHIP_DISPLAY_ID`.
- `node_version_popup.rs`: field `main_text_color`.
- `directory_fetcher.rs`: `current_directory`, `change_directory`.

After the cut, re-check whether the module-level `#[allow(dead_code)]` can also go.

## 4. App dead items: rest (~35 items)

- `code/editor/view.rs`: `line_at_vertical_offset`; `editor/view/model/mod.rs`: `SkipUndoRedoRecord`.
- `code/editor/scroll.rs`: `OnlyHandleOnFocus`, `NeverHandle` variants.
- `code/editor/element/gutter_button.rs`: `Disabled` variant.
- `code/editor_management.rs`: `action_id` fields (2 sites).
- `code_review/telemetry_event.rs`: `Cancelled` variant.
- `terminal/view.rs`: `last_banner_id`. `terminal/input.rs`: `ai_follow_up_icon_mouse_state`.
- `terminal/view/shell_terminated_banner.rs`: `Normal` variant.
- `terminal/model/secrets.rs`: `Secret::set_is_obfuscated`, `RangeMapPoint::as_point`.
- `terminal/find/model/async_find.rs`: `unregister_rich_content_view`;
  `async_find/work_queue.rs`: `clear`, `is_empty`, `len`.
- `view_components/alert.rs`: `Success` + `success()`. `view_components/dropdown.rs`: `Naked`,
  `set_style`.
- `workspace/view/global_search/view.rs`: `get_or_create_directory_entry` ("will be used in later
  PRs" — it never was).
- `ai/mcp/mod.rs`: `items_to_hashmap`. `ai/mcp/templatable_manager.rs`: `uuid` fields (2) + tuple
  fields (4 sites).
- `ai/agent_conversations_model/entry.rs`: `ServerToken` variant.
- `ai/agent_sdk/config_file.rs`: `host` field (file key must stay accepted). `driver/terminal.rs`:
  `current_directory`.
- `ai/skills/skill_manager.rs`: `skill_watcher` field. `ai/blocklist/queued_query.rs`: `query_id`.
- `settings_view/mcp_servers/edit_page.rs`: `database_connection` field.
- `app/src/lib.rs`: `LaunchMode::add_url`.

Deliberate keeps to re-verify, not blind cuts: `terminal/view.rs:2088`
`legacy_passive_suggestions_model` (held for its event subscription —
`ViewHandle`/`ModelHandle` keep-alive), `terminal/cli_agent_sessions` fields carrying the "used for
logging and future subscribers" comment, `terminal/cli_agent_sessions/event` `transcript_path`/`v`,
`warpui_core`'s `platform/test/*` and `integration/capture_recorder` (integration-test
infrastructure, live under the `integration_tests` feature), `image_cache::evict_size`
(TODO(APP-3877) pins it).

## 5. `crates/` probe results (13)

Small; several are deliberately dead-ish:

- `warpui_core/src/ui_components/radio_buttons.rs`: `new` ("temporary constructor", the widget is
  unused — decide whether the whole widget goes).
- `warpui_core/src/platform/test/app.rs`: `new`/`run` (test platform harness — used by tests).
- `warpui_core/src/integration/capture_recorder.rs`: `captured_at`, `h264_buf`, `dimensions`,
  tuple fields (integration-testing capture).
- `warpui_core/src/core/app.rs`: `spawned_futures`. `warpui_core/src/async/mod.rs`: `future` field.
- `warpui_core/src/elements/gui/formatted_text_element.rs`: `contains`.
- `warpui_core/src/elements/gui/new_scrollable/{dual_axis_config,single_axis_config}.rs`:
  `scroll_offset`.
- `warpui_core/src/image_cache.rs`: `evict_size` (TODO).
- `warpui/src/platform/mac/rendering/metal/frame_capture.rs`: `create_capture_texture`.

## 6. Proposed rounds

| Round | Item |
| --- | --- |
| P4-1 | Remove the §1 unused dependencies (all crates); keep the verified false positives; edit `command`'s doc examples if `futures-lite` is to go there too |
| P4-2 | Delete `code/file_tree/snapshot` + module decl (+ `sum_tree::Edit` cascade if orphaned) |
| P4-3 | Delete the §3 context-chips cluster; revisit the module-level allow |
| P4-4 | Delete the §4 app items (minus the verified keeps) |
| P4-5 | `crates/` §5 small items (decide radio-buttons widget and capture/test harness items) |

Verification per round as usual: `./script/format`, clippy on the simplewarp set, `cargo check`
covering tests + `integration_tests` (every item above must be checked for test-only references
first), and the warp nextest suite.
