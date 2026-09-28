//! Plain-data option lists for the orchestration configuration fields:
//! harness and model. One builder per field turns the live catalogs into an
//! [`OptionSnapshot`] — rows, ordering, disabled reasons, load state, and
//! selection — and the pickers render from that snapshot.

use warp_cli::agent::Harness;
use warpui::{AppContext, SingletonEntity};

use super::config_state::OrchestrationConfigState;
use crate::LLMPreferences;
use crate::ai::harness_availability::HarnessAvailabilityModel;
use crate::ai::harness_display;
use crate::ai::local_harness_setup::{
    LocalHarnessSetupState, local_harness_is_product_enabled, local_harness_setup_state,
};

const DEFAULT_MODEL_LABEL: &str = "Default model";

/// One selectable row in an option snapshot. Carries no GUI types.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionRow {
    pub id: String,
    pub label: String,
    /// Harness identifier for rows representing harnesses; the picker maps
    /// it to an icon and brand color.
    pub harness: Option<Harness>,
    pub disabled_reason: Option<String>,
}

impl OptionRow {
    /// Creates an enabled row with no harness.
    fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            harness: None,
            disabled_reason: None,
        }
    }
}

/// Load state of the catalog backing a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionSourceStatus {
    Ready,
    Empty { message: String },
}

/// A complete option list for one configuration field.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionSnapshot {
    pub rows: Vec<OptionRow>,
    pub selected_id: Option<String>,
    pub status: OptionSourceStatus,
}

impl OptionSnapshot {
    /// A `Ready` snapshot.
    fn ready(rows: Vec<OptionRow>, selected_id: Option<String>) -> Self {
        Self {
            rows,
            selected_id,
            status: OptionSourceStatus::Ready,
        }
    }
}

// ── Harness ─────────────────────────────────────────────────────────

/// Server-provided harness entry decoupled from `HarnessAvailability` so
/// the pure row builder can be tested directly.
struct HarnessEntryInput {
    harness: Harness,
    display_name: String,
    enabled: bool,
}

/// Builds the harness options: filtering, ordering, disabled reasons, and
/// selection matching.
pub fn harness_snapshot(state: &OrchestrationConfigState, ctx: &AppContext) -> OptionSnapshot {
    let availability = HarnessAvailabilityModel::as_ref(ctx);
    let entries: Vec<HarnessEntryInput> = availability
        .available_harnesses()
        .iter()
        .map(|entry| HarnessEntryInput {
            harness: entry.harness,
            display_name: entry.display_name.clone(),
            enabled: entry.enabled,
        })
        .collect();
    let target_display = Harness::parse_orchestration_harness(&state.harness_type)
        .map(|harness| availability.display_name_for(harness).to_string());
    build_harness_snapshot(
        entries,
        &state.harness_type,
        target_display,
        &local_harness_setup_state,
    )
}

/// Pure core of [`harness_snapshot`]. `setup_state_for` is injected so
/// tests don't depend on locally installed CLIs.
fn build_harness_snapshot(
    entries: Vec<HarnessEntryInput>,
    initial_harness: &str,
    target_display: Option<String>,
    setup_state_for: &dyn Fn(Harness) -> LocalHarnessSetupState,
) -> OptionSnapshot {
    let resolve_entry_harness = |harness: Harness, display_name: &str| match harness {
        Harness::Unknown => [
            Harness::Oz,
            Harness::Claude,
            Harness::OpenCode,
            Harness::Gemini,
            Harness::Codex,
        ]
        .into_iter()
        .find(|candidate| harness_display::display_name(*candidate) == display_name)
        .unwrap_or(Harness::Unknown),
        harness => harness,
    };
    let setup_is_ready = |harness: Harness| setup_state_for(harness).is_selectable();

    // Sort selectable harnesses before disabled ones, preserving
    // relative order within each group.
    // Filter out Gemini — it is not yet supported as a multi-agent
    // harness and causes an infinite "Spawning agents" hang.
    let mut sorted: Vec<_> = entries
        .iter()
        .filter(|entry| {
            let harness = resolve_entry_harness(entry.harness, &entry.display_name);
            harness != Harness::Gemini && local_harness_is_product_enabled(harness)
        })
        .collect();
    sorted.sort_by_key(|entry| {
        let harness = resolve_entry_harness(entry.harness, &entry.display_name);
        !(entry.enabled && setup_is_ready(harness))
    });

    let mut rows: Vec<OptionRow> = Vec::new();
    let mut selected_id: Option<String> = None;
    for entry in sorted {
        let harness = resolve_entry_harness(entry.harness, &entry.display_name);
        let harness_str = harness.to_string();
        let selectable = entry.enabled && setup_is_ready(harness);
        let disabled_reason = if selectable {
            None
        } else {
            Some(
                match setup_state_for(harness) {
                    LocalHarnessSetupState::MissingHarness { tooltip } => tooltip,
                    LocalHarnessSetupState::ProductDisabled { message } => message,
                    LocalHarnessSetupState::Ready => "Disabled by your administrator",
                }
                .to_string(),
            )
        };
        // Match by harness string first, then fall back to matching
        // the display_name against the client-side name for the target
        // harness. This handles stale cache entries where entry.harness
        // is Unknown but entry.display_name is still correct.
        if selected_id.is_none() {
            if harness_str.eq_ignore_ascii_case(initial_harness) {
                selected_id = Some(harness_str.clone());
            } else if let Some(target_display) = &target_display
                && &entry.display_name == target_display
            {
                selected_id = Some(harness_str.clone());
            }
        }
        rows.push(OptionRow {
            id: harness_str,
            // Use the server-provided display_name for the label so stale
            // cache entries (where harness deserializes as Unknown) still
            // show the correct name.
            label: entry.display_name.clone(),
            harness: Some(harness),
            disabled_reason,
        });
    }
    if rows.is_empty() {
        return OptionSnapshot {
            rows,
            selected_id,
            status: OptionSourceStatus::Empty {
                message: "No harnesses available".to_string(),
            },
        };
    }
    OptionSnapshot::ready(rows, selected_id)
}

