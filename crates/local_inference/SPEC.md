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
`QuotaLimit` (user ruling, round 4ib). Model-list and provider config live here.

## Boundary

The wire contract is the external `warp_multi_agent_api` crate (git dependency,
`warp-proto-apis`): the adapter must keep speaking that request/response shape so the client side
stays unchanged. API keys never leave the machine; the configured provider is the only network
destination this crate may talk to.
