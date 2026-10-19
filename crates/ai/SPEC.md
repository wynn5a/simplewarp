---
id: ai-core
type: module-design
title: AI Domain Library (ai)
parent: architecture
depends-on: [ui-framework]
status: active
---

## Responsibility

The AI domain library the app builds on: agent machinery, LLM ids and providers, API keys,
documents and diff validation, GFM tables, the codebase index and project context, skills, and
workspace AI state.

## Boundary

A domain library: no views (agent-mode UX is [`app-ai`](../../app/src/ai/SPEC.md)), no provider
HTTP (the adapter is [`local-inference`](../local_inference/SPEC.md)), no object persistence.
Uses `warpui_core` framework types only — no rendering of its own.
