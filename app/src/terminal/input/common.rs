use std::sync::Arc;

use warp_completer::completer::Description;
use warpui::elements::{
    AnchorPair, Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    DispatchEventResult, Element, EventHandler, Flex, OffsetPositioning, OffsetType, ParentElement,
    ParentOffsetBounds, PositionedElementOffsetBounds, PositioningAxis, Radius, Shrinkable, Stack,
    XAxisAnchor,
};
use warpui::fonts::Weight;
use warpui::presenter::ChildView;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{AppContext, ViewHandle};

use crate::appearance::Appearance;
use crate::terminal::input::{Input, InputAction, InputSuggestionsMode, MenuPositioning};
use crate::terminal::view::{PADDING_LEFT, TerminalAction};

/// Wraps the given column, assumed to represent the full input content, with appropriate
/// left padding to be consistent with the terminal content, as well as an event handler to
/// focus the input view when clicked.
pub(super) fn wrap_input_with_terminal_padding_and_focus_handler(
    is_active_session: bool,
    column: Box<dyn Element>,
    use_adjusted_padding: bool,
) -> Box<dyn Element> {
    let terminal_padding = if use_adjusted_padding {
        *PADDING_LEFT / 1.5
    } else {
        *PADDING_LEFT
    };

    if is_active_session {
        let mut flex_row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::End);

        flex_row.add_child(
            Shrinkable::new(
                1.,
                Container::new(column)
                    .with_padding_left(terminal_padding)
                    .finish(),
            )
            .finish(),
        );

        EventHandler::new(flex_row.finish())
            .on_left_mouse_down(move |ctx, _, _| {
                ctx.dispatch_typed_action(TerminalAction::ClearSelectionsWhenShellMode);
                ctx.dispatch_typed_action(InputAction::FocusInputBox);
                DispatchEventResult::StopPropagation
            })
            .finish()
    } else {
        Container::new(column)
            .with_padding_left(terminal_padding)
            .finish()
    }
}

/// Renders the selected workflow info overlay over the input.
pub(super) fn add_workflow_info_overlay(
    stack: &mut Stack,
    selected_workflow_state: &super::SelectedWorkflowState,
    pane_height_px: f32,
    menu_positioning: MenuPositioning,
) {
    let workflows_info_view =
        Container::new(ChildView::new(&selected_workflow_state.more_info_view).finish())
            .with_margin_left(16.)
            .with_margin_right(16.)
            .finish();

    stack.add_positioned_overlay_child(
        ConstrainedBox::new(workflows_info_view)
            .with_max_height(pane_height_px * 0.35)
            .finish(),
        OffsetPositioning::from_axes(
            PositioningAxis::relative_to_parent(
                ParentOffsetBounds::ParentByPosition,
                OffsetType::Pixel(0.),
                AnchorPair::new(XAxisAnchor::Left, XAxisAnchor::Left),
            ),
            PositioningAxis::relative_to_parent(
                ParentOffsetBounds::Unbounded,
                OffsetType::Pixel(0.),
                menu_positioning.workflows_info_y_anchor(),
            ),
        ),
    );
}

/// Renders the voltron overlay over the input.
pub(super) fn add_voltron_overlay(
    stack: &mut Stack,
    voltron_view: &ViewHandle<crate::voltron::Voltron>,
    menu_positioning: MenuPositioning,
) {
    stack.add_positioned_overlay_child(
        ChildView::new(voltron_view).finish(),
        OffsetPositioning::offset_from_parent(
            menu_positioning.voltron_offset(),
            ParentOffsetBounds::Unbounded,
            menu_positioning.voltron_parent_anchor(),
            menu_positioning.voltron_child_anchor(),
        ),
    );
}

