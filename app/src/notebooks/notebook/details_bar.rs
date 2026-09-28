//! Components for the notebook header.

use warpui::Element;
use warpui::elements::{
    Container, CrossAxisAlignment, Flex, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    ParentElement, Shrinkable,
};
use warpui::platform::Cursor;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};

use super::{EDIT_BUTTON_MARGIN, NotebookAction};
use crate::appearance::Appearance;
use crate::notebooks::active_notebook_data::Mode;
use crate::notebooks::styles;
use crate::ui_components::buttons::{accent_icon_button, icon_button};
use crate::ui_components::icons::Icon;

/// Component showing the notebook's view/edit mode and its toggle.
pub struct DetailsBar {
    edit_mode_button_mouse_state: MouseStateHandle,
}

impl DetailsBar {
    pub fn new() -> Self {
        Self {
            edit_mode_button_mouse_state: Default::default(),
        }
    }

    pub fn render(&self, mode: Mode, appearance: &Appearance) -> Box<dyn Element> {
        let mut editing_state_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        editing_state_row
            .add_child(Shrinkable::new(1., self.render_mode_label(mode, appearance)).finish());
        editing_state_row.add_child(self.render_mode_toggle(mode, appearance));

        editing_state_row.finish()
    }

    /// Renders a toggle button for the editing mode.
    fn render_mode_toggle(&self, mode: Mode, appearance: &Appearance) -> Box<dyn Element> {
        let edit_button = match mode {
            Mode::View => icon_button(
                appearance,
                Icon::Pencil,
                false,
                self.edit_mode_button_mouse_state.clone(),
            ),
            Mode::Editing => accent_icon_button(
                appearance,
                Icon::Pencil,
                false,
                self.edit_mode_button_mouse_state.clone(),
            ),
        };

        Container::new(
            edit_button
                .build()
                .on_click(move |ctx, _, _| ctx.dispatch_typed_action(NotebookAction::ToggleMode))
                .with_cursor(Cursor::PointingHand)
                .finish(),
        )
        .with_margin_left(EDIT_BUTTON_MARGIN)
        .with_margin_right(EDIT_BUTTON_MARGIN)
        .finish()
    }

    fn render_mode_label(&self, mode: Mode, appearance: &Appearance) -> Box<dyn Element> {
        let label = match mode {
            Mode::View => "Viewing",
            Mode::Editing => "Editing",
        };
        appearance
            .ui_builder()
            .span(label)
            .with_style(UiComponentStyles {
                font_color: Some(styles::title_text_fill(appearance).into_solid()),
                ..Default::default()
            })
            .build()
            .finish()
    }
}
