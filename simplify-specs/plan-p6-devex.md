# SimpleWarp — Plan, part 6: developer efficiency (build and test loop)

Survey date: 2026-10-26, HEAD `1763d7766`, Apple Silicon (10 cores), rustc 1.92.0. Results of each
round go in [plan.md](plan.md)'s round log, as before.

**Constraint: no behavior change in the shipped app.** Every item here changes how the app is built,
checked and tested, not what it does. The one exception is D2 (TLS crypto provider), which needs an
HTTPS smoke test against a real AI provider and an HTTPS MCP server.

## Why

The time to see an edit in a running binary or a passing test sets the pace of every other plan
(the performance work included). Today that loop is 40–65 s on the `warp` crate, and macOS purges
`target/` on its own, so a 3-minute clean build is a regular event, not a rare one.

## Baseline (measured 2026-10-26)

Dev profile, `simplewarp` bin, after `target/` was purged:

| Loop | Now | Target |
| --- | --- | --- |
| Clean `cargo build --bin simplewarp` | **181 s** (593 s CPU, 5.9 GB peak RSS) | ≤ 120 s |
| Edit an `app/` file → `cargo build` | **42 s** | ≤ 12 s |
| Edit an `app/` file → `cargo check` | **23 s** | ≤ 6 s |
| Edit a `warpui_core` file → `cargo build` | **54 s** | ≤ 20 s |
| Edit an `app/` file → `nextest -p warp --lib --no-run` | **64 s** (116 s from cold) | ≤ 25 s |
| Edit an `app/` file → `clippy -p warp --all-targets --tests` | **36 s** | ≤ 15 s |
| Clean `release-small` (ThinLTO) bundle | ~15 min (plan.md, 2026-10-24) | unchanged; not a dev loop |

Same edits with `CARGO_PROFILE_DEV_INCREMENTAL=true`, cache warm: **build 11–12 s, check 5.5 s**
(the first build that fills the cache takes 65 s). The cache is 3.7 GB for one feature set.

Where the time goes:

- **The `warp` crate is 628k lines, 81% of first-party Rust** (134k of it test files). It takes
  40 s on its own: 27 s of frontend to `rmeta` (single-threaded on stable rustc), 13 s of codegen,
  then 2.6 s to link. Every edit anywhere in `app/` pays all of it, because incremental is off.
- **Clean-build critical path:** `aws-lc-sys`'s build script (cmake compiling AWS-LC, the default
  rustls crypto provider pulled in by `reqwest`'s `rustls` feature) runs **62 s** and gates
  `rustls` → `reqwest` → `warp_util` → … → `warp`. `warp` starts at 138 s; without that script the
  chain is ~70 s shorter in wall time.