/// Renders the appropriate input suggestions overlay over the input, based on the current input
/// suggestions mode (if any).
pub(super) fn add_input_suggestions_overlays(
    input: &Input,
    stack: &mut Stack,
    appearance: &Appearance,
    menu_positioning: MenuPositioning,
    app: &AppContext,
) {
    match input.suggestions_mode_model().as_ref(app).mode() {
        InputSuggestionsMode::CompletionSuggestions { menu_position, .. } => {
            let relative_position_id = menu_position.to_position_id(input.editor.id());
            stack.add_positioned_overlay_child(
                input.render_completion_suggestions_menu(appearance, menu_positioning),
                OffsetPositioning::from_axes(
                    PositioningAxis::relative_to_stack_child(
                        &relative_position_id,
                        PositionedElementOffsetBounds::WindowByPosition,
                        OffsetType::Pixel(0.),
                        AnchorPair::new(XAxisAnchor::Left, XAxisAnchor::Left),
                    ),
                    PositioningAxis::relative_to_stack_child(
                        &relative_position_id,
                        PositionedElementOffsetBounds::Unbounded,
                        OffsetType::Pixel(0.),
                        menu_positioning.completion_suggestions_y_anchor(),
                    ),
                ),
            );
        }
        InputSuggestionsMode::StaticWorkflowEnumSuggestions { menu_position, .. } => {
            let relative_position_id = menu_position.to_position_id(input.editor.id());
            stack.add_positioned_overlay_child(
                input.render_workflow_enum_suggestions_menu(appearance, menu_positioning),
                OffsetPositioning::from_axes(
                    PositioningAxis::relative_to_stack_child(
                        &relative_position_id,
                        PositionedElementOffsetBounds::WindowByPosition,
                        OffsetType::Pixel(0.),
                        AnchorPair::new(XAxisAnchor::Left, XAxisAnchor::Left),
                    ),
                    PositioningAxis::relative_to_stack_child(
                        &relative_position_id,
                        PositionedElementOffsetBounds::Unbounded,
                        OffsetType::Pixel(0.),
                        menu_positioning.completion_suggestions_y_anchor(),
                    ),
                ),
            );
        }
        InputSuggestionsMode::DynamicWorkflowEnumSuggestions {
            menu_position,
            dynamic_enum_status,
            command,
            suggestions,
            ..
        } => {
            let relative_position_id = menu_position.to_position_id(input.editor.id());
            stack.add_positioned_overlay_child(
                input.render_dynamic_workflow_enum_menu(
                    appearance,
                    menu_positioning,
                    command.clone(),
                    dynamic_enum_status.clone(),
                    suggestions,
                ),
                OffsetPositioning::from_axes(
                    PositioningAxis::relative_to_stack_child(
                        &relative_position_id,
                        PositionedElementOffsetBounds::WindowByPosition,
                        OffsetType::Pixel(0.),
                        AnchorPair::new(XAxisAnchor::Left, XAxisAnchor::Left),
                    ),
                    PositioningAxis::relative_to_stack_child(
                        &relative_position_id,
                        PositionedElementOffsetBounds::Unbounded,
                        OffsetType::Pixel(0.),
                        menu_positioning.completion_suggestions_y_anchor(),
                    ),
                ),
            );
        }
        InputSuggestionsMode::AIContextMenu { .. } => {
            input.render_ai_context_menu(stack, &menu_positioning, app);
        }
        // SlashCommandsMenu is rendered separately via inline_slash_commands_menu_view
        InputSuggestionsMode::SlashCommands => {}
        // Conversation menu is rendered separately via inline_conversation_menu_view
        InputSuggestionsMode::ConversationMenu => {}
        // Model selector is rendered separately via inline_model_selector_view
        InputSuggestionsMode::ModelSelector => {}
        // Profile selector is rendered separately via inline_profile_selector_view
        InputSuggestionsMode::ProfileSelector => {}
        // Skill menu is rendered separately via inline_skill_selector_view
        InputSuggestionsMode::SkillMenu => {}
        // User query menu is rendered separately via user_query_menu_view
        InputSuggestionsMode::UserQueryMenu { .. } => {}
        // Inline history menu is rendered separately via inline_history_menu_view
        InputSuggestionsMode::InlineHistoryMenu { .. } => {}
        // Repos menu is rendered separately via inline_repos_menu_view
        InputSuggestionsMode::IndexedReposMenu => {}
        InputSuggestionsMode::Closed => {}
    }
}

