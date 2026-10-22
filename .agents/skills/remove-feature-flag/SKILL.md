---
name: remove-feature-flag
description: Remove a feature flag after it has been rolled out and stabilized in the SimpleWarp codebase.
---

# remove-feature-flag

Remove a feature flag after it has been rolled out and stabilized in the SimpleWarp codebase.

## Overview

After a feature flag has shipped in the `simplewarp` feature set and has stabilized, the flag should be removed to reduce technical debt and simplify the codebase. This involves removing the flag definition and all conditional checks.

## When to Remove

Remove a feature flag when:
- The feature is in the `simplewarp` feature set in `app/Cargo.toml`
- The feature has been stable for a reasonable period
- There are no plans to disable the feature or provide configuration options
- The feature is considered permanent

## Steps

### 1. Remove from app/Cargo.toml
Remove the feature from both the `[features]` section and the `simplewarp` set:

```toml
[features]
simplewarp = [
    # Remove "your_feature_name" from here
]

# Remove this line:
# your_feature_name = []
```

### 2. Remove from FeatureFlag enum
Remove the variant from the `FeatureFlag` enum in `crates/warp_features/src/lib.rs`:

```rust
#[derive(Sequence)]
pub enum FeatureFlag {
    // Remove YourFeatureName,
}
```

### 3. Remove from app/src/features.rs
Remove the cargo-feature mapping in `compiled_features()`:

```rust
// Remove these lines:
// #[cfg(feature = "your_feature_name")]
// FeatureFlag::YourFeatureName,
```

Also remove any pin for the flag in `app/src/features_tests.rs`.

### 4. Remove all runtime checks and dead code
Find and remove all `FeatureFlag::YourFeatureName.is_enabled()` checks throughout the codebase:

**Before:**
```rust
if FeatureFlag::YourFeatureName.is_enabled() {
    // new behavior
} else {
    // old behavior (dead code)
}
```

**After:**
```rust
// new behavior (unconditionally enabled)
```

The shared `FeatureFlag` enum is used from library crates too, so search the whole workspace, not just `app/`:
```bash
rg "YourFeatureName" app/ crates/
```

### 5. Remove keybinding predicates
If the feature flag was used in keybinding enabled predicates, remove the predicate:

**Before:**
```rust
EditableBinding::new(
    "action:name",
    "Action description",
    YourAction::Variant
)
.with_enabled(|| FeatureFlag::YourFeatureName.is_enabled())
.with_key_binding("cmdorctrl-key")
```

**After:**
```rust
EditableBinding::new(
    "action:name",
    "Action description",
    YourAction::Variant
)
.with_key_binding("cmdorctrl-key")
```

### 6. Clean up dead code branches
Remove any code paths that were only executed when the feature was disabled (the `else` branches in feature checks). These are now dead code. If the flag gated a Command Palette enable/disable entry or a context flag, remove those too.

### 7. Run tests and validation
After removing the flag:

```bash
# Format and lint
./script/format
cargo clippy --workspace --exclude warp_completer --all-targets --tests -- -D warnings
cargo clippy -p warp --all-targets --tests -- -D warnings
cargo clippy -p warp_completer --all-targets --tests -- -D warnings

# Run tests
cargo nextest run --no-fail-fast --workspace

# Build and run the app
cargo run
```

## Best Practices

- Remove feature flags promptly after they're no longer needed to reduce technical debt
- When removing a flag, remove ALL related code (checks, dead branches, keybinding predicates)
- Use ripgrep to ensure you've found all occurrences
- Test thoroughly after removal to ensure no regressions
- Consider doing flag removal in a separate PR for easier review

## Example Search Commands

```bash
# Find all occurrences of the flag name across the workspace
rg "YourFeatureName" app/ crates/

# Find feature flag checks
rg "FeatureFlag::YourFeatureName" app/ crates/

# Find cfg attributes
rg 'cfg\(feature = "your_feature_name"\)' app/ crates/
```
