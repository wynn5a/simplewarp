//! This module contains common utilities for rendering Blocklist AI UI.
use std::sync::LazyLock;

use pathfinder_color::ColorU;
use warp_core::ui::appearance::Appearance;
use warpui::elements::{
    ConstrainedBox, Container, CrossAxisAlignment, Flex, MainAxisAlignment, MainAxisSize,
    MouseStateHandle, ParentElement,
};
use warpui::fonts::Weight;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::ui_components::text::Span;
use warpui::{AppContext, Element, EntityId, EventContext, SingletonEntity};

use crate::ai::agent::RenderableAIError;
use crate::themes::theme::{AnsiColorIdentifier, Fill, WarpTheme};
use crate::ui_components::icons::Icon;

const PROVIDER_BUTTON_ICON_SIZE: f32 = 14.;
const PROVIDER_BUTTON_ICON_TEXT_GAP: f32 = 8.;
const ERROR_APOLOGY_TEXT: &str = "I'm sorry, I couldn't complete that request.";
const INTERNAL_WARP_ERROR: &str = "Internal SimpleWarp error.";
/// Text to use as a label throughout the app for user interactions that will attach selected
/// block(s) or text selections to a new AI query.
pub static ATTACH_AS_AGENT_MODE_CONTEXT_TEXT: LazyLock<&'static str> =
    LazyLock::new(|| "Attach as agent context");

/// Label we use for the the command palette action to create a new local Warp Agent pane.
pub static NEW_AGENT_PANE_LABEL: LazyLock<&'static str> = LazyLock::new(|| "New Agent Pane");

/// Claude/Anthropic brand color (official brand orange #D97757).
/// Reference: https://github.com/anthropics/skills/blob/main/skills/brand-guidelines/SKILL.md
pub const CLAUDE_ORANGE: ColorU = ColorU {
    r: 0xD9,
    g: 0x77,
    b: 0x57,
    a: 0xFF,
};

/// Returns the color to be used for various AI signifiers
/// input with AI mode).
pub fn ai_brand_color(theme: &WarpTheme) -> ColorU {
    AnsiColorIdentifier::Magenta
        .to_ansi_color(&theme.terminal_colors().normal)
        .into()
}

/// Returns the color to be used for error UI throughout Agent Mode (like the "request limit
/// exceeded" chip).
pub fn error_color(theme: &WarpTheme) -> ColorU {
    AnsiColorIdentifier::Red
        .to_ansi_color(&theme.terminal_colors().normal)
        .into()
}
/// Renderer-neutral content for a failed Agent Mode request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailedOutputPresentation {
    Message(String),
    InvalidApiKey { title: &'static str, detail: String },
    ContextWindowExceeded { message: String },
}

/// Returns the user-facing presentation for an Agent Mode request failure.
///
/// Recovery-pending failures are intentionally suppressed so callers cannot accidentally render
/// an alarming terminal error while an automatic resume is still in flight.
pub fn failed_output_presentation(
    error: &RenderableAIError,
    _app: &AppContext,
) -> Option<FailedOutputPresentation> {
    if error.should_suppress_during_recovery() {
        return None;
    }

    Some(match error {
        RenderableAIError::QuotaLimit {
            user_display_message,
        } => {
            // SimpleWarp does not meter requests, so a quota error can only have
            // come from the configured provider. Show what it said, and do not
            // invent a Warp credit-reset date.
            let message = user_display_message.clone().unwrap_or_else(|| {
                "The AI provider reported that you have reached a rate or quota limit.".to_string()
            });
            FailedOutputPresentation::Message(format!("{ERROR_APOLOGY_TEXT}\n\n{message}"))
        }
        RenderableAIError::InternalWarpError => FailedOutputPresentation::Message(format!(
            "{ERROR_APOLOGY_TEXT}\n\n{INTERNAL_WARP_ERROR}"
        )),
        RenderableAIError::ContextWindowExceeded(message) => {
            FailedOutputPresentation::ContextWindowExceeded {
                message: message.clone(),
            }
        }
        RenderableAIError::InvalidApiKey {
            provider,
            model_name,
        } => FailedOutputPresentation::InvalidApiKey {
            title: "Provided API key is not valid",
            detail: format!(
                "Failed to authenticate with {provider} when using {model_name}. \
                 Double-check that your API key is correct."
            ),
        },
        RenderableAIError::TransientNetworkError { .. } => {
            FailedOutputPresentation::Message(error.to_string())
        }
        RenderableAIError::Other { error_message, .. } => {
            FailedOutputPresentation::Message(format!("{ERROR_APOLOGY_TEXT}\n\n{error_message}"))
        }
        RenderableAIError::AgentExitedShell { .. } => {
            FailedOutputPresentation::Message(format!("{ERROR_APOLOGY_TEXT}\n\n{error}"))
        }
        // Cloud startup failures surface the raw server message directly, matching the
        // dedicated GUI error card which shows the message without an apology prefix.
        RenderableAIError::CloudStartupFailed(msg) => {
            FailedOutputPresentation::Message(msg.clone())
        }
    })
}

