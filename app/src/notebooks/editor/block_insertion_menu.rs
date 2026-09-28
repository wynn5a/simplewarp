use serde::{Deserialize, Serialize};
use warp_editor::content::text::BufferBlockItem;
use warpui::elements::{
    AnchorPair, Border, Container, CornerRadius, MouseStateHandle, OffsetPositioning, OffsetType,
    PositionedElementOffsetBounds, PositioningAxis, Radius, SavePosition, Stack, XAxisAnchor,
    YAxisAnchor,
};
use warpui::presenter::ChildView;
use warpui::ui_components::button::ButtonTooltipPosition;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{AppContext, Element, SingletonEntity, ViewContext, ViewHandle};

use super::BlockType;
use super::view::{EditorViewAction, RichTextEditorView};
use crate::appearance::Appearance;
use crate::menu::{self, Menu, MenuItemFields};
use crate::themes::theme::Fill;
use crate::ui_components::buttons::icon_button;
use crate::ui_components::icons::Icon;

/// The saved position ID for the block insertion button.
const BLOCK_INSERT_BUTTON_ID: &str = "notebook_block_insertion_button";

/// Where the block insertion menu was triggered from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockInsertionSource {
    AtCursor,
    BlockInsertionButton,
}

/// Editor view state related to the block insertion menu.
pub struct BlockInsertionMenuState {
    // If the menu is closed, this will be None.
    pub open_at_source: Option<BlockInsertionSource>,
    button_state: MouseStateHandle,
    pub menu: ViewHandle<Menu<EditorViewAction>>,
}

impl BlockInsertionMenuState {
    pub fn new(ctx: &mut ViewContext<RichTextEditorView>) -> Self {
        let menu = ctx.add_typed_action_view(Self::create_menu);

        ctx.subscribe_to_view(&menu, RichTextEditorView::handle_block_insertion_menu_event);

        Self {
            open_at_source: None,
            button_state: Default::default(),
            menu,
        }
    }

    fn create_menu(ctx: &mut ViewContext<Menu<EditorViewAction>>) -> Menu<EditorViewAction> {
        let appearance = Appearance::as_ref(ctx);
        let mut menu = Menu::new().prevent_interaction_with_other_elements();

        for block_type in BlockType::code_block_types() {
            menu.add_item(
                MenuItemFields::new(block_type.label())
                    .with_icon(block_type.icon())
                    .with_on_select_action(EditorViewAction::InsertBlock(
                        warp_editor::content::text::BlockType::Text(block_type.into()),
                    ))
                    .into_item(),
            );
        }

        for block_type in BlockType::text_block_types() {
            let mut item_fields = MenuItemFields::new(block_type.label())
                .with_icon(block_type.icon())
                .with_on_select_action(EditorViewAction::InsertBlock(
                    warp_editor::content::text::BlockType::Text(block_type.into()),
                ));
            if let Some(icon_fill) = block_type.icon_color(appearance) {
                item_fields = item_fields.with_override_icon_color(icon_fill);
            }
            menu.add_item(item_fields.into_item());
        }

        menu.add_item(
            MenuItemFields::new("Divider")
                .with_icon(Icon::HorizontalRuleBlock)
                .with_on_select_action(EditorViewAction::InsertBlock(
                    warp_editor::content::text::BlockType::Item(BufferBlockItem::HorizontalRule),
                ))
                .with_override_icon_color(Fill::Solid(appearance.theme().ui_warning_color()))
                .into_item(),
        );

        menu
    }

    pub fn reset_selection(&mut self, ctx: &mut AppContext) {
        self.menu.update(ctx, |menu, ctx| {
            menu.reset_selection(ctx);
        })
    }
}

impl RichTextEditorView {
    /// Open the block insertion menu.
    pub(super) fn open_block_insertion_menu(
        &mut self,
        source: BlockInsertionSource,
        ctx: &mut ViewContext<Self>,
    ) {
        // Reset selection if we are opening a new block insertion menu or opening
        // the menu from a different source.
        if self.insertion_menu_state.open_at_source != Some(source) {
            self.insertion_menu_state.reset_selection(ctx);
            ctx.notify();
        }
        self.insertion_menu_state.open_at_source = Some(source);
        ctx.focus(&self.insertion_menu_state.menu);
    }

    /// Close the block insertion menu.
    pub(super) fn close_block_insertion_menu(&mut self, ctx: &mut ViewContext<Self>) {
        if self.is_block_insertion_menu_open() {
            ctx.notify();
        }
        self.insertion_menu_state.open_at_source = None;
        ctx.focus_self();
    }

    /// Whether the block insertion menu is open.
    pub(super) fn is_block_insertion_menu_open(&self) -> bool {
        self.insertion_menu_state.open_at_source.is_some()
    }

