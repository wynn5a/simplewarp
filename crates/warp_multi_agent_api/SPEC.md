---
id: multi-agent-api
type: module-design
title: Agent Wire and Persistence Contract (warp_multi_agent_api)
parent: architecture
status: active
---

## Responsibility

The `warp.multi_agent.v1` protobuf types (request, response events, tasks, messages, tool calls and
results) that the client and [`local-inference`](../local_inference/SPEC.md) speak, and the shape
in which agent tasks are persisted (`agent_tasks.task` is an encoded `Task`). `build.rs` rewrites
the edition-2023 sources under `proto/` to proto3 and compiles them with `prost-reflect-build`;
`src/lib.rs` re-exports the generated types and the descriptor pool.

## Provenance

Vendored from `https://github.com/warpdotdev/warp-proto-apis` at
`b0886a9523e2e05d102f61bd0a212dc15ade4835` (`apis/multi_agent/v1/*.proto` and
`apis/multi_agent/v1/gen/rust`), AGPL-3.0 (`LICENSE.md`). At import only the proto directory in
`build.rs` changed, and `src/lib.rs` wraps the generated code in a private `generated` module
(re-exported with `pub use generated::*`) so the clippy allowance stays off the hand-written code.
It is no longer synced with upstream.

## Invariants

- **Additive only.** Persisted tasks must keep decoding: add fields, messages, enum values or oneof
  arms; never reuse a field number (reserve removed numbers and names); never change an existing
  field's type, number, cardinality or oneof membership. A new field's default must mean "old data".
- Every local change to a `.proto` is listed under "Local changes" below.

## Local changes

None yet.

## Boundary

Depends only on `prost`, `prost-types` and `prost-reflect`. `ai`, `persistence`,
`local_inference`, `app` and `integration` depend on it.