/// Whether a failed Agent Mode response should show the debug footer.
pub fn should_show_failed_output_debug_footer(
    error: &RenderableAIError,
    is_latest_visible_exchange_in_root_task: bool,
    has_expanded_last_requested_command: bool,
    is_restored: bool,
) -> bool {
    !error.should_suppress_during_recovery()
        && is_latest_visible_exchange_in_root_task
        && !has_expanded_last_requested_command
        && !is_restored
        && !error.is_invalid_api_key()
}

/// Returns the size to be used for the AI icon in AI output blocks and the terminal input when in
/// AI mode.
///
/// This size is computed based on the user's current font size and line height ratio, such that the
/// size of the icon matches the user's text line height.  This is necessary because the AI icon in
/// the input is rendered next to text in the editor.
pub fn ai_indicator_height(app: &AppContext) -> f32 {
    let appearance = Appearance::as_ref(app);
    app.font_cache().line_height(
        appearance.monospace_font_size(),
        appearance.line_height_ratio(),
    )
}

/// Returns the saved position ID of the attached blocks chip inside the [`AIBlock`] header.
pub fn get_attached_blocks_chip_element_position_id(view_id: EntityId) -> String {
    format!("aiblock:{view_id}.attached_block_chip_position")
}

/// Returns the saved position ID of the overflow menu inside the [`AIBlock`] header.
pub fn get_ai_block_overflow_menu_element_position_id(view_id: EntityId) -> String {
    format!("aiblock:{view_id}.overflow_menu_position")
}

/// Renders a secondary button with an MCP/skill provider icon and a text label.
pub(crate) fn render_provider_icon_button<F>(
    button_label: &str,
    button_handle: MouseStateHandle,
    appearance: &Appearance,
    icon: Icon,
    color: Fill,
    on_click: F,
) -> Box<dyn Element>
where
    F: FnMut(&mut EventContext) + 'static,
{
    let theme = appearance.theme();
    let font_color = theme.foreground().into_solid();
    let mut label_children = vec![
        ConstrainedBox::new(icon.to_warpui_icon(color).finish())
            .with_width(PROVIDER_BUTTON_ICON_SIZE)
            .with_height(PROVIDER_BUTTON_ICON_SIZE)
            .finish(),
    ];
    label_children.push(
        Container::new(
            Span::new(
                button_label.to_string(),
                UiComponentStyles {
                    font_family_id: Some(appearance.ui_font_family()),
                    font_size: Some(appearance.ui_font_size()),
                    font_weight: Some(Weight::Semibold),
                    font_color: Some(font_color),
                    ..Default::default()
                },
            )
            .build()
            .finish(),
        )
        .with_padding_left(PROVIDER_BUTTON_ICON_TEXT_GAP)
        .finish(),
    );
    let label = Flex::row()
        .with_children(label_children)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_main_axis_alignment(MainAxisAlignment::Center)
        .with_main_axis_size(MainAxisSize::Min)
        .finish();
    let mut on_click = on_click;
    appearance
        .ui_builder()
        .button(ButtonVariant::Secondary, button_handle)
        .with_custom_label(label)
        .with_style(UiComponentStyles {
            font_weight: Some(Weight::Semibold),
            ..Default::default()
        })
        .build()
        .on_click(move |ctx, _, _| {
            on_click(ctx);
        })
        .finish()
}
