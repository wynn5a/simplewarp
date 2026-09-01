use uuid::Uuid;
use warp_errors::report_error;
use warpui::{SingletonEntity, ViewContext};

use super::materialization::{ChildPaneMaterialization, decide_child_pane_materialization};
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::{AIConversation, AIConversationId};
use crate::ai::agent_conversations_model::AgentConversationsModel;
use crate::ai::ambient_agents::{AmbientAgentTask, AmbientAgentTaskId};
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::ai::blocklist::agent_view::AgentViewEntryOrigin;
use crate::ai::blocklist::history_model::CloudConversationData;
use crate::pane_group::{
    AmbientAgentViewModelHandleExt, PaneGroup, PaneId, TerminalPane, TerminalViewResources,
};
use crate::terminal::model::terminal_model::ConversationTranscriptViewerStatus;
use crate::terminal::view::load_ai_conversation::{
    RestoreConversationEntryBehavior, RestoredAIConversation,
};

/// How to hydrate a restored hidden remote-child pane given its
/// [`AmbientAgentTask`]. Only used while `OrchestrationUnifiedStack` is
/// disabled. See [`decide_remote_child_hydration_action`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::pane_group) enum RemoteChildHydrationAction {
    /// A server conversation token is available to load a transcript for.
    LoadTranscript {
        server_token: ServerConversationToken,
    },
    /// Neither live nor cloud transcript available; fall through to
    /// attaching the (possibly empty) ambient session.
    Fallback,
}

/// Pure decision function backing [`PaneGroup::hydrate_task_backed_hidden_child_pane`].
/// Free-standing so it's unit-testable without a `PaneGroup`.
pub(in crate::pane_group) fn decide_remote_child_hydration_action(
    task: &AmbientAgentTask,
) -> RemoteChildHydrationAction {
    // Empty/whitespace tokens would drive a no-op cloud fetch followed by
    // a misleading tombstone; route them to `Fallback` instead.
    let server_token = task
        .conversation_id()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| ServerConversationToken::new(t.to_string()));

    match server_token {
        Some(server_token) => RemoteChildHydrationAction::LoadTranscript { server_token },
        None => RemoteChildHydrationAction::Fallback,
    }
}

impl PaneGroup {
    /// Materializes the pane for a placeholder child conversation from its
    /// [`AmbientAgentTask`](crate::ai::ambient_agents::AmbientAgentTask),
    /// leaving a loading pane in place while the task is still being fetched.
    ///
    /// Idempotent: repeat calls for a placeholder that already has a live
    /// tracked pane are skipped rather than creating a duplicate pane and
    /// orphaning the first.
    pub(in crate::pane_group) fn materialize_child_pane(
        &mut self,
        child_conversation: AIConversation,
        ctx: &mut ViewContext<Self>,
    ) {
        let child_id = child_conversation.id();

        if let Some(existing_pane_id) = self.child_agent_panes.get(&child_id).copied()
            && self.has_pane_id(existing_pane_id)
        {
            return;
        }

        let Some(task_id) = child_conversation.task_id() else {
            log::warn!("Cannot restore remote child conversation {child_id:?} without a task ID");
            return;
        };
        let task = AgentConversationsModel::handle(ctx).update(ctx, |model, ctx| {
            model.get_or_async_fetch_task_data(&task_id, ctx)
        });

        let Some(task) = task else {
            // Show the child loading presentation rather than the generic
            // cloud-agent composing zero state, and register for a re-drive
            // once the task fetch lands.
            if self
                .create_child_loading_placeholder(
                    child_conversation,
                    AgentViewEntryOrigin::CloudAgent,
                    ctx,
                )
                .is_none()
            {
                return;
            }
            self.pending_child_hydrations.insert(task_id, child_id);
            self.ensure_pending_ambient_restoration_subscription(ctx);
            return;
        };

        self.apply_child_pane_materialization(child_conversation, task, ctx);
    }

