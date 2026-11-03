//! Compacting a conversation before its context window fills up, and after it overflowed.
//!
//! `local_inference` picks the cut and writes the summary (see its `compaction` module); the
//! controller decides when to ask for one and what to send once it is written. Two triggers start
//! a compaction on their own: a request about to be sent while the context is past the
//! threshold, which is sent once the summary is done, and a request the provider rejected for
//! exceeding the window, which is followed by one resume.

use std::path::PathBuf;
use std::sync::Arc;

use agent_harness::compaction::should_compact;
use warpui::{AppContext, ModelContext, SingletonEntity};

use super::input_context::input_context_for_request;
use super::{BlocklistAIController, BlocklistAIControllerEvent, RequestInput};
use crate::ai::agent::conversation::{AIConversationId, ConversationStatus};
use crate::ai::agent::{
    AIAgentAttachment, AIAgentContext, AIAgentInput, EntrypointType, RequestMetadata, api,
};
use crate::ai::blocklist::context_model::{PendingAttachment, PendingFile};
use crate::ai::blocklist::queued_query::{QueuedQuery, QueuedQueryModel, QueuedQueryOrigin};
use crate::ai::blocklist::{BlocklistAIHistoryModel, ResponseStreamId};
use crate::ai::llms::LLMPreferences;
use crate::settings::AISettings;

/// What a summary request carries besides the user's instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CompactionBudget {
    keep_recent_tokens: u32,
    context_tokens: u64,
    max_output_tokens: u32,
}

impl CompactionBudget {
    pub(super) fn summarize_input(
        self,
        prompt: Option<String>,
        context: Arc<[AIAgentContext]>,
    ) -> AIAgentInput {
        AIAgentInput::SummarizeConversation {
            prompt,
            keep_recent_tokens: self.keep_recent_tokens,
            context_tokens: self.context_tokens,
            max_output_tokens: self.max_output_tokens,
            context,
        }
    }
}

/// What the controller does when a summary stream ends.
pub(super) enum Continuation {
    /// Nothing more: a summary the user asked for.
    Done,
    /// Sends the request that the compaction went ahead of.
    Send(Box<DeferredRequest>),
    /// Resumes the conversation whose request overflowed the context window.
    Resume,
}

/// A request held back until the compaction sent ahead of it finishes.
pub(super) struct DeferredRequest {
    request_input: RequestInput,
    query_metadata: Option<RequestMetadata>,
    can_attempt_resume_on_error: bool,
}

/// How a response stream ended, as far as what follows it is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StreamOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

impl BlocklistAIController {
    /// How much a summary of `conversation_id` keeps verbatim, how full its context is, and how
    /// long the summary may be.
    ///
    /// When the reserve is as large as the window nothing is kept, and the summary replaces the
    /// whole conversation, as `/compact` did before compaction kept a tail.
    pub(super) fn compaction_budget(
        &self,
        conversation_id: Option<AIConversationId>,
        app: &AppContext,
    ) -> CompactionBudget {
        let settings = AISettings::as_ref(app).compaction_settings();
        let window =
            LLMPreferences::as_ref(app).active_context_window(app, Some(self.terminal_view_id));
        let keep_recent_tokens = window.map_or(settings.keep_recent_tokens, |window| {
            settings.keep_recent_for(window)
        });
        let context_tokens = conversation_id
            .and_then(|id| BlocklistAIHistoryModel::as_ref(app).conversation(&id))
            .map(|conversation| conversation.context_usage())
            .filter(|usage| usage.has_report())
            .map_or(0, |usage| usage.used_tokens());
        CompactionBudget {
            keep_recent_tokens,
            context_tokens,
            max_output_tokens: settings.summary_max_output_tokens(window),
        }
    }

