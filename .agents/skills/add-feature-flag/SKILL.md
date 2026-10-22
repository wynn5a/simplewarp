---
name: add-feature-flag
description: Add a new feature flag to gate code changes in the SimpleWarp codebase.
---

# add-feature-flag

Add a new feature flag to gate code changes in the SimpleWarp codebase.

## Overview

Feature flags are compile-time cargo features with a small runtime plumbing layer: a cargo feature in `app/Cargo.toml` maps to a `FeatureFlag` variant in `app/src/features.rs`, and code checks `FeatureFlag::X.is_enabled()` at runtime. The `simplewarp` feature set (the app's only feature set; `default` forwards to it) decides what ships.

The only runtime source of enabled flags is that mapping. `ChannelState::additional_features` is empty in the `simplewarp` bin apart from `DEBUG_FLAGS` in debug builds, and the `RELEASE_FLAGS` / `DOGFOOD_FLAGS` / `PREVIEW_FLAGS` lists in `crates/warp_features/src/lib.rs` are read only by the settings schema and default-settings generators' `--channel` switch. Do not add a flag to those lists expecting it to turn on.

## Steps

### 1. Add to Cargo.toml
Add the feature to `app/Cargo.toml` under the `[features]` section:

```toml
[features]
your_feature_name = []
```

### 2. Add to FeatureFlag enum
Add a new variant to the `FeatureFlag` enum in `crates/warp_features/src/lib.rs` (re-exported as `warp_core::features`):

```rust
#[derive(Sequence)]
pub enum FeatureFlag {
    YourFeatureName,
}
```

### 3. Map the cargo feature to the flag
Add the pair to the list in `enabled_features()` in `app/src/features.rs`:

```rust
#[cfg(feature = "your_feature_name")]
FeatureFlag::YourFeatureName,
```

### 4. Gate code with runtime checks
In your code, use the runtime check to conditionally execute feature-gated code:

```rust
if FeatureFlag::YourFeatureName.is_enabled() {
    // feature-gated behavior
}
```

### 5. Running with the flag on
To test locally with the feature enabled without shipping it:

```bash
cargo run --features your_feature_name

# Multiple features:
cargo run --features your_feature_name,another_feature
```

### 6. Ship it
When the feature should be on in every build, add it to the `simplewarp` feature set in `app/Cargo.toml`:

```toml
[features]
simplewarp = [
    "your_feature_name",
    # other shipped features...
]
```

`features::tests` in `app/src/features_tests.rs` pins some shipped flags; add a pin when the flag must never silently drop out of the build.

## Keybindings with Feature Flags

If adding an `EditableBinding` or `FixedBinding` that's part of a gated feature, include an enabled predicate that checks the feature flag. This prevents the keybinding from appearing in keyboard settings when the feature is disabled.

Example:
```rust
EditableBinding::new(
    "action:name",
    "Action description",
    YourAction::Variant
)
.with_enabled(|| FeatureFlag::YourFeatureName.is_enabled())
.with_key_binding("cmdorctrl-key")
```

## Best Practices

- **Prefer runtime checks over cfg directives**: Use `FeatureFlag::YourFeatureName.is_enabled()` instead of `#[cfg(...)]` when possible, so flags can be toggled without recompilation and are easier to clean up later
- Use `#[cfg(...)]` only when code cannot compile without the flag (e.g., platform-specific code or missing dependencies)
- Keep flags high-level and product-focused rather than per-call-site
- Remove flags and dead branches after the feature has stabilized (see `remove-feature-flag`)
- When a flag gates a toggleable setting, also add the matching Command Palette enable/disable entry so the setting is discoverable outside Settings
