use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use warpui::{
    AppContext, Entity, EntityId, ModelContext, ModelHandle, SingletonEntity, WeakModelHandle,
    WindowId,
};

use crate::ai::agent::conversation::AIConversationId;
use crate::ai::agent_conversations_model::AgentConversationEntry;
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::ai::blocklist::agent_view::{AgentViewController, AgentViewControllerEvent};
use crate::terminal::model::session::active_session::ActiveSession;

/// Contains the handles needed to track an active agent view.
struct ActiveAgentViewHandles {
    controller: WeakModelHandle<AgentViewController>,
    active_session: WeakModelHandle<ActiveSession>,
}

#[derive(Clone)]
pub enum ActiveAgentViewsEvent {
    /// A conversation was closed (exited from the agent view or its pane was removed).
    ConversationClosed,
    /// A conversation was entered within a terminal view.
    TerminalViewFocused,
    /// A window was closed and its focused state was removed.
    WindowClosed,
}

/// State of the focused terminal view and the active conversation in that terminal view.
#[derive(Clone)]
struct FocusedTerminalState {
    focused_terminal_id: EntityId,
    active_conversation_id: Option<AIConversationId>,
}

/// ActiveAgentViewsModel tracks which agent conversations are currently "active" - meaning an
/// interactive conversation whose agent view is expanded in a pane. This model also tracks which conversation is focused (i.e. active in the currently focused pane).
pub struct ActiveAgentViewsModel {
    /// Per-window focused terminal state, keyed by WindowId.
    focused_terminal_states: HashMap<WindowId, FocusedTerminalState>,
    last_focused_terminal_state: Option<FocusedTerminalState>,
    /// Map from terminal_view_id to agent view handles (for interactive conversations).
    agent_view_handles: HashMap<EntityId, ActiveAgentViewHandles>,
    /// Tracks when each conversation was last opened/focused for sorting purposes.
    last_opened_times: HashMap<AIConversationId, DateTime<Utc>>,
}

impl Entity for ActiveAgentViewsModel {
    type Event = ActiveAgentViewsEvent;
}

impl SingletonEntity for ActiveAgentViewsModel {}

impl ActiveAgentViewsModel {
    pub fn new() -> Self {
        Self {
            focused_terminal_states: HashMap::new(),
            last_focused_terminal_state: None,
            agent_view_handles: HashMap::new(),
            last_opened_times: HashMap::new(),
        }
    }

    fn update_focused_conversation_for_terminal(
        &mut self,
        terminal_view_id: EntityId,
        conversation_id: Option<AIConversationId>,
    ) {
        for focused_terminal_state in self.focused_terminal_states.values_mut() {
            if focused_terminal_state.focused_terminal_id == terminal_view_id {
                focused_terminal_state.active_conversation_id = conversation_id;
            }
        }

        if let Some(last_focused_terminal_state) = &mut self.last_focused_terminal_state
            && last_focused_terminal_state.focused_terminal_id == terminal_view_id
        {
            last_focused_terminal_state.active_conversation_id = conversation_id;
        }
    }

    /// Register an agent view controller to track when the agent view is entered/exited.
    pub fn register_agent_view_controller(
        &mut self,
        controller: &ModelHandle<AgentViewController>,
        active_session: &ModelHandle<ActiveSession>,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        // Skip registering this controller if it is already registered.
        if let Some(existing) = self.agent_view_handles.get(&terminal_view_id)
            && existing
                .controller
                .upgrade(ctx)
                .is_some_and(|c| c.id() == controller.id())
        {
            return;
        }

        self.agent_view_handles.insert(
            terminal_view_id,
            ActiveAgentViewHandles {
                controller: controller.downgrade(),
                active_session: active_session.downgrade(),
            },
        );

        ctx.subscribe_to_model(controller, move |model, _, event, ctx| match event {
            AgentViewControllerEvent::EnteredAgentView {
                conversation_id, ..
            } => {
                model.last_opened_times.insert(*conversation_id, Utc::now());

                // Update the focused conversation in whichever window owns this terminal view.
                model.update_focused_conversation_for_terminal(
                    terminal_view_id,
                    Some(*conversation_id),
                );
                // Emit so subscribers can move this conversation to the Active section.
                ctx.emit(ActiveAgentViewsEvent::TerminalViewFocused);
            }
            AgentViewControllerEvent::ExitedAgentView {
                conversation_id,
                is_exit_before_new_entrance,
                ..
            } => {
                // Skip if this exit is part of an in-place switch — the follow-up
                // entrance will register the new conversation's consumer.
                if *is_exit_before_new_entrance {
                    return;
                }

                model.last_opened_times.remove(conversation_id);

                // Clear the focused conversation in whichever window owns this terminal view.
                model.update_focused_conversation_for_terminal(terminal_view_id, None);
                // Emit so subscribers can move this conversation to the Past section.
                ctx.emit(ActiveAgentViewsEvent::ConversationClosed);
            }
            _ => {}
        });
    }

