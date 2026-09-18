use std::collections::HashMap;
use std::path::PathBuf;

use warp_errors::report_error;
use warpui::{SingletonEntity, ViewContext};

use super::{HiddenChildAgentTaskContext, apply_hidden_child_agent_task_context};
use crate::ai::agent::conversation::{AIConversation, AIConversationId};
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::ai::blocklist::agent_view::AgentViewEntryOrigin;
use crate::ai::restored_conversations::RestoredAgentConversations;
use crate::pane_group::{PaneGroup, PaneId};
use crate::terminal::view::load_ai_conversation::{
    RestoreConversationEntryBehavior, RestoredAIConversation,
};

impl PaneGroup {
    /// Lazily restores hidden child panes for the given parent conversation.
    ///
    /// Unlike the old startup sweep, this runs only when the parent agent view
    /// is actually restored or entered. Children that already belong to some
    /// other pane or tab are left alone.
    pub(in crate::pane_group) fn restore_missing_child_agent_panes_for_parent(
        &mut self,
        parent_conversation_id: AIConversationId,
        parent_pane_id: PaneId,
        ctx: &mut ViewContext<Self>,
    ) {
        let child_ids = BlocklistAIHistoryModel::as_ref(ctx)
            .child_conversation_ids_of(&parent_conversation_id)
            .to_vec();

        for child_id in child_ids {
            if self
                .child_agent_panes
                .get(&child_id)
                .is_some_and(|pane_id| self.has_pane_id(*pane_id))
            {
                continue;
            }

            if self.is_conversation_owned_outside_pane(child_id, parent_pane_id, ctx) {
                continue;
            }

            let child_conversation = BlocklistAIHistoryModel::as_ref(ctx)
                .conversation(&child_id)
                .cloned()
                .or_else(|| {
                    RestoredAgentConversations::handle(ctx)
                        .update(ctx, |store, _| store.take_conversation(&child_id))
                });
            let Some(child_conversation) = child_conversation else {
                log::warn!("Child conversation {child_id:?} not found in memory or restored store");
                continue;
            };

            self.create_hidden_child_agent_pane(child_conversation, parent_pane_id, ctx);
        }
    }

    /// Restores hidden child panes if this terminal pane is already showing a
    /// fullscreen agent view. This covers restored or replaced panes whose
    /// terminal view entered agent view before pane-group attachment finished.
    pub(in crate::pane_group) fn restore_missing_child_agent_panes_for_terminal_pane_if_needed(
        &mut self,
        pane_id: PaneId,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(terminal_pane_id) = pane_id.as_terminal_pane_id() else {
            return;
        };
        let Some(parent_conversation_id) = self
            .terminal_view_from_pane_id(terminal_pane_id, ctx)
            .and_then(|terminal_view| {
                let terminal_view = terminal_view.as_ref(ctx);
                let controller = terminal_view.agent_view_controller().as_ref(ctx);
                if controller.is_fullscreen() {
                    controller.agent_view_state().active_conversation_id()
                } else {
                    None
                }
            })
        else {
            return;
        };

        self.restore_missing_child_agent_panes_for_parent(
            parent_conversation_id,
            terminal_pane_id.into(),
            ctx,
        );
    }

    /// Ensures `child_conversation_id` has a hidden child pane if it still
    /// belongs under a parent conversation in this pane group.
    ///
    /// Returns true if the conversation is already reachable through an
    /// existing pane or if lazy restoration successfully materialized the child
    /// pane.
    pub(in crate::pane_group) fn ensure_hidden_child_agent_pane_for_conversation(
        &mut self,
        child_conversation_id: AIConversationId,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        if self
            .child_agent_panes
            .get(&child_conversation_id)
            .is_some_and(|pane_id| self.has_pane_id(*pane_id))
        {
            return true;
        }

        let parent_conversation_id =
            BlocklistAIHistoryModel::handle(ctx).update(ctx, |history_model, ctx| {
                history_model
                    .conversation(&child_conversation_id)
                    .and_then(|conversation| {
                        history_model.resolved_parent_conversation_id_for_conversation(conversation)
                    })
                    .or_else(|| {
                        RestoredAgentConversations::handle(ctx).update(ctx, |store, _| {
                            store.get_conversation(&child_conversation_id).and_then(
                                |conversation| {
                                    history_model.resolved_parent_conversation_id_for_conversation(
                                        conversation,
                                    )
                                },
                            )
                        })
                    })
            });

        let Some(parent_conversation_id) = parent_conversation_id else {
            return self
                .terminal_view_id_for_owned_conversation(child_conversation_id, ctx)
                .is_some();
        };

        let child_owner_terminal_view_id =
            self.terminal_view_id_for_owned_conversation(child_conversation_id, ctx);
        let Some(parent_pane_id) = self.pane_id_for_owned_conversation(parent_conversation_id, ctx)
        else {
            return child_owner_terminal_view_id.is_some();
        };

        if self.is_conversation_owned_outside_pane(child_conversation_id, parent_pane_id, ctx) {
            return true;
        }

        self.restore_missing_child_agent_panes_for_parent(
            parent_conversation_id,
            parent_pane_id,
            ctx,
        );

        self.child_agent_panes
            .get(&child_conversation_id)
            .is_some_and(|pane_id| self.has_pane_id(*pane_id))
            || self.is_conversation_owned_outside_pane(child_conversation_id, parent_pane_id, ctx)
    }

    /// Creates a hidden child agent pane for an existing child conversation,
    /// restoring the conversation and tracking it in `child_agent_panes`.
    /// Remote children have no restorable content — their transcript lived
    /// behind the deleted cloud conversation fetch — so they are skipped.
    pub(in crate::pane_group) fn create_hidden_child_agent_pane(
        &mut self,
        child_conversation: AIConversation,
        parent_pane_id: PaneId,
        ctx: &mut ViewContext<Self>,
    ) {
        let child_id = child_conversation.id();

        if child_conversation.is_remote_child() {
            log::warn!(
                "Skipping remote child conversation {child_id:?}: no cloud transcript fetch"
            );
            return;
        }

        let child_task_context =
            child_conversation
                .task_id()
                .map(|task_id| HiddenChildAgentTaskContext {
                    task_id,
                    working_dir: child_conversation
                        .current_working_directory()
                        .or_else(|| child_conversation.initial_working_directory())
                        .map(PathBuf::from),
                });
        let new_pane_id =
            self.insert_terminal_pane_hidden_for_child_agent(parent_pane_id, HashMap::new(), ctx);

        match self.terminal_view_from_pane_id(new_pane_id, ctx) {
            Some(new_terminal_view) => {
                if let Some(task_context) = child_task_context.as_ref() {
                    apply_hidden_child_agent_task_context(&new_terminal_view, task_context, ctx);
                }
                new_terminal_view.update(ctx, |terminal_view, ctx| {
                    terminal_view.restore_conversation_after_view_creation(
                        RestoredAIConversation::new(child_conversation),
                        true,
                        RestoreConversationEntryBehavior::PreserveAgentViewState,
                        ctx,
                    );
                    terminal_view.enter_agent_view(
                        None,
                        Some(child_id),
                        AgentViewEntryOrigin::ChildAgent,
                        ctx,
                    );
                });

                self.child_agent_panes.insert(child_id, new_pane_id.into());
            }
            _ => {
                report_error!(
                    "Failed to get terminal view for child agent pane",
                    extra: { "child_id" => ?child_id }
                );
                self.discard_pane(new_pane_id.into(), ctx);
            }
        }
    }
}
