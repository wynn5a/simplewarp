//! Shared orchestration controls reused by the `RunAgentsCardView`
//! confirmation card editor and the plan-card
//! `OrchestrationConfigBlockView`.
//!
//! The generic parameter `A` is the parent view's typed action — both
//! consumers impl [`OrchestrationControlAction`] to provide the mapping
//! from field-change events to their own action enum.

use pathfinder_color::ColorU;
use pathfinder_geometry::vector::{Vector2F, vec2f};
use warp_cli::agent::Harness;
use warp_core::ui::theme::Fill;
use warpui::elements::{
    ChildView, Container, CornerRadius, CrossAxisAlignment, Empty, Flex, MainAxisSize,
    ParentElement, Point, Radius, Text,
};
use warpui::event::DispatchedEvent;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{Coords, UiComponentStyles};
use warpui::{
    AfterLayoutContext, AppContext, Element, EventContext, LayoutContext, PaintContext,
    SingletonEntity, SizeConstraint, View, ViewContext, ViewHandle,
};

use crate::LLMPreferences;
use crate::ai::execution_profiles::model_menu_items::available_model_menu_items;
use crate::ai::harness_availability::HarnessAvailabilityModel;
use crate::ai::harness_display;
use crate::ai::orchestration::{OptionRow, OptionSnapshot, harness_snapshot, model_snapshot};
pub use crate::ai::orchestration::{
    OrchestrationConfigState, OrchestrationEditState, accept_disabled_reason_with_setup,
};
use crate::appearance::Appearance;
use crate::menu::{MenuItem, MenuItemFields};
use crate::ui_components::blended_colors;
use crate::view_components::FilterableDropdown;
use crate::view_components::dropdown::{
    Dropdown, DropdownAction, DropdownItemAction, DropdownStyle,
};

// ── Shared constants ────────────────────────────────────────────────

pub const ORCHESTRATION_PICKER_HEIGHT: f32 = 36.;
pub const ORCHESTRATION_PICKER_BORDER_WIDTH: f32 = 1.;
pub const ORCHESTRATION_PICKER_FONT_SIZE: f32 = 14.;
pub const ORCHESTRATION_PICKER_RADIUS: f32 = 4.;
pub const ORCHESTRATION_PICKER_MAX_WIDTH: f32 = 205.;

// ── Action trait ────────────────────────────────────────────────────

/// Trait that both `RunAgentsCardViewAction` and
/// `OrchestrationConfigBlockAction` implement so the shared picker
/// creation and render helpers can produce the correct action variant.
pub trait OrchestrationControlAction: DropdownItemAction + Clone {
    fn model_changed(model_id: String) -> Self;
    fn harness_changed(harness_type: String) -> Self;
}

// ── Picker handles ──────────────────────────────────────────────────

/// Picker view handles shared between card editor and plan-card config
/// block. Generic over the action type `A`.
#[derive(Clone)]
pub struct OrchestrationPickerHandles<A: OrchestrationControlAction> {
    pub model_picker: Option<ViewHandle<FilterableDropdown<A>>>,
    pub harness_picker: Option<ViewHandle<Dropdown<A>>>,
}

impl<A: OrchestrationControlAction> Default for OrchestrationPickerHandles<A> {
    fn default() -> Self {
        Self {
            model_picker: None,
            harness_picker: None,
        }
    }
}

// ── Picker styling ──────────────────────────────────────────────────

/// Constructs the shared `UiComponentStyles` for orchestration pickers.
pub fn picker_styles(appearance: &Appearance) -> (UiComponentStyles, PickerColors) {
    let theme = appearance.theme();
    let padding = Coords {
        top: 8.,
        bottom: 8.,
        left: 12.,
        right: 12.,
    };
    let corner_radius = CornerRadius::with_all(Radius::Pixels(ORCHESTRATION_PICKER_RADIUS));
    // The picker bg is a translucent overlay (surface_overlay_1 =
    // fg at 5%). It must stay translucent so that the accent-tinted
    // card background in the config block shows through, and so that
    // gradient-background themes render correctly.
    let background_fill: Fill = theme.surface_overlay_1();
    let background: warpui::elements::Fill = background_fill.into();
    // Border and font colors are intentionally left to the dropdown's
    // default ButtonVariant::Secondary styling, which uses
    // theme.outline() and theme.main_text_color() — both are
    // contrast-aware and adapt correctly to all themes.

    let styles = UiComponentStyles {
        height: Some(ORCHESTRATION_PICKER_HEIGHT),
        background: Some(background),
        border_width: Some(ORCHESTRATION_PICKER_BORDER_WIDTH),
        border_radius: Some(corner_radius),
        font_size: Some(ORCHESTRATION_PICKER_FONT_SIZE),
        padding: Some(padding),
        ..Default::default()
    };
    let colors = PickerColors {
        padding,
        corner_radius,
        background,
    };
    (styles, colors)
}

