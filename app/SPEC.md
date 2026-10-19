---
id: app
type: module-design
title: Desktop App — Product Shell
parent: architecture
depends-on: [ui-framework, terminal-core, ai-core, local-inference, object-store, settings]
status: active
---

## Responsibility

Everything user-facing that is not a shared library: windows, tabs, panes and splits; workspaces
and session management; menus, keybindings and the command palette; the settings UI
(`settings_view`) and themes/appearance; launch configs and shell selection; notebooks, workflows,
notifications, URI handling, crash recovery. Owns the product identity: bin `simplewarp`, bundle id
`dev.simplewarp.SimpleWarp`, URL scheme `simplewarp`, `.app` bundling via `script/bundle_simplewarp`.
Runs as the anonymous local user — there is no auth surface to integrate.

Two deep subdomains live as submodules: [`app-terminal`](src/terminal/SPEC.md) (terminal surfaces)
and [`app-ai`](src/ai/SPEC.md) (agent mode UX and orchestration).

## Boundary

Depends on every shared crate below it; nothing in the workspace depends on it (only its binaries
do). Product decisions land here; domain logic belongs in the crate that owns the domain. New
user-visible features belong behind the single `simplewarp` feature set, never a new cargo feature
fork.
