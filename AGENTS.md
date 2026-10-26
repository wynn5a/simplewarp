# AGENTS.md

This file provides guidance when working with code in this repository.

## Development Commands

### Build and Run
- `cargo run` / `./script/run` - Build and run the GUI desktop app locally (the `simplewarp` bin, the app's only feature set)
- `cargo bundle --bin simplewarp` - Bundle the app for local runs; `script/bundle_simplewarp` builds a standalone `.app`
- `export RUSTC_WRAPPER=sccache` (opt-in per machine) caches compiled crates across the `target/`
  purges macOS does on its own: a clean dev build drops from ~2 min to ~30 s once the cache is warm

### Running locally
There is no login: every build runs entirely locally against the anonymous default user, and no
build can sign in to (or sign up for) a Warp account. The old `WITH_LOCAL_SERVER` dev-login flow
that pointed the client at a local warp-server instance is gone, along with its env vars.

### Testing
- `cargo nextest run --no-fail-fast --workspace` - Run tests with nextest
- `cargo test --doc` - Run doc tests
- `cargo test` - Run standard tests for individual packages

### Linting and Formatting
- `./script/presubmit` - Run all presubmit checks (fmt, clippy, tests)
- `./script/format` - Format code
- `cargo clippy --workspace --exclude warp_completer --all-targets --tests -- -D warnings`, then
  `cargo clippy -p warp --all-targets --tests -- -D warnings` and
  `cargo clippy -p warp_completer --all-targets --tests -- -D warnings` - Run clippy the way
  `./script/presubmit` and CI do (no `--all-features`: `warp_completer` has feature-gated WIP code)
- `./script/run-clang-format.py -r --extensions 'c,h,cpp,m' ./crates/warpui/src/ ./app/src/` - Format C/C++/Obj-C code
- `find . -name "*.wgsl" -exec wgslfmt --check {} +` - Check WGSL shader formatting

### Agent tooling
- `.agents/skills/` holds this repo's own skills (`.claude/skills` is a symlink to it); `.warp/skills/`
  carries the Warp-client copy of the integration-test video skill.
- `.claude/settings.json` installs a `PreToolUse` hook, `.claude/hooks/cargo-disk-guard.sh`, that
  prunes stale `target/*/incremental` caches before any `cargo` command and removes `target/`
  outright when less than 10 GB of disk is free.
- `.codegraph/` is a CodeGraph index of the workspace: `codegraph explore "<symbols or question>"`
  returns the relevant source and call paths in one call.

### Platform Setup
- `./script/bootstrap` - Platform-specific setup (`-y` / `--yes` skips the prompts before root steps)
- `./script/install_cargo_build_deps` - Install Cargo build dependencies
- `./script/install_cargo_test_deps` - Install Cargo test dependencies

## Architecture Overview

This is a Rust-based terminal emulator with a custom UI framework called **WarpUI**.

### Specs and the simplification ledger

`specs/SPEC.md` is the goal-and-requirements spec and `specs/architecture/SPEC.md` the architecture
spec; each spec'd boundary has its own `SPEC.md` (`app/SPEC.md`, `app/src/ai/SPEC.md`,
`app/src/terminal/SPEC.md`, `crates/<crate>/SPEC.md`). Keep the relevant `SPEC.md` in step with a
change that moves a boundary. `simplify-specs/plan.md` is the decision ledger of the Warp to
SimpleWarp simplification (decisions, phase status, per-round results): record removal rounds
there, and treat the other files under `simplify-specs/` as history.

### Front-end: the GUI desktop app

The **GUI desktop app** is the `app/` crate on the WarpUI pixel/GPU framework (`warpui`,
`crates/warpui_core`): `Element`/`View` layout, GPU/WGSL rendering, mouse input, `.app` bundles.
Run with `cargo run` / `./script/run`; verify visually by running the app or with the real-display
integration framework (`crates/integration`).

SimpleWarp removed the headless TUI front-end in Phase 4, in full: the `crates/warp_tui` crate,
the app-side TUI modules, the cell-grid element library and terminal runtime in `warpui_core`
(with the `tui` feature and the `ratatui` dependency), and every TUI surface marker —
`SettingsMode` and `SettingSurfaces` are now GUI-only, and the TUI-only slash commands, bundled
skills, execution mode, and config/state paths are gone. There is one front-end.

**Skill convention:** a skill specific to the front-end says so in its name and/or description
(e.g. `gui-ui-guidelines` / `gui-integration-test` are GUI-only). Skills with no front-end
call-out are surface-agnostic.

### Key Components

**Shared UI core** (`crates/warpui`, `crates/warpui_core`):
- Entity-Component-Handle pattern: a global `App` object owns all views/models (entities); views hold `ViewHandle<T>` references to other views; `AppContext` provides temporary access to handles during render/events.
- Actions system for event handling.

