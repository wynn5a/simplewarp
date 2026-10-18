use warpui::{AppContext, ViewHandle};

use crate::editor::EditorView;

pub const BUTTON_GAP: f32 = 8.;

pub fn name(app: &AppContext, team_name_editor: ViewHandle<EditorView>) -> Option<String> {
    let name = team_name_editor.as_ref(app).buffer_text(app);
    if !name.trim().is_empty() {
        Some(name)
    } else {
        None
    }
}
