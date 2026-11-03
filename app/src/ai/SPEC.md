---
id: app-ai
type: submodule-design
title: Agent Mode UI & Orchestration (app/src/ai)
parent: app
depends-on: [ai-core, local-inference]
status: active
---

## Responsibility

Agent mode as a product: conversation views, models and navigation; block rendering and context
chips; client-side tool execution (shell commands, file reads/edits, grep, glob); artifacts;
user-added MCP servers as tools and resources; skills; execution profiles and harnesses (including
local child harnesses such as claude/codex); assistant entry points; conversation export and
restoration.

## Boundary

Domain types come from [`ai-core`](../../../crates/ai/SPEC.md); the provider call goes through
[`local-inference`](../../../crates/local_inference/SPEC.md). This module owns the UX and the
orchestration — what runs, what renders, and what gets sent next. The persisted task is what the
reply events build: `local-inference` stores the tool results there, so a restored conversation
carries them as action results and a continued one replays them, and per-conversation token usage
sums the cache reads and writes that each stream reports. No Warp-server concepts
(conversation links, cloud runs, ambient agents) may reappear here.