    /// Applies the materialization decision for a child whose task snapshot is
    /// available: `LoadTranscript` fetches and merges the cloud transcript, and
    /// `Pending` leaves a loading placeholder in place until fresher task data
    /// arrives.
    fn apply_child_pane_materialization(
        &mut self,
        child_conversation: AIConversation,
        task: AmbientAgentTask,
        ctx: &mut ViewContext<Self>,
    ) {
        match decide_child_pane_materialization(&task) {
            ChildPaneMaterialization::LoadTranscript { server_token } => {
                let child_id = child_conversation.id();
                let task_id = task.task_id;
                let pane_id = self
                    .child_agent_panes
                    .get(&child_id)
                    .copied()
                    .filter(|pane_id| self.has_pane_id(*pane_id))
                    .or_else(|| {
                        self.create_child_loading_placeholder(
                            child_conversation,
                            AgentViewEntryOrigin::CloudAgent,
                            ctx,
                        )
                    });
                let Some(pane_id) = pane_id else {
                    return;
                };
                self.pending_child_hydrations.remove(&task_id);
                self.hydrate_child_transcript(pane_id, child_id, task_id, server_token, ctx);
            }
            ChildPaneMaterialization::Pending => {
                let child_id = child_conversation.id();
                let task_id = task.task_id;
                if !self
                    .child_agent_panes
                    .get(&child_id)
                    .is_some_and(|pane_id| self.has_pane_id(*pane_id))
                {
                    let _ = self.create_child_loading_placeholder(
                        child_conversation,
                        AgentViewEntryOrigin::CloudAgent,
                        ctx,
                    );
                }
                self.pending_child_hydrations.insert(task_id, child_id);
                self.ensure_pending_ambient_restoration_subscription(ctx);
            }
        }
    }

    /// Points the pane's ambient agent view model at the existing session for
    /// `task_id` and at the child's conversation.
    fn apply_existing_ambient_task_to_pane(
        &mut self,
        pane_id: PaneId,
        child_id: AIConversationId,
        task_id: AmbientAgentTaskId,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(terminal_view) = self.terminal_view_from_pane_id(pane_id, ctx) else {
            return;
        };
        terminal_view.update(ctx, |terminal_view, ctx| {
            let Some(ambient_agent_view_model) = terminal_view
                .ambient_agent_view_model()
                .into_optional_handle()
                .cloned()
            else {
                return;
            };
            ambient_agent_view_model.update(ctx, |model, ctx| {
                model.set_conversation_id(Some(child_id));
                model.enter_viewing_existing_session(task_id, ctx);
            });
        });
    }

