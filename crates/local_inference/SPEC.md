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
writes are reported in the stream's token usage.

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

## Boundary

It depends on [`agent-harness`](../agent_harness/SPEC.md) for the token-estimate rule only. The wire
contract is [`warp_multi_agent_api`](../warp_multi_agent_api/SPEC.md), vendored from
`warp-proto-apis` and changed additively only: the adapter must keep speaking that request/response
shape so the client side stays unchanged. API keys never leave the machine; the configured provider is the only network
destination this crate may talk to.
