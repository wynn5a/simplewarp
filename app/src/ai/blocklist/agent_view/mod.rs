pub(crate) mod agent_input_footer;
mod agent_message_bar;
mod agent_view_block;
mod controller;
mod conversation_selection;
mod ephemeral_message_model;
mod gui_input_mode_policy;
mod inline_agent_view_header;
pub mod shortcuts;
mod zero_state_block;

use std::sync::LazyLock;

pub use agent_input_footer::*;
pub use agent_message_bar::*;
pub use agent_view_block::*;
pub use controller::*;
pub(crate) use conversation_selection::AgentViewConversationSelection;
pub use ephemeral_message_model::*;
pub(crate) use gui_input_mode_policy::GuiInputModePolicy;
pub use inline_agent_view_header::*;
use pathfinder_color::ColorU;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::Fill;
use warpui::fonts::Properties;
use warpui::keymap::Keystroke;
pub use zero_state_block::*;

use crate::view_components::action_button::ActionButtonTheme;

pub static ENTER_AGENT_VIEW_NEW_CONVERSATION_KEYSTROKE: LazyLock<Keystroke> = LazyLock::new(|| {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            Keystroke {
                cmd: true,
                key: "enter".to_owned(),
                ..Default::default()
            }
        } else {
            Keystroke {
                ctrl: true,
                shift: true,
                key: "enter".to_owned(),
                ..Default::default()
            }
        }
    }
});

pub struct AgentViewHeaderTheme;

impl ActionButtonTheme for AgentViewHeaderTheme {
    fn background(&self, _: bool, _: &Appearance) -> Option<Fill> {
        None
    }

    fn text_color(
        &self,
        hovered: bool,
        background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        if hovered {
            appearance
                .theme()
                .main_text_color(background.unwrap_or(appearance.theme().background()))
                .into_solid()
        } else {
            appearance
                .theme()
                .sub_text_color(background.unwrap_or(appearance.theme().background()))
                .into_solid()
        }
    }

    fn font_properties(&self) -> Option<Properties> {
        Some(Properties::default())
    }

    fn keyboard_shortcut_background(&self, appearance: &Appearance) -> Option<ColorU> {
        Some(appearance.theme().surface_overlay_2().into_solid())
    }
}

pub struct AgentViewHeaderDisabledTheme;

impl ActionButtonTheme for AgentViewHeaderDisabledTheme {
    fn background(&self, _: bool, _: &Appearance) -> Option<Fill> {
        None
    }

    fn text_color(&self, _: bool, background: Option<Fill>, appearance: &Appearance) -> ColorU {
        appearance
            .theme()
            .disabled_text_color(background.unwrap_or(appearance.theme().background()))
            .into_solid()
    }

    fn keyboard_shortcut_background(&self, _: &Appearance) -> Option<ColorU> {
        None
    }

    fn font_properties(&self) -> Option<Properties> {
        Some(Properties::default())
    }
}
