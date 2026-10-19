---
id: object-store
type: module-design
title: Local Object Store (cloud_object*)
parent: architecture
status: active
---

## Responsibility

Local persistence shaped like the cloud object model: `cloud_objects` is the model-agnostic
substrate (server-facing ids, user ids, object metadata, type/format definitions); 
`cloud_object_models` holds the concrete per-family payloads, typed aliases and model-local SQLite
adapters; `cloud_object_persistence` is the shared SQLite persistence infrastructure; `persistence`
is the Diesel/SQLite layer with migrations. Notebooks, workflows, environments and conversations
persist here.

## Boundary

Internal layering is one-way: the substrate must not depend on concrete models, and
model-agnostic crates must not depend on `cloud_object_models`. All sync/sharing machinery is
gone and must not return; the object shapes remain deliberately (with tombstones where variants
were dropped) so pre-fork Warp data still loads — the user-facing contract is recorded in the
[goal doc](../../../specs/SPEC.md).