#[derive(Clone)]
pub struct PickerColors {
    pub padding: Coords,
    pub corner_radius: CornerRadius,
    pub background: warpui::elements::Fill,
}

// ── Picker creation (generic over action type) ──────────────────────

/// Creates a standard dropdown with the shared orchestration picker
/// chrome (border, radius, background, font).
pub fn new_standard_picker_dropdown<A: OrchestrationControlAction, V: View>(
    colors: &PickerColors,
    ctx: &mut ViewContext<V>,
) -> ViewHandle<Dropdown<A>> {
    let padding = colors.padding;
    let corner_radius = colors.corner_radius;
    let background = colors.background;
    ctx.add_typed_action_view(move |ctx_dropdown| {
        let mut dropdown = Dropdown::<A>::new(ctx_dropdown);
        dropdown.set_use_overlay_layer(false, ctx_dropdown);
        dropdown.set_match_menu_width_to_top_bar(true, ctx_dropdown);
        dropdown.set_main_axis_size(MainAxisSize::Max, ctx_dropdown);
        dropdown.set_style(DropdownStyle::ActionButtonSecondary, ctx_dropdown);
        dropdown.set_top_bar_height(ORCHESTRATION_PICKER_HEIGHT, ctx_dropdown);
        dropdown.set_top_bar_max_width(f32::INFINITY);
        dropdown.set_padding(padding, ctx_dropdown);
        dropdown.set_border_radius(corner_radius, ctx_dropdown);
        dropdown.set_background(background, ctx_dropdown);
        dropdown.set_border_width(ORCHESTRATION_PICKER_BORDER_WIDTH, ctx_dropdown);
        dropdown.set_font_size(ORCHESTRATION_PICKER_FONT_SIZE, ctx_dropdown);
        dropdown
    })
}

/// Creates a searchable dropdown with the shared orchestration picker
/// chrome (border, radius, background, font).
pub fn new_standard_filterable_picker_dropdown<A: OrchestrationControlAction, V: View>(
    styles: &UiComponentStyles,
    ctx: &mut ViewContext<V>,
) -> ViewHandle<FilterableDropdown<A>> {
    let styles = *styles;
    ctx.add_typed_action_view(move |ctx_dropdown| {
        let mut dropdown = FilterableDropdown::<A>::new(ctx_dropdown);
        dropdown.set_use_overlay_layer(false, ctx_dropdown);
        dropdown.set_match_menu_width_to_top_bar(true, ctx_dropdown);
        dropdown.set_main_axis_size(MainAxisSize::Max, ctx_dropdown);
        dropdown.set_button_variant(ButtonVariant::Secondary);
        dropdown.set_style(styles);
        dropdown.set_top_bar_height(ORCHESTRATION_PICKER_HEIGHT, ctx_dropdown);
        dropdown.set_top_bar_max_width(f32::INFINITY);
        dropdown
    })
}

/// Label of the snapshot row matching `selected_id`, if any.
fn selected_row_label(snapshot: &OptionSnapshot) -> Option<String> {
    snapshot.selected_id.as_ref().and_then(|id| {
        snapshot
            .rows
            .iter()
            .find(|row| &row.id == id)
            .map(|row| row.label.clone())
    })
}

