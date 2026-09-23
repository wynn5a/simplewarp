use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::Fill;
use warpui::elements::{
    Align, Border, ChildAnchor, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    DropShadow, Flex, FormattedTextElement, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    OffsetPositioning, ParentAnchor, ParentElement, ParentOffsetBounds, Radius, Stack,
};
use warpui::fonts::Weight;
use warpui::keymap::FixedBinding;
use warpui::platform::Cursor;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext};

use crate::auth::AuthStateProvider;
use crate::settings_view::SettingsSection;
use crate::ui_components::blended_colors;
use crate::workspace::WorkspaceAction;
use crate::workspaces::user_workspaces::UserWorkspaces;

const MODAL_WIDTH: f32 = 480.;
const CORNER_RADIUS: f32 = 12.;
const PANEL_PADDING: f32 = 24.;
const CLOSE_BUTTON_DIAMETER: f32 = 20.;

const NOTICE_TITLE_TEXT: &str = "Warp is no longer providing inference on the free plan.";
const NOTICE_BODY_TEXT: &str = "To keep using Warp's AI features, please upgrade to a paid plan, \
     or bring your own API key or endpoint.";
const NOTICE_BONUS_CREDITS_TEXT: &str = "If you have any unused bonus credits, AI will keep \
     working until these run out.";

const PROMPT_SUGGESTIONS_TITLE_TEXT: &str = "How to use AI features in Warp";
const PROMPT_SUGGESTIONS_BODY_TEXT: &str = "To use AI features in Warp, subscribe to a paid plan, \
     add an API key (OpenAI, Anthropic, or Google), or add a custom inference endpoint (OpenRouter, \
     LiteLLM).";

/// Which surface opened the modal. Selects the copy and disambiguates telemetry;
/// the layout and CTAs are identical across variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreeAiRemovalModalVariant {
    /// One-time notice shown to Free users when Warp-provided AI is removed from their plan.
    Notice,
    /// Shown on demand when a Free user activates Prompt Suggestions while out of credits.
    PromptSuggestions,
}

impl FreeAiRemovalModalVariant {
    fn title(self) -> &'static str {
        match self {
            Self::Notice => NOTICE_TITLE_TEXT,
            Self::PromptSuggestions => PROMPT_SUGGESTIONS_TITLE_TEXT,
        }
    }

    fn body(self) -> &'static str {
        match self {
            Self::Notice => NOTICE_BODY_TEXT,
            Self::PromptSuggestions => PROMPT_SUGGESTIONS_BODY_TEXT,
        }
    }

    /// Secondary note rendered under the body. The on-demand Prompt Suggestions
    /// variant only fires once the user is already out of credits, so the
    /// bonus-credits note doesn't apply there.
    fn secondary(self) -> Option<&'static str> {
        match self {
            Self::Notice => Some(NOTICE_BONUS_CREDITS_TEXT),
            Self::PromptSuggestions => None,
        }
    }
}

pub fn init(app: &mut AppContext) {
    use warpui::keymap::macros::*;

    app.register_fixed_bindings([FixedBinding::new(
        "escape",
        FreeAiRemovalModalAction::Close,
        id!("FreeAiRemovalModal"),
    )]);
}

#[derive(Clone, Debug)]
pub enum FreeAiRemovalModalAction {
    Close,
    SetUpByok,
    Upgrade,
}

#[derive(Clone, Copy, Debug)]
pub enum FreeAiRemovalModalEvent {
    Close,
}

#[derive(Default)]
struct StateHandles {
    close_button: MouseStateHandle,
    byok_button: MouseStateHandle,
    upgrade_button: MouseStateHandle,
}

/// Notice shown to Free-plan users about the removal of Warp-provided AI. The
/// `variant` selects the copy: a one-time rollout notice, or an on-demand prompt
/// when a Free user activates a gated feature (e.g. Prompt Suggestions).
pub struct FreeAiRemovalModal {
    variant: FreeAiRemovalModalVariant,
    state_handles: StateHandles,
}

impl FreeAiRemovalModal {
    pub fn new(variant: FreeAiRemovalModalVariant, _ctx: &mut ViewContext<Self>) -> Self {
        Self {
            variant,
            state_handles: Default::default(),
        }
    }

    fn upgrade_url(ctx: &ViewContext<Self>) -> String {
        if let Some(team) = UserWorkspaces::as_ref(ctx).team_for_view(ctx) {
            UserWorkspaces::upgrade_link_for_team(team.uid)
        } else {
            let user_id = AuthStateProvider::as_ref(ctx)
                .get()
                .user_id()
                .unwrap_or_default();
            UserWorkspaces::upgrade_link(user_id)
        }
    }

