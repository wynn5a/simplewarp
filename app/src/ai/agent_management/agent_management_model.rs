use warpui::{AppContext, Entity, EntityId, ModelContext, SingletonEntity, WindowId};

use crate::BlocklistAIHistoryModel;
use crate::ai::agent::conversation::{AIConversationId, ConversationStatus};
use crate::ai::blocklist::{BlocklistAIHistoryEvent, ConversationStatusUpdate};
use crate::workspace::util::is_terminal_view_in_same_tab;
use crate::workspace::{Workspace, WorkspaceRegistry};

/// Singleton model responsible for triggering an in-app toast when a conversation that is not
/// visible reaches a blocking status.
pub struct AgentNotificationsModel;

impl Entity for AgentNotificationsModel {
    type Event = AgentManagementEvent;
}

impl SingletonEntity for AgentNotificationsModel {}

impl AgentNotificationsModel {
    pub(crate) fn new(ctx: &mut ModelContext<Self>) -> Self {
        let history_model = BlocklistAIHistoryModel::handle(ctx);
        ctx.subscribe_to_model(&history_model, move |me, _, event, ctx| {
            me.handle_history_event(event, ctx);
        });
        Self
    }

    fn handle_history_event(
        &mut self,
        event: &BlocklistAIHistoryEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let BlocklistAIHistoryEvent::UpdatedConversationStatus {
            terminal_surface_id,
            conversation_id,
            // We shouldn't trigger toasts when restoring conversations on startup.
            update: ConversationStatusUpdate::Changed { .. },
            ..
        } = event
        else {
            return;
        };

        let ai_history_model = BlocklistAIHistoryModel::as_ref(ctx);
        let Some(updated_conversation) = ai_history_model.conversation(conversation_id) else {
            return;
        };

        if updated_conversation.should_exclude_from_navigation() {
            return;
        }

        if !updated_conversation.status().should_trigger_notification() {
            return;
        }

        if is_terminal_view_visible(*terminal_surface_id, ctx) {
            return;
        }

        let Some((window_id, tab_index)) =
            window_and_tab_idx_id_for_conversation(*conversation_id, ctx)
        else {
            return;
        };

        ctx.emit(AgentManagementEvent::ConversationNeedsAttention {
            window_id,
            tab_index,
            terminal_view_id: *terminal_surface_id,
            conversation_id: *conversation_id,
        });
    }
}

#[derive(Clone, Debug)]
pub enum AgentManagementEvent {
    /// A Warp-native conversation needs attention and is not visible in the current window/tab.
    ConversationNeedsAttention {
        window_id: WindowId,
        tab_index: usize,
        terminal_view_id: EntityId,
        conversation_id: AIConversationId,
    },
}

impl ConversationStatus {
    /// Returns true if the updating the conversation with this status should trigger some
    /// notification to the user.
    ///
    /// Exhaustive match so a new `ConversationStatus` variant forces a
    /// deliberate decision about whether it should fire a notification.
    pub fn should_trigger_notification(&self) -> bool {
        match self {
            ConversationStatus::Success
            | ConversationStatus::Blocked { .. }
            | ConversationStatus::Error => true,
            // Streaming hasn't reached a notable state; a recovering or
            // yielded conversation is still active; user-cancellations are
            // self-evident.
            ConversationStatus::InProgress
            | ConversationStatus::TransientError
            | ConversationStatus::WaitingForEvents
            | ConversationStatus::Cancelled => false,
        }
    }
}

fn is_terminal_view_visible(terminal_view_id: EntityId, app: &AppContext) -> bool {
    let Some(active_id) = active_focused_terminal_id(app) else {
        return false;
    };
    active_id == terminal_view_id
        || is_terminal_view_in_same_tab(&active_id, &terminal_view_id, app)
}

fn window_and_tab_idx_id_for_conversation(
    conversation_id: AIConversationId,
    app: &AppContext,
) -> Option<(WindowId, usize)> {
    WorkspaceRegistry::as_ref(app)
        .all_workspaces(app)
        .iter()
        .find_map(|(window_id, workspace_handle)| {
            workspace_handle
                .as_ref(app)
                .tab_views()
                .enumerate()
                .find_map(|(tab_idx, pane_group)| {
                    pane_group
                        .as_ref(app)
                        .terminal_pane_ids()
                        .filter_map(|pane_id| {
                            pane_group
                                .as_ref(app)
                                .terminal_view_from_pane_id(pane_id, app)
                        })
                        .find_map(|terminal_view| {
                            let terminal_view_conversation_id =
                                terminal_view.as_ref(app).active_conversation_id(app)?;
                            (terminal_view_conversation_id == conversation_id)
                                .then_some((*window_id, tab_idx))
                        })
                })
        })
}

fn active_focused_terminal_id(app: &AppContext) -> Option<EntityId> {
    let active_window = app.windows().active_window()?;
    let workspace = app
        .views_of_type::<Workspace>(active_window)
        .and_then(|views| views.first().cloned())?;

    let workspace = workspace.as_ref(app);
    workspace.active_terminal_id(app)
}

#[cfg(test)]
#[path = "agent_management_model_tests.rs"]
mod tests;
