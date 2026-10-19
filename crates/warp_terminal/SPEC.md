---
id: terminal-core
type: module-design
title: Terminal Emulation Core (warp_terminal)
parent: architecture
status: active
---

## Responsibility

The machine underneath the app's terminal surfaces: the terminal emulation model (`model`) — grid
and cell state, escape/OSC handling, block bookkeeping — and the shell primitives (`shell`), shell
launch data included.

## Boundary

No UI, no rendering, no app state, no network: emulation logic stays window-free so it is testable
headlessly. Consumed by [`app-terminal`](../../app/src/terminal/SPEC.md); must not reach back up
into app types.
