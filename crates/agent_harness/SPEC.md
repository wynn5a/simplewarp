---
id: agent-harness
type: module-design
title: Agent-Loop Policy (agent_harness)
parent: architecture
status: active
---

## Responsibility

The decisions SimpleWarp's agent loop makes about a conversation, as pure functions over plain
data: today, the context budget (`budget`: a chars/4 token estimate, `ContextUsage` = the newest
request's provider-reported input and the part of its reply kept in the context, plus an estimate
for the request in flight that a resend replaces rather than adds to, the effective
window = the model's window capped by the execution profile's limit, and the compact `42k` /
`1.2M` format), and compaction (`compaction`, ported from pi: when to compact — past
`window − reserve_tokens`, waiting for new content when the last compaction stayed above that —
how much to keep verbatim, the cut that keeps about `keep_recent_tokens` at a user or assistant
entry and never between a tool call and its results (and marks a turn split inside), the
read/modified file lists, the labelled transcript with tool results cut to 2,000 characters, and the
fixed-structure summary prompts, initial and incremental). Steering merges and retry policy join it
in later phases of `specs/plans/native-agent-harness.md`.

## Boundary

No UI framework, no I/O, no network, no dependencies: the app asks it what to do and does it, and
[`local-inference`](../local_inference/SPEC.md) still makes the model call. Every rule here is
unit-tested in isolation. It depends on nothing; the app and
[`local-inference`](../local_inference/SPEC.md) depend on it.