/// Rich menu items for Oz model rows. The snapshot owns inclusion,
/// ordering, and selection; this maps each row id back to its `LLMInfo`
/// and renders through [`available_model_menu_items`] so Oz rows keep
/// provider/credential icons and disabled gating — GUI rendering
/// concerns that cannot live in the frontend-neutral snapshot layer.
fn oz_model_menu_items<A: OrchestrationControlAction, V: View>(
    rows: &[OptionRow],
    ctx: &mut ViewContext<V>,
) -> Vec<MenuItem<DropdownAction>> {
    let llm_prefs = LLMPreferences::as_ref(ctx);
    let all_choices: Vec<_> = llm_prefs.get_base_llm_choices_for_agent_mode().collect();
    let ordered_choices: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            all_choices
                .iter()
                .copied()
                .find(|llm| llm.id.to_string() == row.id)
        })
        .collect();
    available_model_menu_items(
        ordered_choices,
        move |llm| DropdownAction::select_action_and_close(A::model_changed(llm.id.to_string())),
        None,
        None,
        false,
        false,
        ctx,
    )
}

/// Populates the model picker from [`model_snapshot`] for the active
/// harness (Warp LLM catalog for Oz, "Default model" for Codex,
/// "Default model" plus the harness catalog otherwise).
pub fn populate_model_picker_for_harness<A: OrchestrationControlAction, V: View>(
    dropdown: &ViewHandle<FilterableDropdown<A>>,
    initial_model_id: &str,
    harness_type: &str,
    ctx: &mut ViewContext<V>,
) {
    let state = OrchestrationConfigState::from_run_agents_fields(
        Some(initial_model_id),
        Some(harness_type),
    );
    let is_oz = matches!(
        Harness::parse_orchestration_harness(harness_type),
        Some(Harness::Oz) | None
    );
    dropdown.update(ctx, |dropdown, ctx_dropdown| {
        let snapshot = model_snapshot(&state, ctx_dropdown);
        let selected_label = selected_row_label(&snapshot);
        let items = if is_oz {
            oz_model_menu_items::<A, _>(&snapshot.rows, ctx_dropdown)
        } else {
            snapshot
                .rows
                .into_iter()
                .map(|row| {
                    MenuItem::Item(MenuItemFields::new(&row.label).with_on_select_action(
                        DropdownAction::select_action_and_close(A::model_changed(row.id)),
                    ))
                })
                .collect()
        };
        dropdown.set_rich_items(items, ctx_dropdown);
        if let Some(label) = &selected_label {
            dropdown.set_selected_by_name(label, ctx_dropdown);
        }
    });
}

/// Populates the harness picker from [`harness_snapshot`], mapping rows
/// to menu items (icon/brand color from the row's harness, disabled
/// reason to a disabled item with a tooltip).
pub fn populate_harness_picker<A: OrchestrationControlAction, V: View>(
    dropdown: &ViewHandle<Dropdown<A>>,
    initial_harness: &str,
    ctx: &mut ViewContext<V>,
) {
    let state = OrchestrationConfigState::from_run_agents_fields(None, Some(initial_harness));
    dropdown.update(ctx, |dropdown, ctx_dropdown| {
        let snapshot = harness_snapshot(&state, ctx_dropdown);
        let selected_label = selected_row_label(&snapshot);
        let items: Vec<MenuItem<DropdownAction>> = snapshot
            .rows
            .into_iter()
            .map(|row| {
                let mut fields = MenuItemFields::new(&row.label);
                if let Some(harness) = row.harness {
                    fields = fields.with_icon(harness_display::icon_for(harness));
                    if let Some(color) = harness_display::brand_color(harness) {
                        fields = fields.with_override_icon_color(Fill::from(color));
                    }
                }
                match row.disabled_reason {
                    Some(reason) => {
                        fields = fields.with_disabled(true).with_tooltip(reason);
                    }
                    None => {
                        fields = fields.with_on_select_action(
                            DropdownAction::select_action_and_close(A::harness_changed(row.id)),
                        );
                    }
                }
                MenuItem::Item(fields)
            })
            .collect();
        dropdown.set_rich_items(items, ctx_dropdown);
        if let Some(label) = &selected_label {
            dropdown.set_selected_by_name(label, ctx_dropdown);
        }
    });
}