    /// Loads a completed child conversation's cloud transcript and chooses
    /// continuation or passive presentation based on the caller's access to
    /// the conversation.
    fn hydrate_child_transcript(
        &mut self,
        pane_id: PaneId,
        child_id: AIConversationId,
        task_id: AmbientAgentTaskId,
        server_token: ServerConversationToken,
        ctx: &mut ViewContext<Self>,
    ) {
        let history_handle = BlocklistAIHistoryModel::handle(ctx);
        let future = history_handle.update(ctx, |history_model, ctx| {
            history_model.load_conversation_by_server_token(&server_token, ctx)
        });
        ctx.spawn(future, move |group, conversation, ctx| {
            let still_canonical = group
                .child_agent_panes
                .get(&child_id)
                .copied()
                .is_some_and(|p| p == pane_id && group.has_pane_id(p));
            if !still_canonical {
                return;
            }
            // The pane may have been swapped to another conversation while
            // the fetch was in flight; don't overwrite what it now shows.
            let active_conversation = group
                .terminal_view_from_pane_id(pane_id, ctx)
                .and_then(|view| view.as_ref(ctx).active_conversation_id(ctx));
            if active_conversation != Some(child_id) {
                return;
            }
            let merged = match conversation {
                Some(CloudConversationData::Oz(cloud)) => {
                    let tasks: Vec<warp_multi_agent_api::Task> = cloud
                        .all_tasks()
                        .filter_map(|task| task.source().cloned())
                        .collect();
                    let cloud_conversation = *cloud;
                    match BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, _| {
                        history.hydrate_remote_child_placeholder_with_cloud_transcript(
                            child_id,
                            tasks,
                            cloud_conversation,
                        )
                    }) {
                        Ok(merged) => merged,
                        Err(err) => {
                            log::warn!(
                                "child transcript upgrade merge-error \
                                 child_conversation_id={child_id:?} error={err:#}"
                            );
                            return;
                        }
                    }
                }
                Some(CloudConversationData::CLIAgent(_)) => {
                    log::warn!(
                        "child transcript upgrade unsupported \
                         CLI transcript child_conversation_id={child_id:?}"
                    );
                    return;
                }
                None => {
                    log::warn!(
                        "child transcript upgrade fetch-empty \
                         child_conversation_id={child_id:?}"
                    );
                    // Re-queue without evicting the task so the retry is
                    // driven by the normal TasksUpdated cadence rather than
                    // firing immediately on every round-trip. Evicting would
                    // create an unbounded loop on permanent failures such as
                    // a 403 from an Observer that can see the task row but
                    // not the conversation transcript.
                    group.pending_child_hydrations.insert(task_id, child_id);
                    return;
                }
            };

            // A completed cloud child can only be shown as a passive transcript: continuing
            // it needed a cloud follow-up, which this build cannot start.
            group.restore_child_passive_transcript(pane_id, child_id, task_id, merged, ctx);
        });
    }

    /// Restores a child transcript in place without enabling continuation.
    fn restore_child_passive_transcript(
        &mut self,
        pane_id: PaneId,
        child_id: AIConversationId,
        task_id: AmbientAgentTaskId,
        merged: AIConversation,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(terminal_manager) = self
            .terminal_session_by_id(pane_id)
            .map(|session| session.terminal_manager(ctx))
        {
            terminal_manager.update(ctx, |manager, _ctx| {
                let model_handle = manager.model();
                let mut model = model_handle.lock();
                model.set_conversation_transcript_viewer_status(Some(
                    ConversationTranscriptViewerStatus::ViewingAmbientConversation(task_id),
                ));
            });
        }
        if let Some(terminal_view) = self.terminal_view_from_pane_id(pane_id, ctx) {
            BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, _ctx| {
                history.mark_terminal_surface_as_conversation_transcript_viewer(terminal_view.id());
            });
            terminal_view.update(ctx, |view, ctx| {
                view.set_orchestration_child_live_unavailable(false, ctx);
                view.restore_conversation_after_view_creation(
                    RestoredAIConversation::new(merged),
                    true,
                    RestoreConversationEntryBehavior::PreserveAgentViewState,
                    ctx,
                );
            });
        }
        self.child_agent_panes.insert(child_id, pane_id);
    }

    /// Renders the shared loading placeholder presentation used while a child
    /// has neither an attachable session nor a loadable transcript.
    pub(in crate::pane_group) fn create_child_loading_placeholder(
        &mut self,
        child_conversation: AIConversation,
        origin: AgentViewEntryOrigin,
        ctx: &mut ViewContext<Self>,
    ) -> Option<PaneId> {
        let child_id = child_conversation.id();
        let resources = TerminalViewResources {
            tips_completed: self.tips_completed.clone(),
            server_api: self.server_api.clone(),
            model_event_sender: self.model_event_sender.clone(),
        };
        let view_size = Self::estimated_view_bounds(ctx).size();
        let (loading_view, loading_manager) = Self::create_loading_terminal_manager_and_view(
            resources,
            view_size,
            ctx.window_id(),
            ctx,
        );
        let pane_data = TerminalPane::new(
            Uuid::new_v4().as_bytes().to_vec(),
            loading_manager,
            loading_view.clone(),
            self.model_event_sender.clone(),
            ctx,
        );
        let new_pane_id = pane_data.terminal_pane_id();
        if self
            .attach_child_pane_off_tree(Box::new(pane_data), ctx)
            .is_none()
        {
            report_error!(
                "create_child_loading_placeholder: failed to attach child loading pane",
                extra: { "child_id" => ?child_id }
            );
            return None;
        }

        // Entering agent view is what makes the pill bar render; the loading
        // view keeps the output area a spinner until hydration completes.
        loading_view.update(ctx, |terminal_view, ctx| {
            terminal_view.restore_conversation_after_view_creation(
                RestoredAIConversation::new(child_conversation),
                true,
                RestoreConversationEntryBehavior::PreserveAgentViewState,
                ctx,
            );
            terminal_view.enter_agent_view(None, Some(child_id), origin, ctx);
        });

        self.child_agent_panes.insert(child_id, new_pane_id.into());
        Some(new_pane_id.into())
    }

    /// Re-drives child panes after task metadata changes: terminal tasks
    /// upgrade the existing pane to a passive transcript.
    pub(in crate::pane_group) fn process_pending_child_hydrations(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        if !crate::features::FeatureFlag::OrchestrationUnifiedStack.is_enabled()
            || self.pending_child_hydrations.is_empty()
        {
            return;
        }

        let ready_tasks: Vec<_> = self
            .pending_child_hydrations
            .keys()
            .filter(|task_id| {
                AgentConversationsModel::as_ref(ctx)
                    .get_task_data(task_id)
                    .is_some()
            })
            .copied()
            .collect();

        for task_id in ready_tasks {
            let Some(child_id) = self.pending_child_hydrations.remove(&task_id) else {
                continue;
            };
            let Some(task) = AgentConversationsModel::as_ref(ctx).get_task_data(&task_id) else {
                continue;
            };
            let Some(pane_id) = self
                .child_agent_panes
                .get(&child_id)
                .copied()
                .filter(|pane_id| self.has_pane_id(*pane_id))
            else {
                continue;
            };

            match decide_child_pane_materialization(&task) {
                ChildPaneMaterialization::LoadTranscript { server_token } => {
                    self.hydrate_child_transcript(pane_id, child_id, task_id, server_token, ctx);
                }
                ChildPaneMaterialization::Pending => {
                    self.pending_child_hydrations.insert(task_id, child_id);
                }
            }
        }
    }

    // =========================================================================
    // flag-OFF path (OrchestrationUnifiedStack disabled)
    // =========================================================================

    /// Task-backed restore path for remote children while
    /// `OrchestrationUnifiedStack` is disabled. Creates the hidden ambient
    /// pane, registers it in `child_agent_panes` under the placeholder's local
    /// `AIConversationId`, then hydrates it (or queues a pending entry while
    /// task data is fetched).
    ///
    /// Idempotent: repeat calls for a placeholder that already has a live
    /// tracked pane — including while the initial async hydration is still in
    /// flight — are skipped rather than orphaning the first pane.
    pub(super) fn hydrate_task_backed_hidden_child_pane(
        &mut self,
        child_conversation: AIConversation,
        parent_pane_id: PaneId,
        task_id: AmbientAgentTaskId,
        ctx: &mut ViewContext<Self>,
    ) {
        let child_id = child_conversation.id();

        if let Some(existing_pane_id) = self.child_agent_panes.get(&child_id).copied()
            && self.has_pane_id(existing_pane_id)
        {
            return;
        }

        let new_pane_id =
            self.insert_ambient_agent_pane_hidden_for_child_agent(parent_pane_id, ctx);

        let Some(new_terminal_view) = self.terminal_view_from_pane_id(new_pane_id, ctx) else {
            report_error!(
                "Failed to get terminal view for remote child agent pane",
                extra: { "child_id" => ?child_id }
            );
            self.discard_pane(new_pane_id.into(), ctx);
            return;
        };

        // Restore the placeholder so the pane has parent linkage + agent
        // name before task-backed hydration runs.
        let mut restored = false;
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
                AgentViewEntryOrigin::CloudAgent,
                ctx,
            );
            restored = terminal_view
                .ambient_agent_view_model()
                .into_optional_handle()
                .is_some();
        });

        if !restored {
            report_error!(
                "Failed to restore remote child agent pane: missing ambient agent view model",
                extra: { "child_id" => ?child_id }
            );
            self.discard_pane(new_pane_id.into(), ctx);
            return;
        }

        // Placeholder's local id stays the canonical `child_agent_panes`
        // key across live-attach and transcript hydration.
        self.child_agent_panes.insert(child_id, new_pane_id.into());

        let task_now = AgentConversationsModel::handle(ctx).update(ctx, |model, ctx| {
            model.get_or_async_fetch_task_data(&task_id, ctx)
        });

        if task_now.is_none() {
            // Task data not yet cached: queue a pending hydration and
            // attempt a live-attach in the meantime so streaming runs are
            // not stalled while waiting on the fetch.
            self.pending_remote_child_hydrations
                .insert(task_id, child_id);
            self.ensure_pending_ambient_restoration_subscription(ctx);
            self.apply_existing_ambient_task_to_pane(new_pane_id.into(), child_id, task_id, ctx);
            return;
        }

        self.attempt_remote_child_hydration(child_id, task_id, ctx);
    }

    /// Dispatches the hydration action chosen by
    /// [`decide_remote_child_hydration_action`] for a restored hidden child
    /// pane when `OrchestrationUnifiedStack` is disabled.
    fn attempt_remote_child_hydration(
        &mut self,
        child_id: AIConversationId,
        task_id: AmbientAgentTaskId,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(pane_id) = self
            .child_agent_panes
            .get(&child_id)
            .copied()
            .filter(|pane_id| self.has_pane_id(*pane_id))
        else {
            return;
        };

        let Some(task) = AgentConversationsModel::as_ref(ctx).get_task_data(&task_id) else {
            // Defensive: callers only reach here after `get_task_data`
            // returned `Some`. If it's gone now, leave the pending entry
            // alone so the next `TasksUpdated` can re-drive.
            return;
        };

        match decide_remote_child_hydration_action(&task) {
            RemoteChildHydrationAction::LoadTranscript { server_token } => {
                self.hydrate_remote_child_transcript_in_place(
                    pane_id,
                    child_id,
                    task_id,
                    server_token,
                    ctx,
                );
            }
            RemoteChildHydrationAction::Fallback => {
                // No live session, no server token: attach to the
                // (possibly empty) ambient session.
                self.apply_existing_ambient_task_to_pane(pane_id, child_id, task_id, ctx);
            }
        }
    }

    /// Fetches the cloud transcript for a restored hidden child pane when
    /// `OrchestrationUnifiedStack` is disabled.
    fn hydrate_remote_child_transcript_in_place(
        &mut self,
        pane_id: PaneId,
        child_id: AIConversationId,
        task_id: AmbientAgentTaskId,
        server_token: ServerConversationToken,
        ctx: &mut ViewContext<Self>,
    ) {
        let history_handle = BlocklistAIHistoryModel::handle(ctx);
        let future = history_handle.update(ctx, |history_model, ctx| {
            history_model.load_conversation_by_server_token(&server_token, ctx)
        });
        ctx.spawn(future, move |group, conversation, ctx| {
            // Guard against a stale target while the fetch was in flight:
            // the pane id must still be the canonical one for `child_id`
            // AND the pane's terminal view must still be displaying it.
            let still_canonical = group
                .child_agent_panes
                .get(&child_id)
                .copied()
                .is_some_and(|p| p == pane_id && group.has_pane_id(p));
            if !still_canonical {
                return;
            }
            let terminal_view_active_conversation = group
                .terminal_view_from_pane_id(pane_id, ctx)
                .and_then(|tv| tv.as_ref(ctx).active_conversation_id(ctx));
            if terminal_view_active_conversation != Some(child_id) {
                return;
            }

            match conversation {
                Some(CloudConversationData::Oz(cloud)) => {
                    let tasks: Vec<warp_multi_agent_api::Task> = cloud
                        .all_tasks()
                        .filter_map(|task| task.source().cloned())
                        .collect();
                    let cloud_conversation = *cloud;
                    let merge_result =
                        BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, _| {
                            history.hydrate_remote_child_placeholder_with_cloud_transcript(
                                child_id,
                                tasks,
                                cloud_conversation,
                            )
                        });
                    match merge_result {
                        Ok(merged) => {
                            if let Some(terminal_view) =
                                group.terminal_view_from_pane_id(pane_id, ctx)
                            {
                                terminal_view.update(ctx, |view, ctx| {
                                    view.restore_conversation_after_view_creation(
                                        RestoredAIConversation::new(merged),
                                        true,
                                        RestoreConversationEntryBehavior::PreserveAgentViewState,
                                        ctx,
                                    );
                                });
                            }
                        }
                        Err(err) => {
                            log::warn!(
                                "hydrate_remote_child_placeholder_with_cloud_transcript failed for {child_id:?}: {err:#}"
                            );
                        }
                    }
                }
                Some(CloudConversationData::CLIAgent(_)) | None => {
                    // Non-Oz transcript or fetch failure — the post-match
                    // call handles attach + conditional tombstone.
                }
            }

            // Uniform post-match step across all three branches above.
            group.apply_existing_ambient_task_to_pane(pane_id, child_id, task_id, ctx);
        });
    }

    /// Drains entries from `pending_remote_child_hydrations` for which task
    /// data is now available, hydrating each hidden child pane in place. That
    /// map is only populated while `OrchestrationUnifiedStack` is disabled.
    pub(in crate::pane_group) fn process_pending_remote_child_hydrations(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        if self.pending_remote_child_hydrations.is_empty() {
            return;
        }

        let ready_tasks: Vec<_> = self
            .pending_remote_child_hydrations
            .keys()
            .filter(|task_id| {
                AgentConversationsModel::as_ref(ctx)
                    .get_task_data(task_id)
                    .is_some()
            })
            .copied()
            .collect();

        for task_id in ready_tasks {
            let Some(child_id) = self.pending_remote_child_hydrations.remove(&task_id) else {
                continue;
            };

            self.attempt_remote_child_hydration(child_id, task_id, ctx);
        }
    }
}
