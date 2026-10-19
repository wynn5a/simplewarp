---
id: architecture
type: architecture-design
title: SimpleWarp — Architecture
parent: simplewarp
status: active
---

# SimpleWarp — Architecture

## Topology

One GUI desktop app over a Cargo workspace (~62 packages). The app crate (`app/`, package `warp`,
bin `simplewarp`) is the product shell; everything else is a shared library with no product logic.

Layers, top to bottom (spec'd boundaries link by id):

| Layer | Crates | Spec |
| --- | --- | --- |
| Product shell | `app` | [`app`](../../app/SPEC.md) |
| UI framework (MIT) | `warpui`, `warpui_core`, `warpui_extras`, `ui_components` | [`ui-framework`](../../crates/warpui/SPEC.md) |
| Terminal core | `warp_terminal` | [`terminal-core`](../../crates/warp_terminal/SPEC.md) |
| AI domain | `ai`, `local_inference` | [`ai-core`](../../crates/ai/SPEC.md), [`local-inference`](../../crates/local_inference/SPEC.md) |
| Local object store | `cloud_objects`, `cloud_object_models`, `cloud_object_persistence`, `persistence` | [`object-store`](../../crates/cloud_objects/SPEC.md) |
| Settings | `settings`, `settings_value`, `settings_value_derive` | [`settings`](../../crates/settings/SPEC.md) |
| Long-tail shared libs | `editor`, `command`, `sum_tree`, `languages`, `syntax_tree`, `markdown_parser`, `ipynb_parser`, `lsp`, `mcp`, `node_runtime`, `vim`, `fuzzy_match`, `warp_search_core`, `warp_ripgrep`, `warp_completer`, `input_classifier`, `virtual-fs`, `watcher`, `http_client`, `ipc`, `jsonrpc`, `isolation_platform`, `warp_core`, `warp_util`, `warp_files`, `warp_errors`, `warp_logging`, `repo_metadata`, … | none (one boundary each, stable, no spec needed) |
| Test-only | `integration` (real-display GUI test framework), `asset_macro` | none |

Real dependency edges between spec'd nodes:

```
app ──► ui-framework, terminal-core, ai-core, local-inference, object-store, settings
app-terminal ──► terminal-core        (submodule of app)
app-ai ──► ai-core, local-inference   (submodule of app)
ai-core ──► ui-framework              (warpui_core dependency)
local-inference ──► (external) warp_multi_agent_api   [git dep, warp-proto-apis]
```

## Invariants

- **Single front-end.** The GUI desktop app is the only front-end; the TUI and wasm/web targets are
  deleted, not dormant. Any surface marker or front-end conditional is GUI-only by definition.
- **No Warp server anywhere.** No Warp server URL, GraphQL/websocket client, or `.invalid` host
  remains in Rust. New code must not add one; remote needs either go provider-direct or don't ship.
- **Entity-handle ownership.** A global `App` owns all views/models; views reference each other via
  `ViewHandle<T>`, never direct ownership. Context params (`AppContext` / `ViewContext` /
  `ModelContext`) are named `ctx` and go last (closures excepted).
- **Terminal-model locking.** `TerminalModel` locks are deadlock-prone: never stack `model.lock()`
  calls in one call stack; prefer passing an already-locked reference down; keep lock scopes short.
- **Runtime feature flags.** Gate with `FeatureFlag::…​.is_enabled()`, not `#[cfg]`, unless the
  code cannot compile both ways. The `simplewarp` cargo feature set is the only feature set; there
  are no dogfood/preview channels.
- **macOS-first.** Platform code for Windows/Linux is retained (fully local, so not a deletion
  target) but unverified; macOS is the only platform that counts for acceptance.
- **Mouse input.** `MouseStateHandle` is created once during construction and cloned where tracked;
  an inline `MouseStateHandle::default()` while rendering silently disables mouse interaction.

## Decisions

- The AI call path: the client keeps tool-running, block rendering, and next-request decisions;
  only the model call moved from Warp's server to the local adapter
  ([`local-inference`](../../crates/local_inference/SPEC.md)).
- The local object store keeps the cloud object *model* (ids, metadata, serde shapes) while all
  sync/sharing machinery is gone — old data still loads
  ([`object-store`](../../crates/cloud_objects/SPEC.md)).
- Removal methodology of the fork (historical, all applied): gate → hide → delete, one vertical at
  a time, each round pinned by test-count ledger entries in `simplify-specs/plan.md`.
