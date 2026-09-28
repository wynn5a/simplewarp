//! Inline config block rendered on plan cards when the conversation has
//! an active `OrchestrationConfigSnapshot`. Shows a "Use orchestration"
//! toggle and run-wide config dropdowns.

use ai::agent::orchestration_config::OrchestrationConfigStatus;
use warpui::elements::{
    ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Empty, Flex, Hoverable,
    MouseStateHandle, ParentElement, Radius, Stack, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::ui_components::components::UiComponent;
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext};

use crate::BlocklistAIHistoryModel;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::blocklist::BlocklistAIHistoryEvent;
use crate::ai::blocklist::inline_action::orchestration_controls::{
    self as oc, OrchestrationConfigState, OrchestrationControlAction, OrchestrationEditState,
    OrchestrationPickerHandles,
};
use crate::ai::document::ai_document_model::AIDocumentModel;
use crate::ai::llms::{LLMPreferences, LLMPreferencesEvent};
use crate::appearance::Appearance;
use crate::ui_components::blended_colors;

const CONFIG_BLOCK_HEADER: &str = "Use orchestration";
const CONFIG_BLOCK_DESCRIPTION: &str =
    "Break this work into coordinated streams with multiple agents.";
const BASE_MODEL_HELPER: &str = "The primary model all agents will use.";

// ── Action type ─────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrchestrationConfigBlockAction {
    ToggleApproval,
    ToggleDetails,
    ModelChanged { model_id: String },
    HarnessChanged { harness_type: String },
}

impl OrchestrationControlAction for OrchestrationConfigBlockAction {
    fn model_changed(model_id: String) -> Self {
        Self::ModelChanged { model_id }
    }
    fn harness_changed(harness_type: String) -> Self {
        Self::HarnessChanged { harness_type }
    }
}

// ── View ────────────────────────────────────────────────────────────

pub struct OrchestrationConfigBlockView {
    conversation_id: AIConversationId,
    plan_id: String,
    /// Run-wide config being edited plus per-harness model memory.
    orchestration_edit_state: OrchestrationEditState,
    pickers: OrchestrationPickerHandles<OrchestrationConfigBlockAction>,
    is_approved: bool,
    details_expanded: bool,
    pickers_initialized: bool,
    toggle_switch_state: SwitchStateHandle,
    details_mouse_state: MouseStateHandle,
    /// Suppresses self-triggered refresh when `apply_field_change`
    /// saves the config and the resulting event re-enters
    /// `refresh_from_model`.
    suppress_refresh: bool,
}

impl OrchestrationConfigBlockView {
    pub fn new(
        conversation_id: AIConversationId,
        plan_id: String,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let history = BlocklistAIHistoryModel::as_ref(ctx);
        let (config_state, is_approved) = history
            .conversation(&conversation_id)
            .and_then(|conv| {
                conv.orchestration_config_for_plan(&plan_id)
                    .map(|(config, status)| {
                        (
                            OrchestrationConfigState::from_orchestration_config(config),
                            status.is_approved(),
                        )
                    })
            })
            .unwrap_or_else(|| {
                (
                    OrchestrationConfigState::from_run_agents_fields(Some("auto"), Some("oz")),
                    false,
                )
            });

        ctx.subscribe_to_model(
            &BlocklistAIHistoryModel::handle(ctx),
            move |me, _, event, ctx| {
                if let BlocklistAIHistoryEvent::OrchestrationConfigUpdated {
                    conversation_id: cid,
                    ..
                } = event
                    && *cid == me.conversation_id
                {
                    me.refresh_from_model(ctx);
                }
            },
        );

        // Repopulate the model picker when available LLMs change (Oz
        // harness only — non-Oz harnesses get their catalog from
        // HarnessAvailabilityModel, not LLMPreferences).
        ctx.subscribe_to_model(&LLMPreferences::handle(ctx), |me, _, event, ctx| {
            if let LLMPreferencesEvent::UpdatedAvailableLLMs = event
                && let Some(handle) = &me.pickers.model_picker
            {
                oc::populate_model_picker_for_harness(
                    handle,
                    &me.orchestration_edit_state
                        .orchestration_config_state
                        .model_id,
                    &me.orchestration_edit_state
                        .orchestration_config_state
                        .harness_type,
                    ctx,
                );
            }
        });

        let mut view = Self {
            conversation_id,
            plan_id,
            orchestration_edit_state: OrchestrationEditState::new(config_state),
            pickers: OrchestrationPickerHandles::default(),
            is_approved,
            details_expanded: false,
            pickers_initialized: false,
            toggle_switch_state: SwitchStateHandle::default(),
            details_mouse_state: MouseStateHandle::default(),
            suppress_refresh: false,
        };
        if view.is_approved {
            view.ensure_pickers(ctx);
        }
        view
    }