// ── Model ───────────────────────────────────────────────────────────

/// A model choice already resolved to plain strings.
struct ModelChoiceInput {
    id: String,
    label: String,
    disabled_reason: Option<String>,
}

/// Builds the model options for the active harness, in three catalog
/// branches:
/// - **Oz / empty**: the Warp LLM catalog (auto, then custom, then other
///   models).
/// - **Codex**: only a "Default model" entry.
/// - **Other non-Oz harnesses**: "Default model" plus the harness model
///   catalog.
pub fn model_snapshot(state: &OrchestrationConfigState, ctx: &AppContext) -> OptionSnapshot {
    let harness = Harness::parse_orchestration_harness(&state.harness_type);
    match harness {
        Some(Harness::Oz) | None => oz_model_snapshot(&state.model_id, ctx),
        Some(Harness::Codex) => {
            // Local Codex: only "Default model" entry.
            OptionSnapshot::ready(
                vec![OptionRow::new(String::new(), DEFAULT_MODEL_LABEL)],
                Some(String::new()),
            )
        }
        Some(harness) => {
            let models = HarnessAvailabilityModel::as_ref(ctx)
                .models_for(harness)
                .map(|models| {
                    models
                        .iter()
                        .map(|model| ModelChoiceInput {
                            id: model.id.clone(),
                            label: model.display_name.clone(),
                            disabled_reason: None,
                        })
                        .collect::<Vec<_>>()
                });
            build_non_oz_model_snapshot(models, &state.model_id)
        }
    }
}

/// Builds the Oz model options, including custom models backed by local
/// endpoints.
fn oz_model_snapshot(selected_model_id: &str, ctx: &AppContext) -> OptionSnapshot {
    let llm_prefs = LLMPreferences::as_ref(ctx);
    let (auto_models, rest): (Vec<_>, Vec<_>) = llm_prefs
        .get_base_llm_choices_for_agent_mode()
        .partition(|llm| llm.id.as_str().starts_with("auto"));
    let (custom_models, other_models): (Vec<_>, Vec<_>) = rest
        .into_iter()
        .partition(|llm| llm_prefs.custom_llm_info_for_id(&llm.id).is_some());
    let choices = auto_models
        .into_iter()
        .chain(custom_models)
        .chain(other_models)
        .map(|llm| ModelChoiceInput {
            id: llm.id.to_string(),
            label: llm.menu_display_name(),
            disabled_reason: llm
                .disable_reason
                .as_ref()
                .map(|reason| reason.tooltip_text().to_string()),
        })
        .collect();
    build_oz_model_snapshot(choices, selected_model_id)
}

/// Pure core for the Oz / unset branch of [`model_snapshot`].
fn build_oz_model_snapshot(
    choices: Vec<ModelChoiceInput>,
    initial_model_id: &str,
) -> OptionSnapshot {
    let selected_id = choices
        .iter()
        .find(|choice| choice.id == initial_model_id)
        .map(|choice| choice.id.clone());
    let rows: Vec<OptionRow> = choices
        .into_iter()
        .map(|choice| OptionRow {
            disabled_reason: choice.disabled_reason,
            ..OptionRow::new(choice.id, choice.label)
        })
        .collect();
    if rows.is_empty() {
        return OptionSnapshot {
            rows,
            selected_id,
            status: OptionSourceStatus::Empty {
                message: "No models available".to_string(),
            },
        };
    }
    OptionSnapshot::ready(rows, selected_id)
}

/// Pure core for the non-Oz branch of [`model_snapshot`]: "Default model"
/// on top, then server-provided models. Unknown or empty selections fall
/// back to "Default model" (empty id).
fn build_non_oz_model_snapshot(
    models: Option<Vec<ModelChoiceInput>>,
    initial_model_id: &str,
) -> OptionSnapshot {
    let mut rows = vec![OptionRow::new(String::new(), DEFAULT_MODEL_LABEL)];
    let mut found_initial = false;
    for model in models.into_iter().flatten() {
        if model.id == initial_model_id {
            found_initial = true;
        }
        rows.push(OptionRow::new(model.id, model.label));
    }
    let selected_id = if !initial_model_id.is_empty() && found_initial {
        Some(initial_model_id.to_string())
    } else {
        Some(String::new())
    };
    OptionSnapshot::ready(rows, selected_id)
}

#[cfg(test)]
#[path = "snapshots_tests.rs"]
mod tests;