    /// Whether `request_input` should wait for the conversation to be compacted first.
    fn should_compact_before(&self, request_input: &RequestInput, app: &AppContext) -> bool {
        let settings = AISettings::as_ref(app).compaction_settings();
        if !settings.enabled
            || request_input
                .all_inputs()
                .any(|input| matches!(input, AIAgentInput::SummarizeConversation { .. }))
        {
            return false;
        }
        let Some(conversation) =
            BlocklistAIHistoryModel::as_ref(app).conversation(&request_input.conversation_id)
        else {
            return false;
        };
        // A summary covers the root task's conversation; a request to a subagent task is left
        // alone.
        let root_task_id = conversation.get_root_task_id();
        if conversation.is_summarizing()
            || request_input
                .input_messages
                .keys()
                .any(|task_id| task_id != root_task_id)
        {
            return false;
        }
        let mut usage = conversation.context_usage();
        if !usage.has_report() {
            return false;
        }
        let Some(window) =
            LLMPreferences::as_ref(app).active_context_window(app, Some(self.terminal_view_id))
        else {
            return false;
        };
        usage.set_estimate(api::estimate_input_tokens(
            request_input.all_inputs().cloned().collect(),
        ));
        should_compact(
            usage.used_tokens(),
            window,
            &settings,
            conversation.last_compacted_to(),
        )
    }

    /// Sends `request_input`, compacting the conversation first when its context is past the
    /// threshold. See [`Self::send_request_input_now`].
    pub(super) fn send_request_input(
        &mut self,
        request_input: RequestInput,
        query_metadata: Option<RequestMetadata>,
        can_attempt_resume_on_error: bool,
        is_queued_prompt: bool,
        ctx: &mut ModelContext<Self>,
    ) -> anyhow::Result<(AIConversationId, ResponseStreamId)> {
        if !self.should_compact_before(&request_input, ctx) {
            return self.send_request_input_now(
                request_input,
                query_metadata,
                can_attempt_resume_on_error,
                is_queued_prompt,
                ctx,
            );
        }

        let conversation_id = request_input.conversation_id;
        let contains_user_query = request_input
            .all_inputs()
            .any(|input| input.is_user_query());
        match self.send_compaction(conversation_id, ctx) {
            Ok(ids) => {
                log::info!("Compacting the conversation before its next request");
                self.compactions.insert(
                    ids.1.clone(),
                    Continuation::Send(Box::new(DeferredRequest {
                        request_input,
                        query_metadata,
                        can_attempt_resume_on_error,
                    })),
                );
                // The user's submission is done with: clear the input and its attachments now
                // rather than when the held-back request is sent, by which time the user may
                // have typed something new.
                if contains_user_query && !is_queued_prompt {
                    self.context_model.update(ctx, |context_model, ctx| {
                        context_model.reset_context_to_default(ctx);
                    });
                    ctx.emit(BlocklistAIControllerEvent::SentRequest {
                        contains_user_query,
                        is_queued_prompt,
                    });
                }
                Ok(ids)
            }
            Err(error) => {
                log::warn!("Could not start a compaction, sending the request as is: {error:#}");
                self.send_request_input_now(
                    request_input,
                    query_metadata,
                    can_attempt_resume_on_error,
                    is_queued_prompt,
                    ctx,
                )
            }
        }
    }

    /// Sends a summary request for `conversation_id`'s root task.
    fn send_compaction(
        &mut self,
        conversation_id: AIConversationId,
        ctx: &mut ModelContext<Self>,
    ) -> anyhow::Result<(AIConversationId, ResponseStreamId)> {
        let task_id = BlocklistAIHistoryModel::as_ref(ctx)
            .conversation(&conversation_id)
            .ok_or_else(|| anyhow::anyhow!("no conversation to compact"))?
            .get_root_task_id()
            .clone();
        let context = input_context_for_request(
            false,
            self.context_model.as_ref(ctx),
            self.active_session.as_ref(ctx),
            Some(conversation_id),
            vec![],
            ctx,
        );
        let input = self
            .compaction_budget(Some(conversation_id), ctx)
            .summarize_input(None, context);
        let request_input = RequestInput::for_task(
            vec![input],
            task_id,
            &self.active_session,
            conversation_id,
            self.terminal_view_id,
            ctx,
        );
        self.send_request_input_now(
            request_input,
            None,
            /*can_attempt_resume_on_error*/ false,
            /*is_queued_prompt*/ false,
            ctx,
        )
    }

    /// Whether a stream that overflowed the context window is compacted and resumed: once per
    /// overflow, never for a summary or a subagent task's request, and only with auto-compaction
    /// on.
    pub(super) fn will_compact_after_overflow(
        &self,
        stream_id: &ResponseStreamId,
        conversation_id: AIConversationId,
        app: &AppContext,
    ) -> bool {
        AISettings::as_ref(app).compaction_settings().enabled
            && !self.compactions.contains_key(stream_id)
            && !self.overflow_retries.contains(stream_id)
            && BlocklistAIHistoryModel::as_ref(app)
                .conversation(&conversation_id)
                .is_some_and(|conversation| conversation.is_root_task_response(stream_id))
    }

