use cloud_objects::drive::CloudObjectTypeAndId;
use pathfinder_color::ColorU;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::Fill;
use warpui::elements::{Align, ChildView, Container, ParentElement, SavePosition, Stack};
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use super::suggested_rule_modal::SuggestedRuleAndId;
use crate::ai::agent::{SuggestedLoggingId, SuggestedRule};
use crate::ai::facts::{AIFact, AIMemory, CloudAIFactModel};
use crate::cloud_object::model::generic_string_model::GenericStringObjectId;
use crate::cloud_object::model::persistence::{CloudModel, CloudModelEvent};
use crate::server::ids::{ClientId, SyncId};
use crate::ui_components::blended_colors;
use crate::ui_components::icons::Icon;
use crate::view_components::action_button::{ActionButton, ActionButtonTheme, SecondaryTheme};

const MAX_CHIP_WIDTH: f32 = 316.;

/// A chip view component for displaying suggested rules.
///
/// This component is responsible for:
/// - Rendering a clickable chip that represents a suggested rule
/// - Tracking the state of the rule (saved or not)
/// - Handling clicks to show the appropriate modal dialog via workspace-level events
/// - Syncing with the cloud model for persistence
///
/// When clicked, the chip emits an event to display the `SuggestedRuleModal` at the workspace
/// level, which positions itself relative to the chip position.
///
/// # UI Behavior
/// - A chip with a saved rule shows a check icon and has a different theme
/// - The chip position is saved for the modal dialog to position relative to it
/// - Chips have limited width and tooltips for overflow content
///
/// # Events
/// The component emits events to:
/// - Show the rule modal
/// - Open existing rules directly
/// A theme for the dismiss button in the suggestion footer.
pub struct SuggestionDismissButtonTheme;

impl ActionButtonTheme for SuggestionDismissButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        if hovered {
            Some(blended_colors::fg_overlay_2(appearance.theme()))
        } else {
            None
        }
    }

    fn text_color(
        &self,
        _hovered: bool,
        _background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        appearance
            .theme()
            .sub_text_color(appearance.theme().background())
            .into()
    }
}

#[derive(Debug, Clone)]
pub enum SuggestedChipViewEvent {
    ShowSuggestedRuleDialog { rule_and_id: SuggestedRuleAndId },
    OpenAIFactCollection { sync_id: Option<SyncId> },
}

#[derive(Debug, Clone)]
pub enum SuggestedViewAction {
    ChipClicked,
}

#[derive(Debug, Clone)]
enum Suggestion {
    Rule { rule: SuggestedRule },
}

impl Suggestion {
    pub fn icon(&self) -> Icon {
        match self {
            Suggestion::Rule { .. } => Icon::BookOpen,
        }
    }

    pub fn tooltip(&self) -> String {
        match self {
            Suggestion::Rule { rule, .. } => {
                format!("Add rule: {}", rule.content.clone())
            }
        }
    }

    fn position_id(&self) -> String {
        match self {
            Suggestion::Rule { rule, .. } => format!("rule_position_{}", rule.logging_id),
        }
    }

    fn chip_label(&self) -> String {
        match self {
            Suggestion::Rule { rule, .. } => rule.content.clone(),
        }
    }
}

/// Data required to render a single suggested rule.
#[derive(Clone)]
pub struct SuggestionChipView {
    suggestion: Suggestion,
    chip: ViewHandle<ActionButton>,
    sync_id: SyncId,
    is_saved: bool,
}

