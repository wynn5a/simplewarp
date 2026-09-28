use warp_cli::agent::Harness;

use super::{
    DEFAULT_MODEL_LABEL, HarnessEntryInput, ModelChoiceInput, OptionSourceStatus,
    build_harness_snapshot, build_non_oz_model_snapshot, build_oz_model_snapshot,
};
use crate::ai::local_harness_setup::LocalHarnessSetupState;

fn entry(harness: Harness, display_name: &str, enabled: bool) -> HarnessEntryInput {
    HarnessEntryInput {
        harness,
        display_name: display_name.to_string(),
        enabled,
    }
}

fn all_ready(_harness: Harness) -> LocalHarnessSetupState {
    LocalHarnessSetupState::Ready
}

// ── Harness ─────────────────────────────────────────────────────────

#[test]
fn harness_snapshot_excludes_gemini_and_selects_initial() {
    let entries = vec![
        entry(Harness::Oz, "Warp", true),
        entry(Harness::Claude, "Claude Code", true),
        entry(Harness::Gemini, "Gemini", true),
    ];

    let snapshot = build_harness_snapshot(entries, "claude", None, &all_ready);

    let ids: Vec<&str> = snapshot.rows.iter().map(|r| r.id.as_str()).collect();
    assert!(!ids.contains(&"gemini"));
    assert_eq!(snapshot.selected_id.as_deref(), Some("claude"));
    assert_eq!(snapshot.status, OptionSourceStatus::Ready);
    assert!(snapshot.rows.iter().all(|r| r.harness.is_some()));
}

#[test]
fn harness_snapshot_filters_product_disabled_local_harness() {
    let entries = vec![
        entry(Harness::Oz, "Warp", true),
        entry(Harness::Codex, "Codex", true),
    ];

    // Local Codex is product-disabled (feature flag off in tests).
    let snapshot = build_harness_snapshot(entries, "oz", None, &all_ready);

    let ids: Vec<&str> = snapshot.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["oz"]);
}

#[test]
fn harness_snapshot_marks_missing_local_cli_disabled_and_sorts_last() {
    let entries = vec![
        entry(Harness::Claude, "Claude Code", true),
        entry(Harness::Oz, "Warp", true),
    ];
    let setup = |harness: Harness| match harness {
        Harness::Claude => LocalHarnessSetupState::MissingHarness {
            tooltip: "Install Claude Code to use this local harness.",
        },
        Harness::Oz | Harness::OpenCode | Harness::Gemini | Harness::Codex | Harness::Unknown => {
            LocalHarnessSetupState::Ready
        }
    };

    let snapshot = build_harness_snapshot(entries, "oz", None, &setup);

    let ids: Vec<&str> = snapshot.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["oz", "claude"]);
    assert_eq!(
        snapshot.rows[1].disabled_reason.as_deref(),
        Some("Install Claude Code to use this local harness.")
    );
}

#[test]
fn harness_snapshot_marks_server_disabled_entries() {
    let entries = vec![
        entry(Harness::Oz, "Warp", true),
        entry(Harness::Claude, "Claude Code", false),
    ];

    let snapshot = build_harness_snapshot(entries, "oz", None, &all_ready);

    assert_eq!(
        snapshot.rows[1].disabled_reason.as_deref(),
        Some("Disabled by your administrator")
    );
}

#[test]
fn harness_snapshot_matches_selection_by_display_name_for_stale_cache() {
    // Stale cache: harness deserialized as Unknown but display_name intact.
    let entries = vec![entry(Harness::Unknown, "Claude Code", true)];

    let snapshot = build_harness_snapshot(
        entries,
        "claude",
        Some("Claude Code".to_string()),
        &all_ready,
    );

    assert_eq!(snapshot.selected_id.as_deref(), Some("claude"));
}

// ── Model ───────────────────────────────────────────────────────────

fn model(id: &str, label: &str) -> ModelChoiceInput {
    ModelChoiceInput {
        id: id.to_string(),
        label: label.to_string(),
        disabled_reason: None,
    }
}

#[test]
fn oz_model_snapshot_empty_catalog_reports_empty_status() {
    let snapshot = build_oz_model_snapshot(Vec::new(), "auto");
    assert!(matches!(snapshot.status, OptionSourceStatus::Empty { .. }));
}
/// Disabled model metadata remains available to every snapshot consumer.
#[test]
fn oz_model_snapshot_carries_disabled_reason() {
    let mut disabled_model = model("unavailable", "Unavailable");
    disabled_model.disabled_reason = Some("This model is unavailable.".to_string());

    let snapshot = build_oz_model_snapshot(vec![disabled_model], "");

    assert_eq!(
        snapshot.rows[0].disabled_reason.as_deref(),
        Some("This model is unavailable.")
    );
}

#[test]
fn non_oz_model_snapshot_puts_default_first_and_selects_server_model() {
    let snapshot = build_non_oz_model_snapshot(
        Some(vec![model("opus", "Opus"), model("sonnet", "Sonnet")]),
        "sonnet",
    );

    assert_eq!(snapshot.rows[0].label, DEFAULT_MODEL_LABEL);
    assert_eq!(snapshot.rows[0].id, "");
    assert_eq!(snapshot.selected_id.as_deref(), Some("sonnet"));
}

#[test]
fn non_oz_model_snapshot_falls_back_to_default_for_unknown_or_empty_id() {
    for initial in ["", "gone"] {
        let snapshot = build_non_oz_model_snapshot(Some(vec![model("opus", "Opus")]), initial);
        assert_eq!(snapshot.selected_id.as_deref(), Some(""));
    }
    // No server catalog at all: only the Default model row.
    let snapshot = build_non_oz_model_snapshot(None, "");
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.selected_id.as_deref(), Some(""));
}
