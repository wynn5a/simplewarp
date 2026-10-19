---
id: simplewarp
type: goal-and-requirements
title: SimpleWarp — Goal and Requirements
status: active
---

# SimpleWarp — Goal and Requirements

## Goal

SimpleWarp is an offline terminal with bring-your-own-key AI: everything Warp's open-source client
does locally, with every dependency on a Warp-hosted service removed. The app runs entirely on the
user's machine; the AI provider the user configures is the only meaningful network destination.

## Problem

Warp's client couples its local value (terminal emulation, blocks, completions, agent mode) to
Warp's cloud (login, Drive sync, billing, telemetry, server-side AI). SimpleWarp keeps the local
value for a user who wants none of the cloud — no account, no subscription, and API keys that never
leave the machine.

## Audience

The primary user is the maintainer; product and breaking-change decisions optimize for their
workflow first. The repo stays publicly readable with public-facing docs, but no stability
commitment is made to outside users.

## Capabilities

What the product does today:

- GPU-rendered GUI terminal: tabs, panes, windows, splits; blocks with command metadata.
- Shell management: shell selection and launch configs, available-shell detection, command history.
- Input help: completions/suggestions, autosuggestions, command corrections, command palette.
- BYOK AI (agent mode): conversations with streamed model replies from the user's configured
  provider; the client runs the tools (shell commands, file reads/edits, grep, glob) locally.
- MCP servers the user adds themselves, as agent tools and resources.
- Workflows and notebooks (including `.ipynb` rendering and editable Mermaid); local persistence
  of conversations, workflows, notebooks, and environments in a SQLite-backed object store.
- Workspaces and sessions, themes and appearance, typed settings with a generated schema.
- macOS `.app` bundling (`script/bundle_simplewarp`, ad-hoc signed).

## Non-goals

Excluded by decision (the simplification that created this fork — see `simplify-specs/plan.md`):

- Login, accounts, and any Warp-hosted service: Warp Drive sync, teams, sharing, cloud and ambient
  agents, session sharing, telemetry, crash reporting, auto-update, billing.
- The server-side AI harness; AI requests go provider-direct, never through a Warp server.
- Computer use.
- Any second front-end: the headless TUI and the wasm/web target are deleted (desktop only).
- Fetching remote images named in markdown or notebooks.
- Distribution as a notarized, auto-updating product.

## Principles

- **Local-only network policy.** Allowed traffic: the configured AI provider; LSP server installs
  only on explicit request (GitHub/npm); `gh` polling for the current branch's PR only while a PR
  chip is visible; user-added MCP servers including their loopback OAuth callback. Nothing else.
- **No login.** Every build runs against the anonymous default user.
- **Persisted-shape compatibility.** Serde/persisted enum variants and setting keys stay (or get a
  tombstone) so pre-fork Warp data and configs still load.
- **Deletion scope.** Only what *requires* a remote service is a deletion target. Fully-local
  features are never deleted for being disabled — constant-false local flags are per-feature
  enable decisions. *(Standing user ruling, 2026-09-15, reaffirmed 2026-09-25.)*
- **One feature set.** The `simplewarp` cargo feature set is the app's only feature set; feature
  flags resolve at runtime (`FeatureFlag::is_enabled()`), not via cfg, wherever code compiles both
  ways.
- **License split.** `warpui` / `warpui_core` (the UI framework) are MIT; the rest is AGPL v3,
  inherited from upstream.

## Known limitations

Confirmed with the user (2026-10-19):

- The macOS bundle is ad-hoc signed, not notarized — Gatekeeper prompts on first open; intended
  for local launch.
- Six workspace tests fail environmentally (five ssh-dependent, one palette) and are treated as
  known noise in acceptance runs.
- Windows/Linux status is unverified: platform code is retained from upstream but never built
  since the fork. macOS is the only platform that is built, bundled, and verified.
- Pre-fork Warp data loads partially: conversations open; server-id-only rows, team-owned rows,
  and preference objects do not.