- Other C builds (off the critical path, but CPU on every clean build): `libgit2-sys` 17 s
  (`git2`, used only by `app`), `zstd-sys` 14 s, `lzma-sys` 9 s (`zip` → `xz2`, via `node_runtime`
  and `warp_logging`), `onig_sys` 7 s (`syntect`'s default regex engine), `warpui` build script
  11 s (Metal shaders). Large Rust crates: `image` 32 s, `warp-command-signatures` 20 s,
  `tokio` 12 s, `tantivy` 11 s.
- **The serial chain `warpui_core` → `warp_core` → … → `warp`** is ~11 s of one-crate-at-a-time
  work before `warp` can start, which is why a `warpui_core` edit costs 54 s.
- `sccache` is installed but not configured (`RUSTC_WRAPPER` unset, 0 requests).

Inside `app/`, cross-module `crate::` references (non-test files) show what blocks a split:

| from \ to | terminal | ai | workspace | editor | code | search | pane_group | settings | util |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| terminal (177k) | | 213 | 17 | 9 | 4 | 88 | 16 | 37 | 57 |
| ai (110k) | 194 | | 27 | 7 | 25 | 7 | 12 | 30 | 31 |
| workspace (47k) | 46 | 41 | | 5 | 9 | 11 | 12 | 14 | 27 |
| editor (41k) | 4 | 3 | 2 | | | 2 | | 2 | 7 |
| code (33k) | 4 | 8 | 3 | 15 | | 4 | 6 | 8 | 6 |
| notebooks (19k) | 6 | 1 | 8 | 9 | 1 | | 11 | 7 | 16 |

`terminal` ↔ `ai` is a dense two-way cycle (213 / 194). `editor`, `themes`, `settings`, `util`
and `persistence` point outward very little, so they are the leaves a split starts from.

## Workstreams, in order

Cheap rounds first (D1–D4, one round each, measured against the baseline above), then the
structural one (D5).

### D1 — Incremental compilation back on for the dev loop

The single largest win in the table: 42 → 12 s per build, 23 → 5.5 s per check. It was turned off
for disk (plan.md "Disk"), but the `PreToolUse` hook `.claude/hooks/cargo-disk-guard.sh` now prunes
stale `target/*/incremental` caches and removes `target/` below 10 GB free, which is the guard the
old decision lacked.

- Flip `[profile.dev] incremental` back to `true` (the `test` profile inherits it), keep the hook,
  and rewrite the profile comment and plan.md's "Disk" section to match.
- Measure the test and clippy loops with incremental on (not measured yet), and the cache size
  across a day of edits on both feature sets.
- Fallback if disk is still a problem: keep the profile default off and turn it on only in
  `script/run` and a new check script (D4).

### D2 — Take `aws-lc-sys` off the critical path

**Done (2026-10-26).** Clean build **181 → 120 s** (cold) and AWS-LC, cmake and NASM are gone from
the graph. reqwest is built with `rustls-no-provider`; rustls has only its `ring` feature; `warp::run`
and `warp::run_integration_test` install `ring` as the process-wide provider (rmcp builds its own
reqwest clients, so a per-client install is not enough), and so does the unit-test `ctor` in
`app/src/lib.rs`. Verified: format, the three clippy runs, nextest 6,240/6,240, doc tests, and real
HTTP/2 TLS handshakes against `api.anthropic.com`, `api.openai.com` and `github.com`. In-app checks
are in [runbook-p6-devex.md](runbook-p6-devex.md).

Clean build −50 to −60 s wall time, and one fewer native toolchain requirement (cmake).

- Workspace `reqwest`: `default-features = false`, keep `charset`, `http2`, `system-proxy`, and
  use `rustls-no-provider` with `rustls`'s `ring` feature (`ring` builds in a few seconds).
  Install the `ring` provider once at startup if any client is built before rustls picks a
  default. `crates/ui_components` depends on `rustls` directly; check its features too.
- `cargo tree -i aws-lc-sys` must come back empty.
- Verify: the live provider test (plan.md "How to test the AI"), one real conversation in the app,
  an HTTPS MCP server, and the Claude notification plugin's GitHub fetch.

### D3 — Survive `target/` purges: sccache, and trimming the C builds

**Done (2026-10-26).** Native builds left: `zstd-sys` (tantivy) and `libgit2-sys` (git2), down from
six. `syntect` uses `default-fancy` (only notebook command blocks highlight with it, off the main
thread); `zip` keeps only `deflate` + `deflate64` (log export writes Deflated, Node and LSP archives
read Deflated), dropping `bzip2-sys`, `lzma-sys` and zstd/aes/lzma support. `libgit2-sys` (19 s) is
now the clean-build critical path in front of `warp`.

sccache, measured with a separate target dir: filling the cache costs a clean build of 133 s, a
clean build from a warm cache takes **31 s** (1,065/1,065 cacheable units hit, 652 MB cache). With
incremental off every crate is cacheable, the `warp` crate included; turning D1 on would make the
workspace members uncacheable. It is opt-in per machine (`export RUSTC_WRAPPER=sccache`), not set
in `.cargo/config.toml`, because a missing wrapper fails the build (CI has none).

macOS deletes `target/` (it carries `CACHEDIR.TAG`), so the clean build is a regular cost.

- Set `RUSTC_WRAPPER=sccache` (user-level `~/.cargo/config.toml` or `script/` env, not forced
  on CI). sccache does not cache incremental crates, which here means only the workspace members,
  so it covers the ~1,000 dependency units that dominate a clean build's CPU. Measure purge →
  rebuild with a warm cache.
- Drop native builds where a pure-Rust or system option exists, each only if behavior is the same:
  `syntect` → `default-fancy` (pure-Rust regex, no `onig_sys`) after checking highlight output on
  the bundled syntaxes; `zip` without the `xz` feature unless an archive we read needs it;
  `git2` against the system libgit2 or replaced by `gix` (bigger job, record only).

### D4 — A fast local check, and a leaner presubmit

`script/presubmit` lints the `warp` crate twice (the workspace clippy run, then `-p warp` for the
default configuration) and then runs the whole workspace's tests. That is the right gate before a
push, and the wrong one for the edit loop.

- Add `script/check`: format check, clippy on the crates changed versus `master` (plus their
  workspace dependents), and `nextest` filtered to those packages (`-E 'rdeps(...)'`).
- Measure whether the second `-p warp` clippy run catches anything the first does not (it differs
  only in feature unification from `integration`'s `test-util` features); if it is the only place
  the default configuration is linted, keep it but make the first run `--exclude warp`.
- Update AGENTS.md "Linting and Formatting" and the verification list in plan.md.

### D5 — Split the `warp` crate, leaves first

Structural. This is what removes the 27 s single-crate frontend from every edit, gives cargo
parallelism inside the app, and shrinks per-crate test binaries.

- Pilot: move `app/src/editor` (41k lines, 19k of tests; 18 outbound references) into a
  `crates/app_editor` crate. Upward references get inverted through a trait or a callback passed in
  at construction, not by moving the target along with it. Measure the edit loop for an editor file
  and for a terminal file before and after.
- If the pilot pays off, continue in coupling order: `themes`, `settings` + `settings_view`,
  `notebooks`, `code` + `code_review`, `search`. Each one is a round of its own with its own
  `SPEC.md`, per AGENTS.md.
- `terminal` and `ai` stay in the `warp` crate until the cycle between them is broken. That needs
  its own survey of the 400-odd references (agent mode reading the block list, terminal views
  embedding agent UI) and is out of scope until the leaves are out.
- Keep `cargo check` of the default and `simplewarp` builds warning-free and the nextest counts
  equal across each move.

## Out of scope, and why

- **Linker swap (mold, lld):** linking is 2.6 s with Apple's ld; nothing to win.
- **Cranelift codegen backend, parallel rustc frontend (`-Zthreads`):** both need nightly; the
  workspace is on stable 1.92. Revisit if a stable release ships either.
- **Release build time (~15 min ThinLTO):** only bundles pay it. If the performance work needs
  repeated optimized builds, add a `profiling` profile (release, no LTO, `codegen-units = 16`,
  incremental) rather than touching `release-small`.