    fn refresh_from_model(&mut self, ctx: &mut ViewContext<Self>) {
        if self.suppress_refresh {
            self.suppress_refresh = false;
            return;
        }
        let history = BlocklistAIHistoryModel::as_ref(ctx);
        if let Some(conv) = history.conversation(&self.conversation_id)
            && let Some((config, status)) = conv.orchestration_config_for_plan(&self.plan_id)
        {
            self.orchestration_edit_state.orchestration_config_state =
                OrchestrationConfigState::from_orchestration_config(config);
            self.is_approved = status.is_approved();
            if self.pickers_initialized {
                oc::repopulate_all_pickers(
                    &mut self.orchestration_edit_state.orchestration_config_state,
                    &self.pickers,
                    ctx,
                );
            }
            ctx.notify();
        }
    }

    fn ensure_pickers(&mut self, ctx: &mut ViewContext<Self>) {
        if self.pickers_initialized {
            return;
        }

        let appearance = Appearance::as_ref(ctx);
        let (styles, colors) = oc::picker_styles(appearance);

        // When the agent didn't specify a model, fall back to the
        // conversation's current base model so the picker isn't blank.
        let display_model_id = if self
            .orchestration_edit_state
            .orchestration_config_state
            .model_id
            .trim()
            .is_empty()
        {
            BlocklistAIHistoryModel::as_ref(ctx)
                .conversation(&self.conversation_id)
                .and_then(|conv| conv.latest_exchange())
                .map(|ex| ex.model_id.to_string())
                .unwrap_or_default()
        } else {
            self.orchestration_edit_state
                .orchestration_config_state
                .model_id
                .clone()
        };
        let model_handle = oc::new_standard_filterable_picker_dropdown(&styles, ctx);
        model_handle.update(ctx, |d, c| d.set_use_overlay_layer(true, c));
        oc::populate_model_picker_for_harness(
            &model_handle,
            &display_model_id,
            &self
                .orchestration_edit_state
                .orchestration_config_state
                .harness_type,
            ctx,
        );
        self.pickers.model_picker = Some(model_handle);

        let harness_handle = oc::new_standard_picker_dropdown(&colors, ctx);
        harness_handle.update(ctx, |d, c| d.set_use_overlay_layer(true, c));
        oc::populate_harness_picker(
            &harness_handle,
            &self
                .orchestration_edit_state
                .orchestration_config_state
                .harness_type,
            ctx,
        );
        self.pickers.harness_picker = Some(harness_handle);

        self.pickers_initialized = true;
        oc::sync_picker_selections(
            &self.orchestration_edit_state.orchestration_config_state,
            &self.pickers,
            ctx,
        );
    }

    fn apply_field_change(&mut self, ctx: &mut ViewContext<Self>) {
        self.suppress_refresh = true;
        let config = self
            .orchestration_edit_state
            .orchestration_config_state
            .to_orchestration_config();
        let status = if self.is_approved {
            OrchestrationConfigStatus::Approved
        } else {
            OrchestrationConfigStatus::Disapproved
        };
        let conversation_id = self.conversation_id;
        let plan_id = self.plan_id.clone();
        AIDocumentModel::handle(ctx).update(ctx, |model, ctx| {
            model.set_orchestration_config_for_plan(conversation_id, plan_id, config, status, ctx);
        });
    }
}

impl Entity for OrchestrationConfigBlockView {
    type Event = ();
}

impl View for OrchestrationConfigBlockView {
    fn ui_name() -> &'static str {
        "OrchestrationConfigBlockView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        // Header row: "Use orchestration" + pill toggle switch
        let header_label = Text::new(
            CONFIG_BLOCK_HEADER.to_string(),
            appearance.ui_font_family(),
            16.,
        )
        .with_color(blended_colors::text_main(theme, theme.background()))
        .with_style(Properties::default().weight(Weight::Bold))
        .finish();

        let is_on = self.is_approved;
        let ui_builder = appearance.ui_builder();

        let header_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(warpui::elements::Expanded::new(1.0, header_label).finish())
            .with_child(
                ui_builder
                    .switch(self.toggle_switch_state.clone())
                    .check(is_on)
                    .build()
                    .on_click(|ctx, _, _| {
                        ctx.dispatch_typed_action(OrchestrationConfigBlockAction::ToggleApproval);
                    })
                    .finish(),
            )
            .finish();
        column.add_child(header_row);

