use std::sync::Arc;

use pathfinder_geometry::vector::vec2f;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Border, ChildAnchor, ChildView, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    DEFAULT_UI_LINE_HEIGHT_RATIO, Flex, Hoverable, MouseStateHandle, OffsetPositioning,
    ParentAnchor, ParentElement, ParentOffsetBounds, Radius, SavePosition, Stack, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::ui_components::components::UiComponent;
use warpui::{
    AppContext, Element, Entity, EntityId, ModelHandle, SingletonEntity as _, TypedActionView,
    View, ViewContext, ViewHandle,
};

use crate::ai::agent::icons::todo_list_icon;
use crate::ai::agent::todos::popup::{AgentTodosPopupEvent, AgentTodosPopupView};
use crate::ai::blocklist::{
    BlocklistAIContextEvent, BlocklistAIContextModel, BlocklistAIHistoryEvent,
};
use crate::terminal::input::{MenuPositioning, MenuPositioningProvider};
use crate::ui_components::blended_colors;
use crate::{AIAgentTodoList, BlocklistAIHistoryModel};

const TODO_BUTTON_SAVE_POSITION_ID: &str = "plan_and_todo_list::todo_button";

/// A context chip that shows the todo list for the active conversation
pub struct PlanAndTodoListView {
    context_model: ModelHandle<BlocklistAIContextModel>,
    menu_positioning_provider: Arc<dyn MenuPositioningProvider>,
    terminal_view_id: EntityId,
    todo_button_mouse_state: MouseStateHandle,
    agent_todos_popup: ViewHandle<AgentTodosPopupView>,
    is_todo_popup_open: bool,
}

pub enum PlanAndTodoListEvent {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanAndTodoListAction {
    ToggleTodoPopup,
}

impl PlanAndTodoListView {
    pub fn new(
        context_model: ModelHandle<BlocklistAIContextModel>,
        menu_positioning_provider: Arc<dyn MenuPositioningProvider>,
        terminal_view_id: EntityId,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let agent_todos_popup = ctx.add_typed_action_view(|ctx| {
            AgentTodosPopupView::new(terminal_view_id, context_model.clone(), ctx)
        });
        ctx.subscribe_to_view(&agent_todos_popup, |me, _, event, ctx| match event {
            AgentTodosPopupEvent::Close => {
                me.is_todo_popup_open = false;
                ctx.notify();
            }
        });

        ctx.subscribe_to_model(
            &BlocklistAIHistoryModel::handle(ctx),
            |me, _, event, ctx| {
                if event
                    .terminal_surface_id()
                    .is_some_and(|id| id != me.terminal_view_id)
                {
                    return;
                }
                // Note: UpdatedStreamingExchange is not needed here because plan/todo
                // chips only depend on conversation-level events and UpdatedTodoList,
                // not on regular content streaming updates.
                match event.clone() {
                    BlocklistAIHistoryEvent::StartedNewConversation { .. }
                    | BlocklistAIHistoryEvent::SetActiveConversation { .. }
                    | BlocklistAIHistoryEvent::ClearedConversationsForTerminalSurface { .. }
                    | BlocklistAIHistoryEvent::AppendedExchange { .. }
                    | BlocklistAIHistoryEvent::UpdatedTodoList { .. } => {
                        ctx.notify();
                    }
                    _ => (),
                }
            },
        );

        // Subscribe to context model to detect when pending query state changes (e.g., new conversation)
        ctx.subscribe_to_model(&context_model, |_, _, event, ctx| {
            if let BlocklistAIContextEvent::PendingQueryStateUpdated = event {
                ctx.notify();
            }
        });

        Self {
            context_model,
            menu_positioning_provider,
            terminal_view_id,
            todo_button_mouse_state: Default::default(),
            agent_todos_popup,
            is_todo_popup_open: false,
        }
    }

    pub fn should_render(&self, app: &AppContext) -> bool {
        self.todo_list(app).is_some()
    }

    fn render_chip_button(
        &self,
        content: Box<dyn Element>,
        mouse_state_handle: MouseStateHandle,
        tool_tip_text: String,
        corner_radius: CornerRadius,
        appearance: &Appearance,
    ) -> Hoverable {
        Hoverable::new(mouse_state_handle.clone(), move |state| {
            let background = if state.is_hovered() {
                internal_colors::fg_overlay_2(appearance.theme())
            } else {
                internal_colors::fg_overlay_1(appearance.theme())
            };

            let container = Container::new(content)
                .with_background(background)
                .with_padding_left(6.)
                .with_padding_right(6.)
                .with_corner_radius(corner_radius)
                .with_border(
                    Border::all(1.0)
                        .with_border_fill(internal_colors::neutral_3(appearance.theme())),
                )
                .with_padding_top(2.)
                .with_padding_bottom(2.)
                .finish();

            if state.is_hovered() {
                let mut stack = Stack::new().with_child(container);

                let tooltip_element = appearance
                    .ui_builder()
                    .tool_tip(tool_tip_text)
                    .build()
                    .finish();

                stack.add_positioned_overlay_child(
                    tooltip_element,
                    OffsetPositioning::offset_from_parent(
                        vec2f(0., -8.),
                        ParentOffsetBounds::WindowByPosition,
                        ParentAnchor::TopLeft,
                        ChildAnchor::BottomLeft,
                    ),
                );
                stack.finish()
            } else {
                container
            }
        })
        .with_cursor(Cursor::PointingHand)
    }