**GUI rendering** (WarpUI GUI elements — GUI-specific):
- `Element`s describe visual layout (Flutter-inspired), rendered on the GPU (WGSL).
- Mouse input uses `MouseStateHandle`: create it once during construction and reference/clone it wherever mouse input is tracked. An inline `MouseStateHandle::default()` while rendering means no mouse interactions work.

**Main app** (`app/`) — the GUI desktop app:
- Terminal emulation and shell management (`terminal/`)
- AI integration including Agent Mode (`ai/`)
- Workflow editor components (`drive/workflows/`) and the cloud-object store they persist through (`cloud_object/`)
- Settings and preferences (`settings/`)
- Workspace and session management (`workspace/`)

**Core Libraries**:
- `crates/warp_core/` - Core utilities and platform abstractions (shared)
- `crates/editor/` - Text editing functionality
- `crates/warpui/` and `crates/warpui_core/` - Custom UI framework

### Key Architectural Patterns

1. **Entity-Handle System**: Views reference other views via handles, not direct ownership
2. **Modular Structure**: Workspace contains multiple workspace configurations, each with terminals, notebooks, etc.
3. **Cross-Platform**: Native implementations for macOS, Windows, and Linux
4. **AI Integration**: Built-in AI assistant with context awareness and codebase indexing
5. **Local object store**: Notebooks, workflows, environments and similar objects persist locally in a SQLite-backed store shaped like the cloud object model (`crates/cloud_objects`, `app/src/cloud_object/`)

### Development Guidelines

**Workspace Structure**:
- This is a Cargo workspace: `app` plus every crate under `crates/` (about 50 members)
- Main binary is in `app/`, UI framework in `crates/warpui/`
- Platform-specific code is conditionally compiled
- Integration tests are in `crates/integration/`

**Coding Style Preferences**:
- Avoid unnecessary type annotations, especially in closure params.
- Avoid using too many Rust path qualifiers and use imports for concision. Place import statements at the top of the file as per convention.
  An exception to this is inside cfg-guarded code branches. In those cases, you can either embed the import into the relevant scope or just use an absolute path for one-offs.
- If a function takes a context parameter (`AppContext`, `ViewContext`, or `ModelContext`), it should be named `ctx` and go last. The one exception is for
  functions that take a closure parameter, in which case the closure should be last.
- Always remove unused parameters completely rather than prefixing them with `_`. Update the function signature and all call sites accordingly.
- Prefer inline format arguments in macros like `println!`, `eprintln!`, and `format!` (for example, `eprintln!("{message}")` instead of `eprintln!("{}", message)`) to satisfy Clippy's `uninlined_format_args` lint.
- Do not pass `Itertools::format` results directly to logging macros (`log::*`, `safe_*`, etc.). `Itertools::format` produces a single-use formatter, while logging implementations may format a message more than once. Use a reusable `String` such as `iter.join(", ")` for logging arguments instead. Direct use in `format!` or `write!` is fine.
- When adding a toggleable setting, also add the matching Command Palette enable/disable entry and any required context flags so the setting is discoverable outside Settings.

**Comments**:
Comments have a cost. They carry a maintenance burden, because they must be kept in sync
with the code they describe. It is tempting to assume that more comments is always better,
but be judicious about when a comment is actually necessary because the code cannot speak
for itself.
- **Minimalist Comments**: Assume the reader is a Senior Software Engineer. Never comment
  to explain WHAT or HOW code works if self-documenting names accomplish that.
- **Strictly "Why" Only**: Reserve inline comments strictly for non-obvious business
  rationale, workarounds for third-party bugs, complex algorithms, unidiomatic code, or
  unexpected edge cases.
- **No Line-by-Line Narrations**: Never add comments restating the syntax (e.g., omit
  `# Initialize array`, `# Loop over users`).
- **Clean Docstrings**: Keep doc comments concise. Document public APIs, arguments, types,
  and returns. Do not narrate the method's internal implementation steps.
- **Single-source of documentation**: For items/members that have a doc comment explaining
  their purpose, you do not need to repeat that explanation anywhere else. A good example
  is a float const specifying an amount of spacing. You may use a doc comment on the
  declaration if necessary, but do not repeat that where the const is *referenced*. Another
  example is function call sites. Function doc comments explain what they do. Do not repeat
  the explanation at the call site.
- **Container docs describe the whole, member docs describe the parts**: A field's, variant's,
  or parameter's own doc comment is where that member gets explained. A container's item-level
  doc comment (struct, enum, trait) describes the item as a whole and must not enumerate or
  re-explain its members. Do not name members in the container's doc comment just to describe
  them, and do not restate in a member's doc comment what the container's doc comment already
  said. Behavior genuinely shared by several members belongs in exactly one place, not both.
- **Don't enumerate function call sites in doc comments**: Function doc comments should
  document their behavior and NOT their callers, e.g. it should never say things like,
  "this is used by [certain callers]" or "this is used when...".