    /// Unregister an agent view controller
    /// (called when the controller's terminal pane is hidden or closed).
    pub fn unregister_agent_view_controller(
        &mut self,
        terminal_pane_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        if let Some(handles) = self.agent_view_handles.remove(&terminal_pane_id) {
            let closed_conversation_id = handles
                .controller
                .upgrade(ctx)
                .and_then(|c| c.as_ref(ctx).agent_view_state().active_conversation_id());

            // If the focused terminal is the one being unregistered, clear the focused state.
            self.focused_terminal_states
                .retain(|_, state| state.focused_terminal_id != terminal_pane_id);
            if self
                .last_focused_terminal_state
                .as_ref()
                .is_some_and(|state| state.focused_terminal_id == terminal_pane_id)
            {
                self.last_focused_terminal_state = None;
            }

            if closed_conversation_id.is_some() {
                ctx.emit(ActiveAgentViewsEvent::ConversationClosed);
            }
        }
    }

    pub fn handle_pane_focus_change(
        &mut self,
        window_id: WindowId,
        focused_terminal_view_id: Option<EntityId>,
        ctx: &mut ModelContext<Self>,
    ) {
        let old_focused = self.get_focused_conversation(window_id);

        if let Some(terminal_view_id) = focused_terminal_view_id {
            let active_conversation_id = self
                .agent_view_handles
                .get(&terminal_view_id)
                .and_then(|handles| handles.controller.upgrade(ctx))
                .and_then(|controller| {
                    controller
                        .as_ref(ctx)
                        .agent_view_state()
                        .active_conversation_id()
                });

            let new_state = FocusedTerminalState {
                focused_terminal_id: terminal_view_id,
                active_conversation_id,
            };
            self.last_focused_terminal_state = Some(new_state.clone());
            self.focused_terminal_states.insert(window_id, new_state);
        } else {
            self.focused_terminal_states.remove(&window_id);
        }

        if old_focused != self.get_focused_conversation(window_id) {
            ctx.emit(ActiveAgentViewsEvent::TerminalViewFocused);
        }
    }

    /// Get the focused conversation for a specific window.
    /// Returns None if the window doesn't have an active agent view.
    pub fn get_focused_conversation(&self, window_id: WindowId) -> Option<AIConversationId> {
        self.focused_terminal_states
            .get(&window_id)
            .and_then(|state| state.active_conversation_id)
    }

    /// Returns the focused conversation ID if it's a new/empty conversation view.
    /// Only returns Some if the focused agent view was just created to start a new
    /// conversation (i.e. has no exchanges yet).
    pub fn maybe_get_focused_new_conversation(
        &self,
        window_id: WindowId,
        ctx: &AppContext,
    ) -> Option<AIConversationId> {
        let state = self.focused_terminal_states.get(&window_id)?;
        let terminal_id = state.focused_terminal_id;

        let is_new = self
            .agent_view_handles
            .get(&terminal_id)
            .and_then(|handles| handles.controller.upgrade(ctx))
            .map(|c| c.as_ref(ctx).agent_view_state().is_new())
            .unwrap_or(false);

        if is_new {
            state.active_conversation_id
        } else {
            None
        }
    }

    /// Remove the focused state for a window
    /// (called when said window is closed and cleaned up from the undo stack).
    pub fn remove_focused_state_for_window(
        &mut self,
        window_id: WindowId,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.focused_terminal_states.remove(&window_id).is_some() {
            ctx.emit(ActiveAgentViewsEvent::WindowClosed);
        }
    }

    /// Returns the terminal view ID for a conversation if it's currently active
    /// (i.e., has an expanded agent view in some pane).
    pub fn terminal_view_id_for_conversation(
        &self,
        conversation_id: AIConversationId,
        ctx: &AppContext,
    ) -> Option<EntityId> {
        self.agent_view_handles
            .iter()
            .find_map(|(terminal_view_id, handles)| {
                let controller = handles.controller.upgrade(ctx)?;
                controller
                    .as_ref(ctx)
                    .agent_view_state()
                    .active_conversation_id()
                    .is_some_and(|id| id == conversation_id)
                    .then_some(*terminal_view_id)
            })
    }

