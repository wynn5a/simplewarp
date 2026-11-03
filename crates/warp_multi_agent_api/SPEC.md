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

- `Request.Input.SummarizeConversation`: `keep_recent_tokens = 2` (0 = summarize everything, the
  old behavior), `context_tokens = 3` (0 = unknown) and `max_output_tokens = 4` (0 = provider
  default), for compaction (harness P2).
- `Message.Summarization.ConversationSummary`: `first_kept_message_id = 3` (empty = the summary
  replaces everything before it, the old behavior), `read_files = 4`, `modified_files = 5`,
  `tokens_before = 6`, `tokens_after = 7` (0 = unknown).
- The `remember` tool (harness P3): `Message.ToolCall.Remember` (`remember = 39`: `content`,
  `scope` GLOBAL/PROJECT), top-level `RememberResult` (`Success { project_root, already_saved }` /
  `Error { message }`), `Message.ToolCallResult.remember = 43`,
  `Request.Input.ToolCallResult.remember = 38`, and `ToolType.REMEMBER = 36`.
- `Request.Settings.will_retry_failures_before_reply = 32` (false = the client does not retry, the
  old behavior): the local adapter sends nothing but the error for a retryable failure before the
  reply, so a retry does not store the inputs twice (harness P4).

## Boundary

Depends only on `prost`, `prost-types` and `prost-reflect`. `ai`, `persistence`,
`local_inference`, `app` and `integration` depend on it.