/// Handles a harness change for both card views: applies the shared
/// [`OrchestrationEditState::apply_harness_change`] transition, then
/// repopulates the affected pickers.
///
/// Does NOT re-enter the harness picker that dispatched this action
/// (unless local sanitization changed the harness out from under it).
pub fn apply_harness_change<A: OrchestrationControlAction, V: View>(
    orchestration_edit_state: &mut OrchestrationEditState,
    handles: &OrchestrationPickerHandles<A>,
    new_harness_type: &str,
    fallback_base_model_id: Option<String>,
    ctx: &mut ViewContext<V>,
) {
    orchestration_edit_state.apply_harness_change(new_harness_type, fallback_base_model_id, ctx);
    let state = &orchestration_edit_state.orchestration_config_state;
    if state.harness_type != new_harness_type
        && let Some(handle) = &handles.harness_picker
    {
        populate_harness_picker(handle, &state.harness_type, ctx);
    }
    if let Some(handle) = &handles.model_picker {
        populate_model_picker_for_harness(handle, &state.model_id, &state.harness_type, ctx);
    }
}

// ── Picker repopulation + selection sync ──

/// Revalidates the edit state against the latest catalogs via
/// [`OrchestrationConfigState::revalidate_after_catalog_change`], then
/// repopulates every picker from the current catalogs and re-syncs
/// dropdown selections.
pub fn repopulate_all_pickers<A: OrchestrationControlAction, V: View>(
    state: &mut OrchestrationConfigState,
    handles: &OrchestrationPickerHandles<A>,
    ctx: &mut ViewContext<V>,
) {
    state.revalidate_after_catalog_change(ctx);
    if let Some(handle) = &handles.harness_picker {
        populate_harness_picker(handle, &state.harness_type, ctx);
    }
    if let Some(handle) = &handles.model_picker {
        populate_model_picker_for_harness(handle, &state.model_id, &state.harness_type, ctx);
    }
    sync_picker_selections(state, handles, ctx);
}

pub fn sync_picker_selections<A: OrchestrationControlAction, V: View>(
    state: &OrchestrationConfigState,
    handles: &OrchestrationPickerHandles<A>,
    ctx: &mut ViewContext<V>,
) {
    if let Some(model_picker) = handles.model_picker.clone() {
        let snapshot = model_snapshot(state, ctx);
        if let Some(label) = selected_row_label(&snapshot) {
            model_picker.update(ctx, |dropdown, ctx_dropdown| {
                dropdown.set_selected_by_name(&label, ctx_dropdown);
            });
        }
    }
    if let Some(harness_picker) = handles.harness_picker.clone() {
        let harness_type = state.harness_type.clone();
        harness_picker.update(ctx, |dropdown, ctx_dropdown| {
            let target = Harness::parse_orchestration_harness(&harness_type).unwrap_or(Harness::Oz);
            // Use the server-provided display_name from HarnessAvailabilityModel
            // so the selection matches the labels (which also use display_name).
            let display = HarnessAvailabilityModel::as_ref(ctx_dropdown)
                .display_name_for(target)
                .to_string();
            dropdown.set_selected_by_name(&display, ctx_dropdown);
        });
    }
}

// ── Adaptive picker layout ──────────────────────────────────────────

