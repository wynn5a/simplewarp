---
id: app-terminal
type: submodule-design
title: Terminal Surfaces (app/src/terminal)
parent: app
depends-on: [terminal-core]
status: active
---

## Responsibility

The terminal as a product surface: `TerminalView` and the block-list rendering stack (elements,
viewport, settings), alt-screen handling, in-terminal find, command history and corrections, shell
detection and bootstrap, launch, and the app-side `TerminalModel` surfaces (focus, events,
restoration).

## Boundary

Builds on [`terminal-core`](../../../crates/warp_terminal/SPEC.md)'s emulation model and shell
primitives (`warp_terminal::shell`); renders through WarpUI elements. No AI, no network, no object
persistence. Bound by the terminal-model locking invariant (see architecture): no stacked
`model.lock()` calls; pass locked references down.