    /// Whether `stream_id` is a summary request.
    pub(super) fn is_compaction_stream(&self, stream_id: &ResponseStreamId) -> bool {
        self.compactions.contains_key(stream_id)
    }

    /// Drops what the controller tracks for a stream whose end it could not process.
    pub(super) fn forget_stream(&mut self, stream_id: &ResponseStreamId) {
        self.overflow_retries.remove(stream_id);
        self.compact_after_overflow.remove(stream_id);
        if let Some(Continuation::Send(_)) = self.compactions.remove(stream_id) {
            log::warn!("Dropped a request held for a compaction whose stream could not be found");
        }
    }

    /// Sends what follows a stream that just ended: the compaction an overflow owes, or what a
    /// finished compaction went ahead of.
    pub(super) fn continue_after_stream(
        &mut self,
        stream_id: &ResponseStreamId,
        conversation_id: AIConversationId,
        outcome: StreamOutcome,
        ctx: &mut ModelContext<Self>,
    ) {
        self.overflow_retries.remove(stream_id);
        if self.compact_after_overflow.remove(stream_id) {
            if outcome != StreamOutcome::Cancelled {
                log::info!("Compacting the conversation after it overflowed the context window");
                match self.send_compaction(conversation_id, ctx) {
                    Ok((_, summary_stream_id)) => {
                        self.compactions
                            .insert(summary_stream_id, Continuation::Resume);
                    }
                    Err(error) => {
                        log::warn!("Could not start a compaction: {error:#}");
                        self.fail_conversation(conversation_id, ctx);
                    }
                }
            }
            return;
        }

        let Some(continuation) = self.compactions.remove(stream_id) else {
            return;
        };
        let step = next_step(continuation, outcome);
        if outcome == StreamOutcome::Failed {
            // Compacting again right away would most likely fail the same way.
            BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, _| {
                if let Some(conversation) = history.conversation_mut(&conversation_id) {
                    conversation.note_compaction();
                }
            });
        }
        match step {
            Step::Nothing => {}
            Step::Send(deferred) => {
                let DeferredRequest {
                    request_input,
                    query_metadata,
                    can_attempt_resume_on_error,
                } = *deferred;
                if let Err(error) = self.send_request_input_now(
                    request_input,
                    query_metadata,
                    can_attempt_resume_on_error,
                    /*is_queued_prompt*/ true,
                    ctx,
                ) {
                    log::warn!("Could not send the request held for a compaction: {error:#}");
                }
            }
            Step::Requeue(deferred) => self.requeue_held_query(&deferred.request_input, ctx),
            Step::Resume => {
                let resume = self.resume_request_input(conversation_id, vec![], ctx);
                let sent = resume.and_then(|request_input| {
                    self.send_request_input_now(
                        request_input,
                        Some(RequestMetadata {
                            is_autodetected_user_query: false,
                            entrypoint: EntrypointType::ResumeConversation,
                            is_auto_resume_after_error: true,
                        }),
                        /*can_attempt_resume_on_error*/ false,
                        /*is_queued_prompt*/ false,
                        ctx,
                    )
                });
                match sent {
                    Ok((_, resume_stream_id)) => {
                        self.overflow_retries.insert(resume_stream_id);
                    }
                    Err(error) => {
                        log::warn!("Could not resume after a compaction: {error:#}");
                        self.fail_conversation(conversation_id, ctx);
                    }
                }
            }
        }
    }

    /// Puts the question a cancelled compaction held back at the head of the conversation's
    /// queue, where the user can see, edit, delete or send it. A user Stop restores it to the
    /// input rather than sending it, as for any queued prompt.
    fn requeue_held_query(&mut self, request_input: &RequestInput, ctx: &mut ModelContext<Self>) {
        let Some((query, attachments)) = held_query(request_input) else {
            return;
        };
        let conversation_id = request_input.conversation_id;
        QueuedQueryModel::handle(ctx).update(ctx, |model, ctx| {
            model.push_front(
                conversation_id,
                QueuedQuery::new_with_attachments(
                    query,
                    QueuedQueryOrigin::HeldForCompaction,
                    attachments,
                ),
                ctx,
            );
        });
    }

    /// Ends the conversation in an error, for a recovery that could not go on.
    fn fail_conversation(
        &mut self,
        conversation_id: AIConversationId,
        ctx: &mut ModelContext<Self>,
    ) {
        let terminal_view_id = self.terminal_view_id;
        BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
            history.update_conversation_status(
                terminal_view_id,
                conversation_id,
                ConversationStatus::Error,
                ctx,
            );
        });
    }
}

