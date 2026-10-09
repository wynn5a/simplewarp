# Frequently Asked Questions

## What is SimpleWarp?

A fork of [warpdotdev/warp](https://github.com/warpdotdev/warp) that runs entirely locally. See
[README.md](README.md).

## What does it drop?

Everything that needed a Warp-hosted service: login and accounts, Warp Drive sync, teams and
sharing, cloud and ambient agents, session sharing, telemetry, crash reporting, auto-update, the
MCP gallery, and the server-side AI harness. Computer use is gone too.

## Where does AI come from?

From your own provider key. The `local_inference` crate builds the requests and streams the
replies; the model calls tools (shell commands, file reads and edits, grep, glob) that the client
runs. The AI provider is the only network destination.

## What network traffic is left?

- Requests to the AI provider you configure.
- LSP server installs, only when you ask for one (downloads from GitHub or npm).
- `gh` polling for the current branch's pull request, only while a PR chip is visible.
- MCP servers you add yourself, including their OAuth callback on loopback.

Remote images named in markdown or notebooks are never fetched.

## How do I build and run it?

```bash
./script/bootstrap
cargo run --no-default-features --features simplewarp --bin simplewarp
```

`script/bundle_simplewarp` builds a macOS `.app`. See [AGENTS.md](AGENTS.md) for the engineering
guide.

## Can I still open old Warp data?

Saved conversations load. Cloud-object rows that only have a server id, team-owned rows and
preference objects are no longer loaded. [simplify-specs/plan-p2.md](simplify-specs/plan-p2.md)
records what changed.

## Licensing

The client is [AGPL v3](LICENSE-AGPL) and the UI framework crates (`warpui_core`, `warpui`) are
[MIT](LICENSE-MIT), as upstream. Using SimpleWarp as your terminal doesn't trigger AGPL's
obligations; they apply if you modify it and distribute or host the modified version.
