use ai::api_keys::ApiKeyManager;
use markdown_parser::{FormattedText, FormattedTextFragment, FormattedTextLine};
use warp_core::ui::appearance::Appearance;
use warpui::elements::{
    ConstrainedBox, Container, CrossAxisAlignment, Flex, FormattedTextElement,
    HighlightedHyperlink, MainAxisAlignment, MainAxisSize, ParentElement,
};
use warpui::{AppContext, Element, Entity, SingletonEntity, View, ViewContext};

use crate::ai::AIRequestUsageModel;
use crate::ai::blocklist::error_color;
use crate::network::NetworkStatus;
use crate::ui_components::icons::Icon;
use crate::workspaces::user_workspaces::UserWorkspaces;

const NO_CONNECTION_PRIMARY_TEXT: &str = "No internet connection";

/// The alert has no actionable state left, so it never emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptAlertEvent {}

/// The alert state of the chip that appears to the right of certain parts of the prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptAlertState {
    /// The user is offline (no connection).
    NoConnection,
    /// No alert should be displayed.
    NoAlert,
}

pub struct PromptAlertView {
    state: PromptAlertState,
}

impl PromptAlertView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let request_usage_model = AIRequestUsageModel::handle(ctx);
        let user_workspaces = UserWorkspaces::handle(ctx);
        let network_status = NetworkStatus::handle(ctx);
        let api_key_manager = ApiKeyManager::handle(ctx);

        ctx.subscribe_to_model(&request_usage_model, |me, _, _, ctx| {
            me.state = Self::determine_state(ctx);
            ctx.notify();
        });

        ctx.subscribe_to_model(&user_workspaces, |me, _, _, ctx| {
            me.state = Self::determine_state(ctx);
            ctx.notify();
        });

        ctx.subscribe_to_model(&network_status, |me, _, _, ctx| {
            me.state = Self::determine_state(ctx);
            ctx.notify();
        });

        ctx.subscribe_to_model(&api_key_manager, |me, _, _, ctx| {
            me.state = Self::determine_state(ctx);
            ctx.notify();
        });

        Self {
            state: Self::determine_state(ctx),
        }
    }

    /// SimpleWarp has no request quota, so the only thing that can stop an AI
    /// request before it is sent is having no network at all.
    pub fn determine_state(app: &AppContext) -> PromptAlertState {
        if NetworkStatus::as_ref(app).is_online() {
            PromptAlertState::NoAlert
        } else {
            PromptAlertState::NoConnection
        }
    }

    pub fn is_no_alert(&self) -> bool {
        matches!(self.state, PromptAlertState::NoAlert)
    }

    pub fn state(&self) -> &PromptAlertState {
        &self.state
    }

    pub fn does_alert_block_ai_requests(app: &AppContext) -> bool {
        does_alert_block_ai_requests(&Self::determine_state(app))
    }

    fn primary_text(
        &self,
        state: &PromptAlertState,
        text_fragments: &mut Vec<FormattedTextFragment>,
    ) {
        // Add leading space to separate text from icon.
        //
        // Use this instead of hardcoded margin so it scales with font size and is consistent
        // with the space between this primary fragment and the option hyperlink fragment.
        text_fragments.push(FormattedTextFragment::plain_text("  "));
        match state {
            PromptAlertState::NoConnection => {
                text_fragments.push(FormattedTextFragment::plain_text(
                    NO_CONNECTION_PRIMARY_TEXT,
                ));
            }
            PromptAlertState::NoAlert => {}
        }
    }
}

fn does_alert_block_ai_requests(state: &PromptAlertState) -> bool {
    match state {
        PromptAlertState::NoAlert => false,
        PromptAlertState::NoConnection => true,
    }
}

impl Entity for PromptAlertView {
    type Event = PromptAlertEvent;
}

impl View for PromptAlertView {
    fn ui_name() -> &'static str {
        "PromptAlertView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let state = Self::determine_state(app);
        let mut text_fragments = vec![];

        self.primary_text(&state, &mut text_fragments);

        let formatted_text_element = FormattedTextElement::new(
            FormattedText::new([FormattedTextLine::Line(text_fragments)]),
            appearance.ui_font_size(),
            appearance.ui_font_family(),
            appearance.ui_font_family(),
            error_color(appearance.theme()),
            HighlightedHyperlink::default(),
        )
        .with_line_height_ratio(1.)
        .with_no_text_wrapping()
        .finish();

        let icon_size = appearance.ui_font_size();

        let mut chip_row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::End);
        if does_alert_block_ai_requests(&self.state) {
            chip_row.add_child(
                ConstrainedBox::new(
                    Icon::AlertTriangle
                        .to_warpui_icon(error_color(appearance.theme()).into())
                        .finish(),
                )
                .with_width(icon_size)
                .with_height(icon_size)
                .finish(),
            )
        }

        chip_row.add_child(formatted_text_element);

        Container::new(chip_row.finish())
            .with_margin_right(16.)
            .finish()
    }
}

#[cfg(test)]
#[path = "prompt_alert_tests.rs"]
mod tests;
