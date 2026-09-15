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
- SCOPE CORRECTION (2026-09-15, user): delete only features that require a remote service. Fully-local features behind constant-false flags stay untouched (a 4bq JupyterNotebookRendering attempt was reverted pre-commit) and are at most enable-in-simplewarp candidates, decided per feature.
- ENABLED (2026-09-15): jupyter_notebook_rendering added to the simplewarp set — .ipynb now opens in the notebook viewer. Pinning test features::tests::jupyter_notebook_rendering_is_compiled_into_the_build guards it. warp lib 5212 simplewarp. Remaining enable-candidates: EditableMarkdownMermaid, ImeMarkedText, ITermImages.
- Flag verticals (4bp, 2026-09-14): WarpControlCli folded, 69 files +56/−12,194. Ledger: plan.md 4bp entry. NOTE: WarpControlCli was also local-only (loopback control server + local CLI) — deletable-history if the user wants it back: revert 3fa529040.
- Remote-dependent constant-false flags done (4br–4bu, 2026-09-15): Autoupdate, RemoteCodebaseIndexing, PromptSuggestionsViaMAA, PredictAMQueries — queue empty. Leftovers: AgentHarness flag (11 sites, live-by-design per 4at, careful).
- Client chain: AIClient 32 methods (4bv took 2: get_feature_model_choices, get_free_available_models), AuthClient 12 methods, then ServerApi/Provider/BaseClient. Next: login-gated refresh group (get_request_limit_info, get_available_harnesses, list_connected_self_hosted_workers) + zero-caller ServerApi stubs (server_time, fetch_channel_versions).
- Client chain blocked on feature rounds: AIClient 34 methods, AuthClient 12 methods, then ServerApi/Provider/BaseClient — all remaining methods have live callers (surveys in ledger); they shrink only alongside feature rounds.
- AIClient wave done through 4bm (2026-09-14): 62 → 34 methods. Slices: 4bh zero-caller walls (7), 4bi memory-store CLI (9), 4bj named-agent mgmt CLI (9), 4bk observability posting (2), 4bl skills CLI (1). Ledger: plan.md 4bh–4bm entry. Remaining 34 all need feature rounds (survey in ledger).
- `CloudModel::mock` vs restored production registration is fine; `object_actions` unused-variable watch.
- `Duration` unused import in notebook_tests.rs (pre-existing 4bg warning, left).
