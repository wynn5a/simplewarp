---
id: ui-framework
type: module-design
title: WarpUI — GPU UI Framework
parent: architecture
status: active
---

## Responsibility

The custom UI framework the app renders with: `Element`/`View` layout (Flutter-inspired) and the
GPU/WGSL rendering pipeline (`warpui_core`); platform backends for macOS/Windows/Linux (`warpui`);
the actions system for event handling; and the entity-handle model — a global `App` owns all
views/models, `ViewHandle<T>` references them, `AppContext` grants temporary access. Widget-level
extras live in `warpui_extras` / `ui_components`.

MIT-licensed — the one MIT corner of an otherwise AGPL repo; keep the split intact.

## Boundary

Rendering, layout, input and event-loop primitives only: no product or domain logic, no settings,
no persistence, no AI. Nothing above it in the workspace depends downward past it; it is the
leaf. Mouse interaction requires a `MouseStateHandle` created at construction and cloned where
tracked — inline `default()` handles silently disable mouse input.
