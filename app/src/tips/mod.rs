use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use settings::Setting as _;
use warp_errors::report_if_error;
use warpui::keymap::Keystroke;
use warpui::{AppContext, Entity, SingletonEntity};

pub mod tip_view;
pub use tip_view::{TipsEvent, TipsView};

use crate::terminal::general_settings::GeneralSettings;
use crate::util::bindings::trigger_to_keystroke;

#[derive(Clone, Copy, Debug, Hash, PartialEq, std::cmp::Eq, Serialize, Deserialize)]
pub enum WelcomeTipFeature {
    Workflows,
    CommandPalette,
    SplitPane,
    ThemePicker,
    HistorySearch,
    AiCommandSearch,
}

pub const WELCOME_TIP_FEATURE_LENGTH: usize = 6;

impl WelcomeTipFeature {
    pub fn editable_binding_name(&self) -> &'static str {
        match self {
            WelcomeTipFeature::Workflows => "input:toggle_workflows",
            WelcomeTipFeature::CommandPalette => "workspace:toggle_command_palette",
            WelcomeTipFeature::SplitPane => "pane_group:add_right",
            WelcomeTipFeature::HistorySearch => "input:search_command_history",
            WelcomeTipFeature::AiCommandSearch => "input:toggle_natural_language_command_search",
            WelcomeTipFeature::ThemePicker => "workspace:show_theme_chooser",
        }
    }

    pub fn keyboard_shortcut(&self, ctx: &mut AppContext) -> Option<Keystroke> {
        ctx.editable_bindings()
            .find(|binding| binding.name == self.editable_binding_name())
            .and_then(|binding| trigger_to_keystroke(binding.trigger))
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    Hash,
    PartialEq,
    std::cmp::Eq,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "A welcome tip shown to new users.",
    rename_all = "snake_case"
)]
pub enum Tip {
    #[schemars(description = "A non-interactive informational hint.")]
    Hint(TipHint),
    #[schemars(description = "An interactive tip that triggers an action when clicked.")]
    Action(TipAction),
}

// Tips that aren't clickable to dispatch an action
#[derive(
    Clone,
    Copy,
    Debug,
    Hash,
    PartialEq,
    std::cmp::Eq,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(description = "A non-interactive tip hint.", rename_all = "snake_case")]
pub enum TipHint {
    CreateBlock,
    BlockSelect,
    BlockAction,
}

// Tips that are clickable and dispatch an action
#[derive(
    Clone,
    Copy,
    Debug,
    Hash,
    PartialEq,
    std::cmp::Eq,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(description = "An interactive tip action.", rename_all = "snake_case")]
pub enum TipAction {
    CommandPalette,
    SplitPane,
    ThemePicker,
    HistorySearch,
    CommandSearch,
    AiCommandSearch,
    SaveNewLaunchConfig,
    WarpAI,
    // This toggles Warp Drive rather than opening it. This enum can't directly be
    // renamed because we serialize it into the welcome tips.
    OpenWarpDrive,
    // Note that these items have been deprecated from the UI and are not in any section.
    // We are leaving them in this enum to ensure that we don't re-use their values. Since
    // old clients will have them in their user defaults, we want to prevent future usage
    // of these enum values.
    Changelog,
    Workflows,
}

impl TipAction {
    pub fn editable_binding_name(&self) -> &'static str {
        match self {
            TipAction::CommandPalette => "workspace:toggle_command_palette",
            TipAction::SplitPane => "pane_group:add_right",
            TipAction::HistorySearch => "input:search_command_history",
            TipAction::CommandSearch => "workspace:show_command_search",
            TipAction::AiCommandSearch => "input:toggle_natural_language_command_search",
            TipAction::ThemePicker => "workspace:show_theme_chooser",
            TipAction::SaveNewLaunchConfig => "workspace:open_launch_config_save_modal",
            TipAction::WarpAI => "workspace:toggle_ai_assistant",
            TipAction::OpenWarpDrive => "workspace:toggle_left_panel",
            // Deprecated variants. No binding is registered under these names, so the
            // lookup in `keyboard_shortcut` simply finds nothing.
            TipAction::Changelog => "/changelog",
            TipAction::Workflows => "input:toggle_workflows",
        }
    }

    pub fn keyboard_shortcut(&self, ctx: &mut AppContext) -> Option<Keystroke> {
        ctx.editable_bindings()
            .find(|binding| binding.name == self.editable_binding_name())
            .and_then(|binding| trigger_to_keystroke(binding.trigger))
    }
}

#[derive(Default)]
pub struct TipsCompleted {
    pub features_used: HashSet<Tip>,
    pub skipped_or_completed: bool,
    pub gamified_tips_count: Option<usize>,
}

impl Entity for TipsCompleted {
    type Event = ();
}

impl TipsCompleted {
    pub fn new(features_used: HashSet<Tip>, skipped_or_completed: bool) -> Self {
        Self {
            features_used,
            skipped_or_completed,
            gamified_tips_count: None,
        }
    }

    /// Returns true if the feature previously wasn't used.
    pub fn mark_feature_used(&mut self, feature: Tip) -> bool {
        let is_new_value = self.features_used.insert(feature);

        // Check if all gamified tips are completed
        if let Some(total_tips) = self.gamified_tips_count
            && is_new_value
            && self.features_used.len() == total_tips
        {
            self.skipped_or_completed = true;
        }

        is_new_value
    }

    pub fn serialized_tips(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.features_used)
    }

    pub fn completed_count(&self) -> usize {
        self.features_used.len()
    }

    pub fn set_gamified_tips_count(&mut self, total: usize) {
        self.gamified_tips_count = Some(total)
    }
}

/// Marks the welcome tip as used, writes their current state to a cloud synced preference.
pub fn mark_feature_used_and_write_to_user_defaults(
    feature: Tip,
    tips_completed: &mut TipsCompleted,
    ctx: &mut AppContext,
) {
    if tips_completed.mark_feature_used(feature) {
        GeneralSettings::handle(ctx).update(ctx, |general_settings, ctx| {
            report_if_error!(
                general_settings
                    .welcome_tips_features_used
                    .set_value(tips_completed.features_used.clone(), ctx)
            );

            if tips_completed.skipped_or_completed {
                report_if_error!(
                    general_settings
                        .welcome_tips_skipped_or_completed
                        .set_value(true, ctx)
                );
            }
        });
    }
}

/// Updates the model to reflect welcome tips are skipped, writes to user defaults, and sends telemetry.
pub fn skip_tips_and_write_to_user_defaults(
    tips_completed: &mut TipsCompleted,
    ctx: &mut AppContext,
) {
    tips_completed.skipped_or_completed = true;
    GeneralSettings::handle(ctx).update(ctx, |general_settings, ctx| {
        report_if_error!(
            general_settings
                .welcome_tips_skipped_or_completed
                .set_value(true, ctx)
        );
    });
}