    /// Returns true if the conversation is currently open
    /// (i.e., has an expanded agent view in some pane).
    pub fn is_conversation_open(
        &self,
        conversation_id: AIConversationId,
        ctx: &AppContext,
    ) -> bool {
        self.terminal_view_id_for_conversation(conversation_id, ctx)
            .is_some()
    }

    /// Returns the active session for a conversation if it's currently active
    /// (i.e., has an expanded agent view).
    pub fn get_active_session_for_conversation(
        &self,
        conversation_id: AIConversationId,
        ctx: &AppContext,
    ) -> Option<ModelHandle<ActiveSession>> {
        for handles in self.agent_view_handles.values() {
            let Some(controller) = handles.controller.upgrade(ctx) else {
                continue;
            };
            let is_active = controller
                .as_ref(ctx)
                .agent_view_state()
                .active_conversation_id()
                .is_some_and(|id| id == conversation_id);
            if is_active {
                return handles.active_session.upgrade(ctx);
            }
        }
        None
    }

    /// Returns the controller for a conversation if it's currently active
    /// (i.e., has an expanded agent view).
    pub fn get_controller_for_conversation(
        &self,
        conversation_id: AIConversationId,
        ctx: &AppContext,
    ) -> Option<ModelHandle<AgentViewController>> {
        for handles in self.agent_view_handles.values() {
            if let Some(controller) = handles.controller.upgrade(ctx) {
                let is_active = controller
                    .as_ref(ctx)
                    .agent_view_state()
                    .active_conversation_id()
                    .is_some_and(|id| id == conversation_id);
                if is_active {
                    return Some(controller);
                }
            }
        }
        None
    }

    /// Returns the last opened time for a conversation, used for sorting active conversations.
    pub fn get_last_opened_time(&self, id: &AIConversationId) -> Option<DateTime<Utc>> {
        self.last_opened_times.get(id).copied()
    }

    /// Returns the terminal view ID that has an active conversation with the given ID.
    pub fn get_terminal_view_id_for_conversation(
        &self,
        conversation_id: AIConversationId,
        ctx: &AppContext,
    ) -> Option<EntityId> {
        for (terminal_view_id, handles) in &self.agent_view_handles {
            let Some(controller) = handles.controller.upgrade(ctx) else {
                continue;
            };
            let is_active = controller
                .as_ref(ctx)
                .agent_view_state()
                .active_conversation_id()
                .is_some_and(|id| id == conversation_id);
            if is_active {
                return Some(*terminal_view_id);
            }
        }

        None
    }

    pub fn get_terminal_view_id_for_entry(
        &self,
        entry: &AgentConversationEntry,
        ctx: &AppContext,
    ) -> Option<EntityId> {
        if let Some(conversation_id) = entry.identity.local_conversation_id {
            return self.get_terminal_view_id_for_conversation(conversation_id, ctx);
        }

        None
    }
    /// Get all currently active conversation IDs.
    /// A conversation is active if it is open and a query has been sent since it was last opened.
    /// New (empty) conversations are always considered active when open.
    pub fn get_all_active_conversation_ids(&self, ctx: &AppContext) -> HashSet<AIConversationId> {
        let history_model = BlocklistAIHistoryModel::as_ref(ctx);
        let mut ids = HashSet::new();

        for handles in self.agent_view_handles.values() {
            if let Some(controller) = handles.controller.upgrade(ctx) {
                let state = controller.as_ref(ctx).agent_view_state();
                if let Some(conversation_id) = state.active_conversation_id() {
                    let Some(conversation) = history_model.conversation(&conversation_id) else {
                        continue;
                    };
                    if !conversation.is_entirely_passive()
                        && state.was_conversation_modified_since_opening(history_model)
                    {
                        ids.insert(conversation_id);
                    }
                }
            }
        }

        ids
    }

    /// Get all currently open conversation IDs.
    /// A conversation is considered open if it is in an expanded agent view.
    pub fn get_all_open_conversation_ids(&self, ctx: &AppContext) -> HashSet<AIConversationId> {
        let mut ids = HashSet::new();

        // Collect from interactive agent views (expanded).
        for handles in self.agent_view_handles.values() {
            if let Some(controller) = handles.controller.upgrade(ctx)
                && let Some(conversation_id) = controller
                    .as_ref(ctx)
                    .agent_view_state()
                    .active_conversation_id()
            {
                ids.insert(conversation_id);
            }
        }

        ids
    }
}

#[cfg(test)]
#[path = "active_agent_views_model_tests.rs"]
mod tests;
