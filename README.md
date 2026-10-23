<div align="center">

<a href=".github/assets/simplewarp-intro.mp4">
  <img src=".github/assets/simplewarp-intro.webp" alt="SimpleWarp, a 15-second introduction" width="100%">
</a>

<sub>▶ Click the preview to watch the full intro with sound.</sub>

# SimpleWarp

**The offline terminal with bring-your-own-key AI.**

Local-first · Fast · Easy to use · Beautiful

</div>

---

## Built on Warp

SimpleWarp would not exist without [Warp](https://www.warp.dev). The team at
[warpdotdev/warp](https://github.com/warpdotdev/warp) built the hard parts: a terminal rendered on
the GPU in Rust, their own UI framework (WarpUI) to draw it, command blocks, an input editor that
works like a real text editor, completions, workflows, notebooks, and an agent mode that runs its
tools in your shell. Then they published the client's source, which is what made a fork like this
one possible.

Everything good in this repo is theirs. SimpleWarp only takes things out: it is Warp's client with
every Warp-hosted service removed, so that it runs entirely on one machine. That is a narrower
product, not a better one. If you want accounts, Warp Drive, teams, sharing or cloud agents, use
[Warp](https://www.warp.dev). Thank you, Warp team.

## What you get

| | |
|---|---|
| **Local-first** | No account and no login. Notebooks, workflows, conversations and settings live in a local SQLite store. The only network destination is the AI provider you configure. |
| **Fast** | Warp's Rust core and GPU (WGSL) renderer, with roughly 940,000 lines of cloud plumbing removed. |
| **Easy to use** | `cargo run` and it starts. Tabs, panes, blocks, completions, autosuggestions, command palette and agent mode all work as they do in Warp. |
| **Beautiful** | WarpUI as Warp designed it: themes, typography and the same care for detail. |

AI is bring-your-own-key: agent mode talks to your provider directly, streams the replies, and runs
its tools (shell commands, file reads and edits, grep, glob) on your machine. MCP servers you add
yourself are available as tools.

## What's gone

Login and accounts, Warp Drive sync, teams and sharing, cloud and ambient agents, session sharing,
the server-side AI harness, telemetry, crash reporting, auto-update, computer use, the headless TUI
and the web target. [FAQ.md](FAQ.md) lists what is kept and dropped. The network traffic that
remains:

- requests to the AI provider you configure;
- LSP server installs, only when you ask for one;
- `gh` polling for the current branch's PR, only while a PR chip is visible;
- MCP servers you add, including their loopback OAuth callback.

## Run it

```bash
./script/bootstrap   # platform-specific setup
cargo run            # builds the simplewarp bin with the app's default (only) feature set
```

`script/bundle_simplewarp` builds a macOS `.app` (ad-hoc signed, not notarized) with its bundled
resources; it needs `cargo about` (`script/install_cargo_release_deps`), or `NO_LICENSES=1` to skip
the licence file.

## Develop it

```bash
./script/presubmit   # fmt, clippy, and tests
```

[AGENTS.md](AGENTS.md) is the engineering guide: build and test commands, code style, WarpUI
patterns. [CONTRIBUTING.md](CONTRIBUTING.md) has the short version. The cleanup history lives in
[simplify-specs/](simplify-specs/).

## Licensing

Same licences as upstream. The UI framework (the `warpui_core` and `warpui` crates) is under the
[MIT license](LICENSE-MIT) and the rest of the code under the [AGPL v3](LICENSE-AGPL). This is a
derivative of Warp's open-source client, and the upstream copyright notices stand. "Warp" is a
trademark of its owners; SimpleWarp is an independent fork and is not affiliated with or endorsed by
Warp.

## Security

See [SECURITY.md](SECURITY.md). The [Code of Conduct](CODE_OF_CONDUCT.md) is upstream's.
