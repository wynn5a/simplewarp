# SimpleWarp

SimpleWarp is a fork of [warpdotdev/warp](https://github.com/warpdotdev/warp): an offline terminal
with bring-your-own-key AI. It has no login, Warp Drive, cloud agents, session sharing, telemetry,
crash reporting or auto-update. The only network traffic goes to the AI provider you configure.

## Run it

```bash
./script/bootstrap   # platform-specific setup
cargo run --no-default-features --features simplewarp --bin simplewarp
```

`script/bundle_simplewarp` builds a macOS `.app` (ad-hoc signed, not notarized).

## Develop it

```bash
./script/presubmit   # fmt, clippy, and tests
```

[AGENTS.md](AGENTS.md) is the engineering guide: build and test commands, code style, WarpUI
patterns. [CONTRIBUTING.md](CONTRIBUTING.md) has the short version, and [FAQ.md](FAQ.md) answers
what this fork keeps and drops. The cleanup history lives in [simplify-specs/](simplify-specs/).

## Licensing

The UI framework (the `warpui_core` and `warpui` crates) is licensed under the
[MIT license](LICENSE-MIT). The rest of the code is licensed under the [AGPL v3](LICENSE-AGPL).
This is a derivative of Warp's open-source client; the upstream copyright notices stand.

## Security

See [SECURITY.md](SECURITY.md). The [Code of Conduct](CODE_OF_CONDUCT.md) is upstream's.
