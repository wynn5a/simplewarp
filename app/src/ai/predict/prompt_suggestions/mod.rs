use std::sync::LazyLock;

use warpui::AppContext;
use warpui::keymap::Keystroke;

use crate::util::bindings::keybinding_name_to_keystroke;

pub const ACCEPT_PROMPT_SUGGESTION_KEYBINDING: &str = "terminal:accept_prompt_suggestions";

pub fn is_accept_prompt_suggestion_bound_to_cmd_enter(app: &AppContext) -> bool {
    static CMD_ENTER_KEYSTROKE: LazyLock<Keystroke> = LazyLock::new(|| Keystroke {
        cmd: true,
        key: "enter".to_owned(),
        ..Default::default()
    });
    keybinding_name_to_keystroke(ACCEPT_PROMPT_SUGGESTION_KEYBINDING, app)
        .is_some_and(|keystroke| keystroke == *CMD_ENTER_KEYSTROKE)
}

pub fn is_accept_prompt_suggestion_bound_to_ctrl_enter(app: &AppContext) -> bool {
    static CTRL_ENTER_KEYSTROKE: LazyLock<Keystroke> = LazyLock::new(|| Keystroke {
        ctrl: true,
        key: "enter".to_owned(),
        ..Default::default()
    });
    keybinding_name_to_keystroke(ACCEPT_PROMPT_SUGGESTION_KEYBINDING, app)
        .is_some_and(|keystroke| keystroke == *CTRL_ENTER_KEYSTROKE)
}
