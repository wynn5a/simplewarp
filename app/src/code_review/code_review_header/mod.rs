mod header_revamp;

use warpui::elements::{ChildView, ConstrainedBox, Container};
use warpui::ui_components::components::Coords;
use warpui::{Element, ViewHandle};

use crate::appearance::Appearance;
use crate::view_components::action_button::ActionButton;

pub(crate) const HEADER_BUTTON_PADDING: Coords = Coords {
    top: 2.,
    bottom: 2.,
    left: 6.,
    right: 6.,
};

pub struct CodeReviewHeader;

impl CodeReviewHeader {
    pub fn new() -> Self {
        Self
    }

    pub(super) fn render_maximize_pane_button(
        &self,
        maximize_button: &ViewHandle<ActionButton>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        Container::new(
            ConstrainedBox::new(ChildView::new(maximize_button).finish())
                .with_height(appearance.ui_font_size() + 10.)
                .with_width(appearance.ui_font_size() + 10.)
                .finish(),
        )
        .with_margin_left(8.)
        .with_margin_right(6.)
        .finish()
    }
}
