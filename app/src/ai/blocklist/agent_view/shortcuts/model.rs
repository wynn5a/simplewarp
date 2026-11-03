use warpui::{Entity, ModelContext, ModelHandle};

use crate::ai::blocklist::agent_view::{AgentViewController, AgentViewControllerEvent};
use crate::terminal::input::buffer_model::InputBufferModel;

/// Model responsible for managing state required to conditionally render the shortcuts view.
pub struct AgentShortcutViewModel {
    is_shortcut_view_open: bool,
}

impl AgentShortcutViewModel {
    pub fn new(
        input_buffer_model: ModelHandle<InputBufferModel>,
        agent_view_controller: ModelHandle<AgentViewController>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        ctx.subscribe_to_model(&input_buffer_model, |me, _, event, _| {
            if me.is_shortcut_view_open && !event.new_content.is_empty() {
                me.hide_shortcut_view();
            }
        });
        ctx.subscribe_to_model(&agent_view_controller, |me, _, event, _| {
            if matches!(event, AgentViewControllerEvent::ExitedAgentView { .. }) {
                me.hide_shortcut_view();
            }
        });

        Self {
            is_shortcut_view_open: false,
        }
    }

    pub fn is_shortcut_view_open(&self) -> bool {
        self.is_shortcut_view_open
    }

    pub fn toggle_shortcut_view(&mut self) {
        self.is_shortcut_view_open = !self.is_shortcut_view_open;
    }

    fn hide_shortcut_view(&mut self) {
        self.is_shortcut_view_open = false;
    }
}

impl Entity for AgentShortcutViewModel {
    type Event = ();
}
