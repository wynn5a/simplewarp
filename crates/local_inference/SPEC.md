---
id: local-inference
type: module-design
title: BYOK Provider Adapter (local_inference)
parent: architecture
status: active
---

## Responsibility

The bring-your-own-key AI call path — the local replacement for the deleted server client. It keeps
the agent loop on the machine: reads the conversation out of the request, calls the configured
provider direct with the user's key, and converts the reply into the same `api::ResponseEvent`
stream the client already knows how to apply. The client keeps what it always owned — running the
tools, rendering the blocks, deciding what to send next; only the model call moved (before:
client → Warp server → provider; after: client → provider). Provider 429 responses map to
`QuotaLimit` (user ruling, round 4ib), and so does a rate limit reported inside a stream. It
speaks three protocols: Anthropic Messages, OpenAI Chat Completions, and OpenAI Responses
(stateless, `store: false`; reasoning is not replayed across turns). Model-list and provider
config live here.

The conversation the client keeps is the task that the reply events build, so the adapter echoes
into it what the client sent: the question and every tool result in `Request::input`, before the
reply. A stored result is cut by the same rule as the one sent to the model (16k characters, head
and tail), so a replay shows the model what it saw; calls stored without results (older
conversations) are replayed with a placeholder. Requests are built for provider prompt caching:
the system prompt holds only what stays the same across a conversation, the per-request state
(working directory, time, git) follows the newest turn and is never stored, and requests to the
built-in Anthropic endpoint mark cache breakpoints on the system prompt and the two newest user
messages (custom Anthropic-schema endpoints get no cache fields). Cache reads and
writes are reported in the stream's token usage. The user's `~/.agents/APPEND_SYSTEM.md`, read
afresh for every request but a summary, is appended to the very end of the system prompt (it
cannot replace it); a missing or unreadable file adds nothing.

The `remember` tool (offered only when the client lists it, with a `# Memory` section of the
system prompt that says what is worth keeping) carries one fact and a scope (`global`, or
`project` when the scope is missing or unknown); the client saves it and reports where. The facts
reach later requests as rules, the same way as the user's own.

A reply's opening (`Init`, the transaction, the echoed inputs, a summary's unfinished placeholder)
goes out before the provider is called (a `CreateTask` for a request with no task comes first, outside
the transaction, so a rollback never removes the conversation's root task), so a request stopped while the provider is still thinking
keeps what it sent and a summary shows as in progress at once. A request that fails before the
provider starts replying reports it (`Failure::before_reply`), with whether `agent-harness`'s retry
policy would send it again (`Error::is_retryable`, which reads the `retry-after` headers of an HTTP
error and Anthropic's in-stream `error` events). When the client says it will retry
(`Settings.will_retry_failures_before_reply`) and the failure is retryable, or the request is a
summary, a `RollbackTransaction` precedes the error, so a retry stores nothing twice and a failed
summary leaves no unfinished summary behind; otherwise the transaction is committed with the error
and the task keeps what the request sent. A request carrying tool results and a question (a steer) has the results
replayed and stored first, right after the calls they answer, then the question.

A request the provider rejects for exceeding the context window (Anthropic "prompt is too long",
OpenAI `context_length_exceeded` before or inside the stream, the vLLM-style "maximum context length
is" wording, Gemini's "input token count … exceeds the maximum") ends the stream with
`StreamFinished` reason `ContextWindowExceeded` instead of an HTTP error; only a 400 whose code or
wording is specific to that failure counts, and an overflowed `/compact` rolls its transaction back
so no unfinished summary is left in the task. Every finish that carries an input count also reports
how full the context is, in `ConversationUsageMetadata`: `total_input_tokens` is the request's input
and an `Other` context segment the part of the reply later requests carry (output less reasoning
that is not sent back; `TokenUsage.output` stays the billed total). Model metadata includes each
first-party model's input-token window (`context_window`: a prefix table, lowered by Anthropic's
`/models` `max_input_tokens` when that is smaller), and `convert::input_text` gives the text an
input adds to what the model sees, for estimates.

A summary request (`SummarizeConversation`) does not show the model the conversation as turns: the
history since the previous summary is cut by `agent-harness`'s rule at `keep_recent_tokens`
(0 means summarize everything), the part before the cut is sent as one labelled transcript with the
previous summary to update and the user's `/compact` focus, under a summarization system prompt and
with no tools; the client's output cap is sent to Anthropic only (OpenAI reasoning models reject
`max_tokens` and count reasoning in `max_completion_tokens`, and the Responses plan endpoint rejects
`max_output_tokens`). A summary counts
only when the model ends it normally with some text; otherwise it is rolled back like an
overflowed one and the stream ends in an error. The finished summary records the first message it kept, the cumulative read and
modified files (also appended to its text) and the context before and after, and its stream reports
the compacted estimate as the context usage. On replay a summary drops only the turns before its
first kept message (an older summary without one still drops everything before it); the stored
`/compact` instruction is not replayed, and a tool result whose call was summarized away is sent as
user text.

## Boundary

It depends on [`agent-harness`](../agent_harness/SPEC.md) for the token-estimate rule only. The wire
contract is [`warp_multi_agent_api`](../warp_multi_agent_api/SPEC.md), vendored from
`warp-proto-apis` and changed additively only: the adapter must keep speaking that request/response
shape so the client side stays unchanged. API keys never leave the machine; the configured provider is the only network
destination this crate may talk to.