/// Lays out children horizontally at a fixed width when they all fit,
/// otherwise stacks them vertically at full available width.
///
/// Switches to vertical when `n * picker_width + (n-1) * spacing` exceeds
/// the available width from the incoming size constraint.
struct AdaptivePickerRow {
    children: Vec<Box<dyn Element>>,
    picker_width: f32,
    spacing: f32,
    is_vertical: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl AdaptivePickerRow {
    fn new(picker_width: f32, spacing: f32) -> Self {
        Self {
            children: Vec::new(),
            picker_width,
            spacing,
            is_vertical: false,
            size: None,
            origin: None,
        }
    }

    fn add_child(&mut self, child: Box<dyn Element>) {
        self.children.push(child);
    }

    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for AdaptivePickerRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let n = self.children.len();
        if n == 0 {
            self.size = Some(Vector2F::zero());
            return Vector2F::zero();
        }

        let total_horizontal =
            self.picker_width * n as f32 + self.spacing * n.saturating_sub(1) as f32;

        self.is_vertical = total_horizontal > constraint.max.x();

        if self.is_vertical {
            let width = constraint.max.x();
            let mut total_height = 0.0f32;
            for (i, child) in self.children.iter_mut().enumerate() {
                if i > 0 {
                    total_height += self.spacing;
                }
                let child_constraint =
                    SizeConstraint::new(vec2f(width, 0.), vec2f(width, f32::INFINITY));
                let child_size = child.layout(child_constraint, ctx, app);
                total_height += child_size.y();
            }
            let size = vec2f(width, total_height);
            self.size = Some(size);
            size
        } else {
            let mut max_height = 0.0f32;
            for child in self.children.iter_mut() {
                let child_constraint = SizeConstraint::new(
                    vec2f(self.picker_width, 0.),
                    vec2f(self.picker_width, f32::INFINITY),
                );
                let child_size = child.layout(child_constraint, ctx, app);
                max_height = max_height.max(child_size.y());
            }
            let size = vec2f(total_horizontal, max_height);
            self.size = Some(size);
            size
        }
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        for child in &mut self.children {
            child.after_layout(ctx, app);
        }
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let mut current = origin;
        if self.is_vertical {
            for (i, child) in self.children.iter_mut().enumerate() {
                if i > 0 {
                    current += vec2f(0., self.spacing);
                }
                child.paint(current, ctx, app);
                if let Some(size) = child.size() {
                    current += vec2f(0., size.y());
                }
            }
        } else {
            for (i, child) in self.children.iter_mut().enumerate() {
                if i > 0 {
                    current += vec2f(self.spacing, 0.);
                }
                child.paint(current, ctx, app);
                let advance = child.size().map_or(self.picker_width, |s| s.x());
                current += vec2f(advance, 0.);
            }
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        let mut handled = false;
        for child in &mut self.children {
            handled |= child.dispatch_event(event, ctx, app);
        }
        handled
    }
}

// ── Render helpers ──────────────────────────────────────────────────

pub fn render_picker_row<A: OrchestrationControlAction>(
    handles: &OrchestrationPickerHandles<A>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    render_picker_row_with_layout(handles, appearance, false)
}

/// Renders pickers vertically at full width when `vertical` is true,
/// or in the original horizontal layout when false.
pub fn render_picker_row_with_layout<A: OrchestrationControlAction>(
    handles: &OrchestrationPickerHandles<A>,
    appearance: &Appearance,
    vertical: bool,
) -> Box<dyn Element> {
    let harness_picker = handles
        .harness_picker
        .as_ref()
        .map(|p| ChildView::new(p).finish());
    let model_picker = handles
        .model_picker
        .as_ref()
        .map(|p| ChildView::new(p).finish());

    if vertical {
        let column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(12.)
            .with_child(render_picker_column(
                "Agent harness",
                harness_picker,
                appearance,
            ))
            .with_child(render_picker_column("Base model", model_picker, appearance));

        Container::new(column.finish())
            .with_margin_top(12.)
            .finish()
    } else {
        let mut row = AdaptivePickerRow::new(ORCHESTRATION_PICKER_MAX_WIDTH, 12.);
        row.add_child(render_picker_column(
            "Agent harness",
            harness_picker,
            appearance,
        ));
        row.add_child(render_picker_column("Base model", model_picker, appearance));

        Container::new(row.finish()).with_margin_top(12.).finish()
    }
}

pub fn render_picker_column(
    label: &str,
    picker: Option<Box<dyn Element>>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let label_el = Text::new(
        label.to_string(),
        appearance.ui_font_family(),
        appearance.monospace_font_size() - 1.,
    )
    .with_color(blended_colors::text_disabled(theme, theme.surface_1()))
    .finish();

    let body: Box<dyn Element> = picker.unwrap_or_else(|| Empty::new().finish());
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(label_el)
        .with_child(body)
        .finish()
}

pub fn render_validation_error(
    reason: impl Into<String>,
    color: ColorU,
    appearance: &Appearance,
) -> Box<dyn Element> {
    Container::new(
        Text::new(
            reason.into(),
            appearance.ui_font_family(),
            appearance.monospace_font_size() - 1.,
        )
        .with_color(color)
        .finish(),
    )
    .with_margin_bottom(8.)
    .finish()
}