    fn render_buttons(&self, appearance: &Appearance) -> Box<dyn Element> {
        let byok_button = appearance
            .ui_builder()
            .button(
                ButtonVariant::Secondary,
                self.state_handles.byok_button.clone(),
            )
            .with_style(UiComponentStyles {
                font_size: Some(14.),
                height: Some(32.),
                ..Default::default()
            })
            .with_centered_text_label("Bring your own AI".to_string())
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(FreeAiRemovalModalAction::SetUpByok);
            })
            .finish();

        let upgrade_button = appearance
            .ui_builder()
            .button(
                ButtonVariant::Accent,
                self.state_handles.upgrade_button.clone(),
            )
            .with_style(UiComponentStyles {
                font_size: Some(14.),
                height: Some(32.),
                ..Default::default()
            })
            .with_centered_text_label("View pricing".to_string())
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(FreeAiRemovalModalAction::Upgrade);
            })
            .finish();

        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.)
            .with_child(byok_button)
            .with_child(upgrade_button)
            .finish()
    }

    fn render_close_button(&self, appearance: &Appearance) -> Box<dyn Element> {
        appearance
            .ui_builder()
            .close_button(
                CLOSE_BUTTON_DIAMETER,
                self.state_handles.close_button.clone(),
            )
            .build()
            .on_click(|ctx, _, _| ctx.dispatch_typed_action(FreeAiRemovalModalAction::Close))
            .finish()
    }
}

impl Entity for FreeAiRemovalModal {
    type Event = FreeAiRemovalModalEvent;
}

impl View for FreeAiRemovalModal {
    fn ui_name() -> &'static str {
        "FreeAiRemovalModal"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let bg = blended_colors::neutral_1(theme);
        let font_family = appearance.ui_font_family();

        let title = FormattedTextElement::from_str(self.variant.title(), font_family, 18.)
            .with_color(blended_colors::text_main(theme, bg))
            .with_weight(Weight::Bold)
            .finish();

        let body_color = blended_colors::text_sub(theme, bg);
        let body = FormattedTextElement::from_str(self.variant.body(), font_family, 14.)
            .with_color(body_color)
            .finish();

        let mut content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(Container::new(title).with_margin_bottom(12.).finish());

        // Tighten the gap below the body when a secondary note follows; otherwise
        // keep the full gap above the buttons.
        let body_margin_bottom = if self.variant.secondary().is_some() {
            8.
        } else {
            20.
        };
        content.add_child(
            Container::new(body)
                .with_margin_bottom(body_margin_bottom)
                .finish(),
        );

        if let Some(secondary_text) = self.variant.secondary() {
            let secondary = FormattedTextElement::from_str(secondary_text, font_family, 14.)
                .with_color(body_color)
                .finish();
            content.add_child(Container::new(secondary).with_margin_bottom(20.).finish());
        }

        let content = content.with_child(self.render_buttons(appearance)).finish();

        let mut modal = Stack::new();
        modal.add_child(
            Container::new(
                ConstrainedBox::new(content)
                    .with_width(MODAL_WIDTH)
                    .finish(),
            )
            .with_background_color(bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(CORNER_RADIUS)))
            .with_border(Border::all(1.).with_border_fill(theme.outline()))
            .with_uniform_padding(PANEL_PADDING)
            .with_drop_shadow(DropShadow::default())
            .finish(),
        );
        modal.add_positioned_child(
            self.render_close_button(appearance),
            OffsetPositioning::offset_from_parent(
                vec2f(-8., 8.),
                ParentOffsetBounds::ParentByPosition,
                ParentAnchor::TopRight,
                ChildAnchor::TopRight,
            ),
        );

        let mut stack = Stack::new();
        stack.add_positioned_child(
            modal.finish(),
            OffsetPositioning::offset_from_parent(
                vec2f(0., 0.),
                ParentOffsetBounds::WindowByPosition,
                ParentAnchor::Center,
                ChildAnchor::Center,
            ),
        );

        Container::new(Align::new(stack.finish()).finish())
            .with_background(Fill::Solid(ColorU::new(97, 97, 97, 255)).with_opacity(50))
            .finish()
    }
}

impl TypedActionView for FreeAiRemovalModal {
    type Action = FreeAiRemovalModalAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            FreeAiRemovalModalAction::Close => {
                ctx.emit(FreeAiRemovalModalEvent::Close);
            }
            FreeAiRemovalModalAction::SetUpByok => {
                // Deferred so the close-driven refocus below doesn't steal focus from
                // the settings page this opens.
                ctx.dispatch_typed_action_deferred(WorkspaceAction::ShowSettingsPageWithSearch {
                    search_query: "api".to_string(),
                    section: Some(SettingsSection::WarpAgent),
                });
                ctx.emit(FreeAiRemovalModalEvent::Close);
            }
            FreeAiRemovalModalAction::Upgrade => {
                let upgrade_url = Self::upgrade_url(ctx);
                ctx.open_url(&upgrade_url);
                ctx.emit(FreeAiRemovalModalEvent::Close);
            }
        }
    }
}