- **No "transformation comments"**: Do not add comments that explain *your edits*. Comments
  only need explain the *current state* of the code. Explanations of edits belong in pull
  request comments instead. You shouldn't add comments with phrases like, "this used to do
  so-and-so".
- Do not remove existing comments when making unrelated changes. Only remove or modify a
  comment if the logic it describes has changed.
- The formatter (`./script/format`) is configured with a `max_width` (max line length) of
  100. Flow (reflow) comment line-wrapping to fill that full width rather than wrapping
  early at a narrower column, so comments span as few lines as possible.

**Terminal Model Locking**:
- Be extremely careful when calling `model.lock()` on the terminal model (`TerminalModel`). Acquiring multiple locks on the same model from different call sites can cause a deadlock, resulting in a UI freeze (beach ball on macOS).
- Before adding a new `model.lock()` call, verify that no caller in the current call stack already holds the lock.
- Prefer passing already-locked model references down the call stack rather than acquiring new locks.
- If you must lock the model, keep the lock scope as short as possible and avoid calling other functions that might also attempt to lock.

**Testing**:
- Use `cargo nextest` for parallel test execution
- Integration tests use the custom framework in `crates/integration/`.
- Tests should be run via presubmit script before submitting
- Unit tests should be placed in separate files using the naming convention `${filename}_tests.rs` or `mod_test.rs`
- Test files should be included at the end of their corresponding module with:
  ```rust
  #[cfg(test)]
  #[path = "filename_tests.rs"]  // or "mod_test.rs"
  mod tests;
  ```

**Pull Request Workflow**:
- **ALWAYS** run `./script/format` and `cargo clippy` (the versions specified in ./script/presubmit) before opening a PR or pushing updates to an existing PR branch
- Those commands must pass completely before creating or updating a pull request
- Specifically, ensure `./script/format` and `cargo clippy` checks pass
- If they fail, fix all issues before proceeding with the PR
- Do not create public pull requests or public issues that disclose a non-public security vulnerability. Refer users to `SECURITY.md` for the proper disclosure methods instead.
- This applies to:
  - Opening new pull requests
  - Pushing new commits to existing PR branches
  - Any branch updates that will be reviewed
- There is no PR template and no changelog convention: keep each PR to one logical change and
  explain what changed and why in the PR body, as `CONTRIBUTING.md` describes.

**Database**:
- Uses Diesel ORM with SQLite
- Migrations in `crates/persistence/migrations/`
- Schema defined in `crates/persistence/src/schema.rs`

**GraphQL**: there is no GraphQL client. The old `crates/graphql` crate (cynic client + schema SDL around the login flow) is gone; its locally-used types live on in `crates/cloud_objects` (`ServerTimestamp`) and `crates/ai` (`AgentTaskState`).

### Feature Flags

Warp uses compile-time feature flags with a small runtime plumbing layer.

How to add a feature flag:
- Add a new variant to the `FeatureFlag` enum in `crates/warp_features/src/lib.rs` (re-exported as
  `warp_core::features`)
- Gate code paths with `FeatureFlag::YourFlag.is_enabled()`
- To turn it on in a build, add a cargo feature in `app/Cargo.toml` that `app/src/features.rs`
  maps to the flag, and list it in the `simplewarp` feature set to ship it. That mapping is the
  only source of enabled flags (`compiled_features()`): `ChannelState::additional_features` adds
  nothing in the `simplewarp` bin except `DEBUG_FLAGS` in debug builds, and the settings schema and
  default-settings generators read the same mapping.

Best practices:
- **Prefer runtime checks over cfg directives**: Prefer `FeatureFlag::YourFlag.is_enabled()` over `#[cfg(...)]` compile-time directives so flags can be toggled without recompilation and are easier to clean up later. Use `#[cfg(...)]` only when the code cannot compile without them (for example, platform-specific code or dependencies that do not exist when the feature is disabled).
- Keep flags high-level and product-focused rather than per-call-site
- Remove the flag and dead branches after launch has stabilized
- For UI sections that expose a new feature, hide the UI behind the same flag

Example:
```rust
// crates/warp_features/src/lib.rs
#[derive(Sequence)]
pub enum FeatureFlag {
    YourNewFeature,
}
```

```toml
# app/Cargo.toml
your_new_feature = []
simplewarp = [
    "your_new_feature",
]
```

```rust
// app/src/features.rs
#[cfg(feature = "your_new_feature")]
FeatureFlag::YourNewFeature,

// Use in code
if FeatureFlag::YourNewFeature.is_enabled() {
    // gated behavior
}
```

### Exhaustive Matching

When adding/editing match statements, avoid using the wildcard _ when at all possible. Exhaustive matching is helpful for ensuring that all variants are handled, especially when adding new variants to enums in the future.
