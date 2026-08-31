use warpui::elements::MouseStateHandle;
use warpui::fonts::Weight;
use warpui::platform::Cursor;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{Action, AppContext, Element, ViewHandle};

use crate::appearance::Appearance;
use crate::editor::EditorView;

pub const INPUT_BOX_FONT_SIZE: f32 = 14.;
pub const TEAM_BLOCK_INITIAL_HEIGHT: f32 = 92.;
pub const SKIP_BUTTON_WIDTH: f32 = 60.;
pub const SKIP_BUTTON_HEIGHT: f32 = 40.;
pub const BUTTON_GAP: f32 = 8.;

pub fn render_skip_button<A: Action + Clone>(
    action: A,
    mouse_state_handle: MouseStateHandle,
    appearance: &Appearance,
) -> Box<dyn Element> {
    appearance
        .ui_builder()
        .button(ButtonVariant::Secondary, mouse_state_handle.clone())
        .with_style(UiComponentStyles {
            font_color: Some(appearance.theme().surface_3().into()),
            font_weight: Some(Weight::Medium),
            width: Some(SKIP_BUTTON_WIDTH),
            height: Some(SKIP_BUTTON_HEIGHT),
            font_size: Some(14.),
            ..Default::default()
        })
        .with_hovered_styles(UiComponentStyles {
            background: Some(appearance.theme().outline().into()),
            ..Default::default()
        })
        .with_centered_text_label("Skip".to_owned())
        .build()
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish()
}

pub fn name(app: &AppContext, team_name_editor: ViewHandle<EditorView>) -> Option<String> {
    let name = team_name_editor.as_ref(app).buffer_text(app);
    if !name.trim().is_empty() {
        Some(name)
    } else {
        None
    }
}