/// What to do once a summary stream ends.
pub(super) enum Step {
    Nothing,
    /// Sends the held request.
    Send(Box<DeferredRequest>),
    /// Queues the held request's question for the user, without sending it.
    Requeue(Box<DeferredRequest>),
    Resume,
}

/// A held request is sent even when its compaction failed, since the threshold is only an estimate
/// and the provider's own overflow handling decides; a cancelled compaction's question goes back
/// to the user. An overflow is resumed only after a compaction that worked.
pub(super) fn next_step(continuation: Continuation, outcome: StreamOutcome) -> Step {
    match (continuation, outcome) {
        (Continuation::Done, _)
        | (Continuation::Resume, StreamOutcome::Failed | StreamOutcome::Cancelled) => Step::Nothing,
        (Continuation::Send(deferred), StreamOutcome::Succeeded | StreamOutcome::Failed) => {
            Step::Send(deferred)
        }
        (Continuation::Send(deferred), StreamOutcome::Cancelled) => Step::Requeue(deferred),
        (Continuation::Resume, StreamOutcome::Succeeded) => Step::Resume,
    }
}

/// The question a held request asked, with the images and files attached to it.
fn held_query(request_input: &RequestInput) -> Option<(String, Vec<PendingAttachment>)> {
    request_input.all_inputs().find_map(|input| match input {
        AIAgentInput::UserQuery {
            query,
            context,
            referenced_attachments,
            ..
        } => {
            let images = context.iter().filter_map(|context| match context {
                AIAgentContext::Image(image) => Some(PendingAttachment::Image(image.clone())),
                _ => None,
            });
            let files = referenced_attachments
                .values()
                .filter_map(|attachment| match attachment {
                    AIAgentAttachment::FilePathReference {
                        file_name,
                        file_path,
                        ..
                    } => Some(PendingAttachment::File(PendingFile {
                        file_name: file_name.clone(),
                        file_path: PathBuf::from(file_path),
                        mime_type: mime_guess::from_path(file_path)
                            .first_or_octet_stream()
                            .to_string(),
                    })),
                    _ => None,
                });
            Some((query.clone(), images.chain(files).collect()))
        }
        _ => None,
    })
}

#[cfg(test)]
impl BlocklistAIController {
    pub(super) fn owes_compaction_for_test(&self, stream_id: &ResponseStreamId) -> bool {
        self.compact_after_overflow.contains(stream_id)
    }

    pub(super) fn mark_compaction_for_test(&mut self, stream_id: ResponseStreamId) {
        self.compactions.insert(stream_id, Continuation::Done);
    }

    pub(super) fn mark_overflow_retry_for_test(&mut self, stream_id: ResponseStreamId) {
        self.overflow_retries.insert(stream_id);
    }

    pub(super) fn hold_for_compaction_for_test(
        &mut self,
        stream_id: ResponseStreamId,
        request_input: RequestInput,
    ) {
        self.compactions.insert(
            stream_id,
            Continuation::Send(Box::new(DeferredRequest::for_test(request_input))),
        );
    }

    pub(super) fn mark_resume_after_compaction_for_test(&mut self, stream_id: ResponseStreamId) {
        self.compactions.insert(stream_id, Continuation::Resume);
    }

    pub(super) fn is_tracking_compaction_for_test(&self, stream_id: &ResponseStreamId) -> bool {
        self.compactions.contains_key(stream_id)
    }
}

#[cfg(test)]
impl DeferredRequest {
    pub(super) fn for_test(request_input: RequestInput) -> Self {
        Self {
            request_input,
            query_metadata: None,
            can_attempt_resume_on_error: false,
        }
    }
}
