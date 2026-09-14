# SimpleWarp release app build

Date: 2026-09-07

## Result
Built `target/release/SimpleWarp.app` via `script/bundle_simplewarp`.

- cargo: `Finished release profile [optimized + debuginfo] in 11m 31s`
- icon compiled (AppIcon.icns + Assets.car)
- ad-hoc codesigned (`codesign --force --deep -s -`)
- bundle id: `dev.simplewarp.SimpleWarp`
- version: 0.1.0

Not Apple-notarized. Local launch only; Gatekeeper may prompt on first open.

Working tree had uncommitted AI/orchestration/features edits; they are in this binary.

---

# Phase 4 (4be-4bg) — ObjectClient vertical done

Date: 2026-09-14

## Slices (all committed)
- 4be `18f710f`: attachment-upload orphan surface (6 files, +37/-342). nextest 8730/8724, six env only.
- 4bf `74d2160`: managed_secrets + auth-secret UI chain (44 files, +88/-4219). nextest 8688/8681, six env + 1 load flake.
- 4bg `8e9a39f`: ObjectClient + sync_queue + sharing/dialog + cloud_object_client crate (151 files, +812/-26569, 18 deleted). Provider down to 2 getters (auth + ai).

## 4bg acceptance repairs (all suite-caught)
1. CloudModel startup registration deleted with the queue block → every integration test died at launch. Restored (queue parts left out).
2. migrate/activate lost the UpdateManager-load wait → re-pointed at the syncer initial-load flag.
3. Notebook baton auto-grab waited on the dead load signal via `async {}` → open entered edit mode, toggle flipped back to view. Removed; open stays in view (HEAD-local behavior). Canary: mermaid backspace integration (fails pre-fix, passes on HEAD and after). Baton unit test re-pinned; helper yields once for render (command-block models build on LayoutUpdated).
4. profiles/notebook test graphs: syncer (flag-holder, sync off) + WarpDrivePrivacySettings singletons; `complete_initial_load_for_test` wears `cfg_attr(not(test), allow(dead_code))`.
5. Integration websocket names (4) removed from ui_tests.rs to match bin unregistration. `test_up_arrow_history` (ignored) left.

## 4bg acceptance (final)
- format clean; simplewarp bin check clean (lib 4 baseline warnings)
- clippy `-p warp --all-targets --tests`: 0 errors
- nextest workspace: 8564 run / 8558 passed — only the six known environmental (five ssh, palette)
- Ledger: simplify-specs/plan.md 4be/4bf/4bg entries ("Record rounds 4be-4bg" commit).

## Next candidates
- Remaining clients: AIClient (2226 lines, last), AuthClient. Then ServerApi/Provider/BaseClient, then crates (warp_server_client/auth, firebase, graphql).
- `CloudModel::mock` vs restored production registration is fine; `object_actions` unused-variable watch.
- `Duration` unused import in notebook_tests.rs (pre-existing 4bg warning, left).