    fn todo_list(&self, app: &AppContext) -> Option<AIAgentTodoList> {
        let todo_list = self
            .context_model
            .as_ref(app)
            .selected_conversation_todolist(app);

        let should_show_todo_button = todo_list.is_some();

        if !should_show_todo_button {
            return None;
        }

        if let Some(todo_list) = todo_list
            && !todo_list.is_empty()
        {
            return Some(todo_list.clone());
        }

        None
    }

    fn render_todo_button(
        &self,
        todo_list: &AIAgentTodoList,
        icon_size: f32,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let num_todo_items = todo_list.len();
        let num_completed_todo_items = todo_list.completed_items().len();

        let primary_color = appearance.theme().surface_1();
        let todo_icon = Container::new(
            ConstrainedBox::new(
                todo_list_icon(appearance)
                    .with_color(appearance.theme().sub_text_color(primary_color))
                    .finish(),
            )
            .with_height(icon_size)
            .with_width(icon_size)
            .finish(),
        )
        .finish();

        // Use the same font sizing conventions as other UDI chips so text height never exceeds icon height
        let chip_font_size = appearance.monospace_font_size() - 1.0;
        let line_height_ratio = appearance.line_height_ratio();

        let completed_text = Text::new_inline(
            format!("{}", num_completed_todo_items + 1),
            appearance.ui_font_family(),
            chip_font_size,
        )
        .with_color(blended_colors::text_main(appearance.theme(), primary_color))
        .with_line_height_ratio(line_height_ratio)
        .with_style(Properties::default().weight(Weight::Semibold))
        .finish();

        // Separate the "slash" so we can apply a small margin between the slash and the numbers
        let slash_text = Text::new_inline("/", appearance.ui_font_family(), chip_font_size)
            .with_color(appearance.theme().sub_text_color(primary_color).into())
            .with_line_height_ratio(line_height_ratio)
            .with_style(Properties::default().weight(Weight::Semibold))
            .finish();

        let total_text = Text::new_inline(
            format!("{num_todo_items}"),
            appearance.ui_font_family(),
            chip_font_size,
        )
        .with_color(appearance.theme().sub_text_color(primary_color).into())
        .with_line_height_ratio(line_height_ratio)
        .with_style(Properties::default().weight(Weight::Semibold))
        .finish();

        let content = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(todo_icon)
            .with_child(Container::new(completed_text).with_margin_left(4.).finish())
            .with_child(Container::new(slash_text).with_margin_left(2.).finish())
            .with_child(Container::new(total_text).with_margin_left(2.).finish())
            .finish();

        let corner_radius = CornerRadius::with_all(Radius::Pixels(4.));

        let todo_button = self
            .render_chip_button(
                content,
                self.todo_button_mouse_state.clone(),
                "View todo list".to_string(),
                corner_radius,
                appearance,
            )
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(PlanAndTodoListAction::ToggleTodoPopup);
            })
            .finish();

        let todo_button = SavePosition::new(todo_button, TODO_BUTTON_SAVE_POSITION_ID).finish();

        // Todo popup overlay
        let mut todo_button = Stack::new().with_child(todo_button);
        if self.is_todo_popup_open {
            let positioning = match self.menu_positioning_provider.menu_position(app) {
                MenuPositioning::BelowInputBox => {
                    OffsetPositioning::offset_from_save_position_element(
                        TODO_BUTTON_SAVE_POSITION_ID,
                        vec2f(0., 4.),
                        warpui::elements::PositionedElementOffsetBounds::WindowByPosition,
                        warpui::elements::PositionedElementAnchor::BottomLeft,
                        ChildAnchor::TopLeft,
                    )
                }
                MenuPositioning::AboveInputBox => {
                    OffsetPositioning::offset_from_save_position_element(
                        TODO_BUTTON_SAVE_POSITION_ID,
                        vec2f(0., -4.),
                        warpui::elements::PositionedElementOffsetBounds::WindowByPosition,
                        warpui::elements::PositionedElementAnchor::TopLeft,
                        ChildAnchor::BottomLeft,
                    )
                }
            };
            todo_button.add_positioned_overlay_child(
                ChildView::new(&self.agent_todos_popup).finish(),
                positioning,
            );
        }

        todo_button.finish()
    }
}

impl Entity for PlanAndTodoListView {
    type Event = PlanAndTodoListEvent;
}

impl View for PlanAndTodoListView {
    fn ui_name() -> &'static str {
        "PlanAndTodoListView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn warpui::Element> {
        let appearance = Appearance::as_ref(app);

        // Calculate icon size
        let base_icon_size = app.font_cache().line_height(
            appearance.monospace_font_size(),
            DEFAULT_UI_LINE_HEIGHT_RATIO / 1.4,
        );
        let text_line_height = app.font_cache().line_height(
            appearance.monospace_font_size() - 1.0,
            appearance.line_height_ratio(),
        );
        let icon_size = (base_icon_size * 1.1).min(text_line_height);

        let todo_list = self.todo_list(app);

        let mut row = Flex::row();
        if let Some(todo_list) = todo_list {
            row.add_child(self.render_todo_button(&todo_list, icon_size, appearance, app));
        }

        row.finish()
    }
}

impl TypedActionView for PlanAndTodoListView {
    type Action = PlanAndTodoListAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            PlanAndTodoListAction::ToggleTodoPopup => {
                self.is_todo_popup_open = !self.is_todo_popup_open;
                // If we just opened the popup, request initial scroll to the in-progress item
                if self.is_todo_popup_open {
                    self.agent_todos_popup
                        .update(ctx, |popup, _ctx| popup.scroll_to_in_progress_item());
                    ctx.focus(&self.agent_todos_popup);
                }
                ctx.notify();
            }
        }
    }
}
