---
id: app-ai
type: submodule-design
title: Agent Mode UI & Orchestration (app/src/ai)
parent: app
depends-on: [ai-core, local-inference, agent-harness]
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
sums the cache reads and writes that each stream reports. Context usage is separate from that sum:
each conversation keeps the newest request's reported input and kept reply (from the stream's
`ConversationUsageMetadata`) plus an estimate of the request in flight, taken from the text the
model sees ([`agent-harness`](../../../crates/agent_harness/SPEC.md) owns the rule), persists the
total, and the input footer shows it against the active model's window (capped by the execution
profile's `context_window_limit`; custom-endpoint models count as 128k). The controller compacts a
conversation on its own (Settings > AI > Context, on by default): before a request once the context
is within the reserve of the window, holding the request back until the summary is written, and
after a request overflows the window, then resuming once; `/compact`, `/compact-and` and
`/fork-and-compact` send the same summary request (how much to keep verbatim and the current
usage), and the finished summary resets the usage to its compacted estimate. A held request is
sent even when its compaction fails; when the compaction is cancelled its question goes back to the
head of the queued prompts, and a failed compaction waits for new content before trying again.

Rules reach every request as project-rules context: the project's rule files (one per directory,
`WARP.md` > `AGENTS.md` > `CLAUDE.md`, from `ai-core`'s project context), `~/.agents/AGENTS.md`,
and the Knowledge rules when rules are on, both those for all work and those saved for the
repository of the working directory (none for a session on another host, whose working directory
is not on this machine). `~/.claude/CLAUDE.md` is never read: it is written for another
tool and may hold what the user meant only for that tool's provider; a user who wants it shared
links it, `ln -s ~/.claude/CLAUDE.md ~/.agents/AGENTS.md`. When rules are on the agent is offered
`remember`: the executor saves the fact as a Knowledge rule marked as added by the agent and, for
the `project` scope, bound to the local repository root, after the user approves it. Only Settings >
Agents > Knowledge can save memories without asking; a conversation's auto-approve does not cover
them, since a memory reaches every later conversation. With rules off the executor saves nothing. The
Knowledge page labels those rules. Memory content is never logged. No Warp-server concepts
(conversation links, cloud runs, ambient agents) may reappear here.