impl SuggestionChipView {
    pub fn new_rule_chip(rule: SuggestedRule, ctx: &mut ViewContext<Self>) -> Self {
        Self::listen_for_warp_drive_events(ctx);

        let chip = ctx.add_typed_action_view(|_| {
            ActionButton::new(rule.content.clone(), SecondaryTheme)
                .with_max_label_width(MAX_CHIP_WIDTH)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(SuggestedViewAction::ChipClicked);
                })
        });

        let suggestion = Suggestion::Rule { rule };
        let mut me = Self {
            suggestion,
            sync_id: SyncId::ClientId(ClientId::default()),
            chip,
            is_saved: false,
        };
        me.reset_suggestion(ctx);
        me
    }

    pub fn logging_id(&self) -> SuggestedLoggingId {
        match &self.suggestion {
            Suggestion::Rule { rule, .. } => rule.logging_id.clone(),
        }
    }

    fn listen_for_warp_drive_events(ctx: &mut ViewContext<Self>) {
        let cloud_model = CloudModel::handle(ctx);
        ctx.subscribe_to_model(&cloud_model, |me, _, event, ctx| {
            me.handle_cloud_model_event(event, ctx);
        });
    }

    fn handle_cloud_model_event(&mut self, event: &CloudModelEvent, ctx: &mut ViewContext<Self>) {
        match event {
            CloudModelEvent::ObjectUpdated {
                type_and_id: CloudObjectTypeAndId::GenericStringObject { id, .. },
                ..
            } => {
                if self.sync_id.into_client() == id.into_client() {
                    self.load_suggestion(ctx);
                }
            }
            CloudModelEvent::ObjectTrashed {
                type_and_id: CloudObjectTypeAndId::GenericStringObject { id, .. },
                ..
            }
            | CloudModelEvent::ObjectDeleted {
                type_and_id: CloudObjectTypeAndId::GenericStringObject { id, .. },
                ..
            } => {
                // If the rule has been deleted, then we should reset it such that
                // the suggestion can be added again.
                if self.sync_id == *id {
                    self.reset_suggestion(ctx);
                }
            }
            _ => {}
        }
    }

    /// Resets the rule to its initial state.
    fn reset_suggestion(&mut self, ctx: &mut ViewContext<Self>) {
        self.sync_id = SyncId::ClientId(ClientId::default());
        self.is_saved = false;
        let icon: Icon = self.suggestion.icon();
        let tooltip: String = self.suggestion.tooltip();
        let label: String = self.suggestion.chip_label();
        self.chip.update(ctx, |chip, ctx| {
            chip.set_icon(Some(icon), ctx);
            chip.set_label(label.clone(), ctx);
            chip.set_tooltip(Some(tooltip.clone()), ctx);
            chip.set_theme(SecondaryTheme, ctx);
            chip.set_active(false, ctx);
        });
        ctx.notify();
    }

    /// Fetches the rule from the cloud model, and updates the UI to reflect that.
    fn load_suggestion(&mut self, ctx: &mut ViewContext<Self>) {
        let cloud_model = CloudModel::handle(ctx);
        let tooltip = self.suggestion.tooltip();

        match &mut self.suggestion {
            Suggestion::Rule { .. } => {
                if let Some(rule) = cloud_model
                    .as_ref(ctx)
                    .get_object_of_type::<GenericStringObjectId, CloudAIFactModel>(&self.sync_id)
                {
                    let AIFact::Memory(AIMemory { content, .. }) =
                        rule.model().string_model.clone();
                    self.chip.update(ctx, |chip, ctx| {
                        chip.set_label(content.clone(), ctx);
                        chip.set_tooltip(Some(tooltip.clone()), ctx);
                    });
                    ctx.notify();
                }
            }
        }
    }
}

impl Entity for SuggestionChipView {
    type Event = SuggestedChipViewEvent;
}

impl View for SuggestionChipView {
    fn ui_name() -> &'static str {
        "SuggestionChipView"
    }

    fn render(&self, _app: &AppContext) -> Box<dyn Element> {
        let stack = Stack::new();
        let position_id = self.suggestion.position_id();

        let chip_element = Container::new(ChildView::new(&self.chip).finish())
            .with_margin_right(8.)
            .finish();

        stack
            .with_child(
                Align::new(
                    Container::new(SavePosition::new(chip_element, position_id.as_str()).finish())
                        .finish(),
                )
                .left()
                .finish(),
            )
            .finish()
    }
}

impl TypedActionView for SuggestionChipView {
    type Action = SuggestedViewAction;

    fn handle_action(&mut self, action: &SuggestedViewAction, ctx: &mut ViewContext<Self>) {
        match action {
            SuggestedViewAction::ChipClicked => match &self.suggestion {
                Suggestion::Rule { rule, .. } => {
                    if CloudModel::as_ref(ctx)
                        .get_object_of_type::<GenericStringObjectId, CloudAIFactModel>(
                            &self.sync_id,
                        )
                        .is_some()
                    {
                        ctx.emit(SuggestedChipViewEvent::OpenAIFactCollection {
                            sync_id: Some(self.sync_id),
                        });
                    } else {
                        ctx.emit(SuggestedChipViewEvent::ShowSuggestedRuleDialog {
                            rule_and_id: SuggestedRuleAndId {
                                rule: rule.clone(),
                                sync_id: self.sync_id,
                            },
                        });
                        self.chip.update(ctx, |chip, ctx| {
                            chip.set_active(true, ctx);
                        });
                    }
                }
            },
        }
    }
}
