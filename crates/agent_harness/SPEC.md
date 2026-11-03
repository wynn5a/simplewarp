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
`1.2M` format). Compaction cut points, steering merges and retry policy join it in later phases of
`specs/plans/native-agent-harness.md`.

## Boundary

No UI framework, no I/O, no network, no dependencies: the app asks it what to do and does it, and
[`local-inference`](../local_inference/SPEC.md) still makes the model call. Every rule here is
unit-tested in isolation. It depends on nothing; the app and
[`local-inference`](../local_inference/SPEC.md) depend on it.
