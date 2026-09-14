# SimpleWarp release app build

## Git push identity (standing decision, 2026-09-14)
- Push as `wynn5a`, then switch back to `fuwenming-pacvue`.
- Both accounts live in `gh auth` (keyring); remote is HTTPS + osxkeychain (holds fuwenming's credential, so a plain push 403s).
- Flow (touches nothing stored): `gh auth switch --hostname github.com --user wynn5a`, then `git -c credential.helper='!gh auth git-credential' push origin master`, then switch back to `fuwenming-pacvue` and verify with `gh auth status`.
- Never add a remote pointing at `warpdotdev/warp`; push to `origin` only.

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
- Flag verticals resumed (4bo, 2026-09-14): CLIAgentRichInput folded, 48 files +1,065/−5,220 — input-session half of CLIAgentSessionsModel, composer flow, Ctrl-G binding, 4 settings, 3 telemetry events, hide_cursor_cell render plumbing. 98 constant-false flags remain of 208; next by site count: WarpControlCli (24), GeminiEnterprise (23, live-by-design per 4at), EditableMarkdownMermaid (23). Ledger: plan.md 4bo entry.
- Client chain blocked on feature rounds: AIClient 34 methods, AuthClient 12 methods, then ServerApi/Provider/BaseClient — all remaining methods have live callers (surveys in ledger); they shrink only alongside feature rounds.
- AIClient wave done through 4bm (2026-09-14): 62 → 34 methods. Slices: 4bh zero-caller walls (7), 4bi memory-store CLI (9), 4bj named-agent mgmt CLI (9), 4bk observability posting (2), 4bl skills CLI (1). Ledger: plan.md 4bh–4bm entry. Remaining 34 all need feature rounds (survey in ledger).
- `CloudModel::mock` vs restored production registration is fine; `object_actions` unused-variable watch.
- `Duration` unused import in notebook_tests.rs (pre-existing 4bg warning, left).
