# SimpleWarp simplification records

Historical records of the simplification that turned Warp's open-source client into SimpleWarp.
**Current state lives in [plan.md](plan.md) (Status / Next) and in the spec graph under
[`../specs/`](../specs/SPEC.md)**; everything else here documents how the code got there and is not
kept up to date.

| File | Contents |
| --- | --- |
| [plan.md](plan.md) | Main ledger: decisions, phase status, per-round results through 4id. |
| [plan-p2.md](plan-p2.md) | Survey (2026-09-29) of what still depended on a remote service, before the second deletion phase. |
| [plan-p3.md](plan-p3.md) | Dead-code audit (2026-10-11): cross-crate public API, settings/actions, whole modules. |
| [plan-p4.md](plan-p4.md) | Second dead-code sweep (2026-10-17), mechanical pass: unused deps, allow→warn probe, orphan files, features. |
| [plan-p4b.md](plan-p4b.md) | Second sweep, semantic pass: public API, actions/menus, DB tables, assets, every `allow(dead_code)` site. |
| [plan-p4c.md](plan-p4c.md) | Post-P4b fixpoint (2026-10-19): crates-side leftovers, annotation pass, dependency re-check, decision round. |
| [plan-osc7501.md](plan-osc7501.md) | Design note for the OSC 7501 program-status protocol implementation. |
| [progress-log.md](progress-log.md) | Scratch progress log through 2026-09-14; superseded by `plan.md`, kept for its acceptance notes. |