/// Renders the command xray overlay on the input using the command x ray-specific position id.
pub(super) fn add_command_xray_overlay(
    input: &Input,
    stack: &mut Stack,
    token_description: &Arc<Description>,
    appearance: &Appearance,
    menu_positioning: MenuPositioning,
    app: &AppContext,
) {
    let command_x_ray_position_id = format!("editor:command_x_ray_{}", input.editor.id());
    let line_height = input
        .editor
        .as_ref(app)
        .line_height(app.font_cache(), appearance);
    let offset = match menu_positioning {
        MenuPositioning::AboveInputBox => OffsetType::Pixel(0.),
        MenuPositioning::BelowInputBox => OffsetType::Pixel(line_height),
    };
    stack.add_positioned_overlay_child(
        render_command_token_description(token_description, appearance),
        OffsetPositioning::from_axes(
            PositioningAxis::relative_to_stack_child(
                &command_x_ray_position_id,
                PositionedElementOffsetBounds::ParentByPosition,
                OffsetType::Pixel(0.),
                AnchorPair::new(XAxisAnchor::Left, XAxisAnchor::Left),
            ),
            PositioningAxis::relative_to_stack_child(
                &command_x_ray_position_id,
                PositionedElementOffsetBounds::Unbounded,
                offset,
                menu_positioning.command_xray_y_anchor(),
            ),
        ),
    );
}

fn render_command_token_description(
    description: &Arc<Description>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    // Append an ellipsis to the description if the token has more characters than the max
    // number of characters that are allowed.
    const MAX_XRAY_LABEL_CHARS: usize = 16;
    const TOKEN_DESCRIPTION_PADDING: f32 = 12.;
    const TOKEN_DESCRIPTION_MARGIN: f32 = 10.;
    const TOKEN_DESCRIPTION_WIDTH: f32 = 240.;
    const TOKEN_LABEL_HORIZONTAL_PADDING: f32 = 8.;
    const TOKEN_LABEL_VERTICAL_PADDING: f32 = 4.;

    let truncated_label = match description
        .token
        .item
        .char_indices()
        .nth(MAX_XRAY_LABEL_CHARS)
    {
        None => description.token.item.clone(),
        Some((byte_index, _)) => format!("{}...", &description.token[..byte_index]),
    };

    let theme = appearance.theme();
    let ui_builder = appearance.ui_builder();

    let mut command_description = Flex::column().with_child(
        Flex::row()
            .with_child(
                Container::new(
                    ui_builder
                        .paragraph(truncated_label)
                        .with_style(UiComponentStyles {
                            font_family_id: Some(appearance.monospace_font_family()),
                            font_color: Some(theme.active_ui_text_color().into()),
                            font_size: Some(appearance.monospace_font_size()),
                            font_weight: Some(Weight::Bold),
                            ..Default::default()
                        })
                        .build()
                        .finish(),
                )
                .with_padding_top(2.)
                .finish(),
            )
            .with_child(
                Container::new(
                    ui_builder
                        .paragraph(description.suggestion_type.to_name().to_string())
                        .with_style(UiComponentStyles {
                            font_family_id: Some(appearance.ui_font_family()),
                            font_color: Some(theme.active_ui_text_color().into()),
                            font_size: Some(appearance.monospace_font_size() * 0.75),
                            ..Default::default()
                        })
                        .build()
                        .finish(),
                )
                .with_background(theme.outline())
                .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)))
                .with_margin_left(TOKEN_DESCRIPTION_MARGIN)
                .with_padding_left(TOKEN_LABEL_HORIZONTAL_PADDING)
                .with_padding_right(TOKEN_LABEL_HORIZONTAL_PADDING)
                .with_padding_top(TOKEN_LABEL_VERTICAL_PADDING)
                .with_padding_bottom(TOKEN_LABEL_VERTICAL_PADDING)
                .finish(),
            )
            .finish(),
    );

    if let Some(description_text) = description.description_text.clone() {
        command_description.add_child(
            Container::new(
                ui_builder
                    .paragraph(description_text)
                    .with_style(UiComponentStyles {
                        font_family_id: Some(appearance.ui_font_family()),
                        font_color: Some(theme.sub_text_color(theme.surface_2()).into()),
                        font_size: Some(appearance.monospace_font_size() * 0.9),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            )
            .with_margin_top(TOKEN_DESCRIPTION_MARGIN)
            .finish(),
        );
    }

    ConstrainedBox::new(
        Container::new(command_description.finish())
            .with_uniform_padding(TOKEN_DESCRIPTION_PADDING)
            .with_margin_bottom(TOKEN_DESCRIPTION_MARGIN)
            .with_border(Border::all(1.).with_border_fill(theme.split_pane_border_color()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
            .with_background_color(theme.surface_2().into_solid())
            .finish(),
    )
    .with_width(TOKEN_DESCRIPTION_WIDTH)
    .finish()
}
