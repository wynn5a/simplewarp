# Contributing to SimpleWarp

SimpleWarp is a personal fork of [warpdotdev/warp](https://github.com/warpdotdev/warp). Upstream's
issue, spec and Oz-review process does not apply here: there are no readiness labels, spec PRs,
bots or CLA. Open an issue or a pull request on this repository.

## Scope

Anything that needs a Warp-hosted service (login, Drive, cloud agents, sharing, telemetry) is out
of scope. A change should keep the app working offline, with the AI provider as the only network
destination. Cleanup of what upstream's server owned is tracked in
[simplify-specs/plan-p2.md](simplify-specs/plan-p2.md).

## Setup and checks

```bash
./script/bootstrap   # platform-specific setup
cargo run --no-default-features --features simplewarp --bin simplewarp
./script/presubmit   # fmt, clippy, and tests
```

- `./script/format --check` and `cargo clippy --workspace --all-targets --all-features --tests -- -D warnings` must pass.
- Run unit tests with `cargo nextest run`. Bug fixes should come with a regression test.
- User-facing flows can be covered under [`crates/integration/`](crates/integration/).
- See [AGENTS.md](AGENTS.md) for the full style guide, including WarpUI patterns and terminal
  model locking rules.

## Pull requests

- Keep each PR to one logical change, and explain *what* and *why* in the commit message.
- Include before/after screenshots for visual changes.

## Security

See [SECURITY.md](SECURITY.md). Do not open public issues for vulnerabilities.