    /// Callback for events on the block insertion menu.
    fn handle_block_insertion_menu_event(
        &mut self,
        _menu: ViewHandle<Menu<EditorViewAction>>,
        event: &menu::Event,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            menu::Event::ItemSelected | menu::Event::ItemHovered => (),
            menu::Event::Close { via_select_item } => {
                self.close_block_insertion_menu(ctx);
                if !*via_select_item {
                    ctx.focus_self()
                }
            }
        }
    }

    /// Renders controls for the block insertion menu.
    pub(super) fn render_block_insertion_menu(&self, stack: &mut Stack, app: &AppContext) {
        if self.disable_block_insertion_menu() {
            return;
        }

        if self.can_edit_app(app) {
            self.render_button(stack, app);
        }

        if let Some(source) = self.insertion_menu_state.open_at_source {
            self.render_menu(source, stack, app);
        }
    }

    /// Renders a button that opens the block insertion menu when clicked.
    fn render_button(&self, stack: &mut Stack, app: &AppContext) {
        let appearance = Appearance::as_ref(app);
        let ui_builder = appearance.ui_builder().clone();
        let button = icon_button(
            appearance,
            Icon::Plus,
            self.insertion_menu_state.open_at_source
                == Some(BlockInsertionSource::BlockInsertionButton),
            self.insertion_menu_state.button_state.clone(),
        )
        .with_active_styles(UiComponentStyles {
            background: Some(appearance.theme().surface_2().into()),
            border_color: Some(appearance.theme().surface_3().into()),
            ..Default::default()
        })
        .with_tooltip(move || {
            ui_builder
                .tool_tip("Insert block".to_string())
                .build()
                .finish()
        })
        // Position the tooltip above the insertion button to ensure they don't overlap if the
        // button is towards the bottom of the screen.
        .with_tooltip_position(ButtonTooltipPosition::Above)
        .build()
        .on_click(|ctx, _, _| ctx.dispatch_typed_action(EditorViewAction::OpenBlockInsertionMenu))
        .finish();

        let render_state = self.model.as_ref(app).render_state();
        let hovered_block_id = render_state
            .as_ref(app)
            .saved_positions()
            .hovered_block_start();

        stack.add_positioned_child(
            SavePosition::new(button, BLOCK_INSERT_BUTTON_ID).finish(),
            OffsetPositioning::from_axes(
                PositioningAxis::relative_to_stack_child(
                    &hovered_block_id,
                    PositionedElementOffsetBounds::Unbounded,
                    OffsetType::Pixel(-4.),
                    AnchorPair::new(XAxisAnchor::Left, XAxisAnchor::Right),
                )
                .with_conditional_anchor(),
                PositioningAxis::relative_to_stack_child(
                    hovered_block_id,
                    PositionedElementOffsetBounds::Unbounded,
                    OffsetType::Pixel(0.),
                    AnchorPair::new(YAxisAnchor::Middle, YAxisAnchor::Middle),
                )
                .with_conditional_anchor(),
            ),
        );
    }

    /// Renders a menu for inserting new kinds of blocks.
    fn render_menu(&self, source: BlockInsertionSource, stack: &mut Stack, app: &AppContext) {
        let appearance = Appearance::as_ref(app);
        let render_state = self.model.as_ref(app).render_state.as_ref(app);

        let menu = ChildView::new(&self.insertion_menu_state.menu).finish();
        let container = Container::new(menu)
            .with_border(Border::all(1.).with_border_fill(appearance.theme().outline()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .finish();
        let bounds = PositionedElementOffsetBounds::ParentByPosition;

        let positioning = match source {
            BlockInsertionSource::BlockInsertionButton => OffsetPositioning::from_axes(
                PositioningAxis::relative_to_stack_child(
                    BLOCK_INSERT_BUTTON_ID,
                    bounds,
                    OffsetType::Pixel(0.),
                    AnchorPair::new(XAxisAnchor::Left, XAxisAnchor::Left),
                ),
                PositioningAxis::relative_to_stack_child(
                    BLOCK_INSERT_BUTTON_ID,
                    bounds,
                    OffsetType::Pixel(4.),
                    AnchorPair::new(YAxisAnchor::Bottom, YAxisAnchor::Top),
                ),
            ),
            BlockInsertionSource::AtCursor => {
                let cursor_position = render_state.saved_positions().cursor_id();
                OffsetPositioning::from_axes(
                    PositioningAxis::relative_to_stack_child(
                        &cursor_position,
                        bounds,
                        OffsetType::Pixel(0.),
                        AnchorPair::new(XAxisAnchor::Left, XAxisAnchor::Left),
                    )
                    .with_conditional_anchor(),
                    PositioningAxis::relative_to_stack_child(
                        &cursor_position,
                        PositionedElementOffsetBounds::WindowByPosition,
                        OffsetType::Pixel(4.),
                        // TODO: Decide if this should be above or below the cursor based
                        // on its location within the viewport.
                        AnchorPair::new(YAxisAnchor::Bottom, YAxisAnchor::Top),
                    )
                    .with_conditional_anchor(),
                )
            }
        };

        stack.add_positioned_overlay_child(container, positioning);
    }
}