        // Description
        let description = Text::new(
            CONFIG_BLOCK_DESCRIPTION.to_string(),
            appearance.ui_font_family(),
            appearance.monospace_font_size(),
        )
        .with_color(blended_colors::text_main(theme, theme.background()))
        .finish();
        column.add_child(Container::new(description).with_margin_top(8.).finish());

        // "View details" row + expandable controls (only when approved)
        if self.is_approved {
            // Divider
            let divider = Container::new(
                ConstrainedBox::new(Empty::new().finish())
                    .with_height(1.)
                    .finish(),
            )
            .with_background_color(theme.surface_2().into_solid())
            .finish();
            column.add_child(Container::new(divider).with_margin_top(8.).finish());

            // "View details" link row
            let chevron_icon = if self.details_expanded {
                warp_core::ui::Icon::ChevronDown
            } else {
                warp_core::ui::Icon::ChevronRight
            };
            let disabled_text_color = blended_colors::text_disabled(theme, theme.background());
            let details_text = Text::new(
                "View details".to_string(),
                appearance.ui_font_family(),
                appearance.monospace_font_size() + 1.,
            )
            .with_color(disabled_text_color)
            .finish();
            let chevron = ConstrainedBox::new(
                chevron_icon
                    .to_warpui_icon(warp_core::ui::theme::Fill::Solid(disabled_text_color))
                    .finish(),
            )
            .with_width(14.)
            .with_height(14.)
            .finish();
            let details_link = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(2.)
                .with_child(details_text)
                .with_child(chevron)
                .finish();
            let details_link_hoverable =
                Hoverable::new(self.details_mouse_state.clone(), move |_| details_link)
                    .on_click(|ctx, _, _| {
                        ctx.dispatch_typed_action(OrchestrationConfigBlockAction::ToggleDetails);
                    })
                    .with_cursor(Cursor::PointingHand)
                    .finish();
            let details_row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(details_link_hoverable)
                .finish();
            column.add_child(Container::new(details_row).with_margin_top(8.).finish());

            // Expanded controls
            if self.details_expanded {
                // Pickers stacked vertically
                column.add_child(oc::render_picker_row_with_layout(
                    &self.pickers,
                    appearance,
                    true,
                ));

                // Helper text
                let helper = Text::new(
                    BASE_MODEL_HELPER.to_string(),
                    appearance.ui_font_family(),
                    appearance.monospace_font_size() - 1.,
                )
                .with_color(blended_colors::text_disabled(theme, theme.background()))
                .finish();
                column.add_child(Container::new(helper).with_margin_top(4.).finish());

                // Validation
                if let Some(reason) = oc::accept_disabled_reason_with_setup(
                    &self.orchestration_edit_state.orchestration_config_state,
                ) {
                    column.add_child(oc::render_validation_error(
                        reason,
                        theme.ui_error_color(),
                        appearance,
                    ));
                }
            }
        }

        // Outer container with accent styling per Figma
        let card = Container::new(column.finish())
            .with_uniform_padding(12.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
            .with_background(warp_core::ui::theme::color::internal_colors::accent_overlay_1(theme))
            .with_border(warpui::elements::Border::all(1.).with_border_fill(theme.accent()))
            .finish();

        Stack::new().with_child(card).finish()
    }
}

impl TypedActionView for OrchestrationConfigBlockView {
    type Action = OrchestrationConfigBlockAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            OrchestrationConfigBlockAction::ToggleApproval => {
                self.is_approved = !self.is_approved;
                if self.is_approved && !self.pickers_initialized {
                    self.ensure_pickers(ctx);
                }
                self.apply_field_change(ctx);
                ctx.notify();
            }
            OrchestrationConfigBlockAction::ToggleDetails => {
                self.details_expanded = !self.details_expanded;
                if self.details_expanded && !self.pickers_initialized {
                    self.ensure_pickers(ctx);
                }
                ctx.notify();
            }
            OrchestrationConfigBlockAction::ModelChanged { model_id } => {
                self.orchestration_edit_state
                    .orchestration_config_state
                    .model_id = model_id.clone();
                self.apply_field_change(ctx);
                ctx.notify();
            }
            OrchestrationConfigBlockAction::HarnessChanged { harness_type } => {
                let fallback = BlocklistAIHistoryModel::as_ref(ctx)
                    .conversation(&self.conversation_id)
                    .and_then(|conv| conv.latest_exchange())
                    .map(|ex| ex.model_id.to_string());
                oc::apply_harness_change(
                    &mut self.orchestration_edit_state,
                    &self.pickers,
                    harness_type,
                    fallback,
                    ctx,
                );
                self.apply_field_change(ctx);
                ctx.notify();
            }
        }
    }
}
