use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use agent_harness::retry::{self, Decision, MAX_RETRIES};
use futures::channel::oneshot;
use uuid::Uuid;
use warp_errors::report_error;
use warpui::r#async::Timer;
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::ai::agent::CancellationReason;
use crate::ai::agent::api::{self, ConvertToAPITypeError, generate_multi_agent_output};
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::network::NetworkStatus;
use crate::server::server_api::AIApiError;
use crate::settings::AISettings;

/// What to do about a failed or truncated response attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryAction {
    /// Re-send the same request after the wait.
    RetryAfter(Duration),
    /// Re-send the same request once connectivity returns.
    RetryWhenOnline,
    /// Resume the conversation with a fresh request after the stream completes.
    Resume,
    /// Surface the error; the conversation ends in error.
    Fail,
}

/// What a failed attempt says about recovering from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AttemptFailure {
    /// No part of the reply had reached the client.
    before_reply: bool,
    /// The retry policy would send the same request again.
    retryable: bool,
    retry_after: Option<Duration>,
    /// A fresh request (a resume) may succeed.
    is_recoverable: bool,
}

impl AttemptFailure {
    /// A provider failure reports what it knows; for any other error, an attempt that had received
    /// no client actions failed before the reply.
    fn of(error: &AIApiError, has_received_client_actions: bool) -> Self {
        match error {
            AIApiError::Provider { error, failure } => Self {
                before_reply: failure.before_reply,
                retryable: failure.retryable,
                retry_after: failure.retry_after,
                is_recoverable: error.is_recoverable(),
            },
            other => Self {
                before_reply: !has_received_client_actions,
                retryable: other.is_recoverable(),
                retry_after: None,
                is_recoverable: other.is_recoverable(),
            },
        }
    }
}

/// Decides how to recover from a failed response-stream attempt.
///
/// Before any of the reply has arrived, the request can be re-sent verbatim, as the retry policy
/// allows: `may_retry` is whether it allowed a retry when this attempt was sent. After part of the
/// reply has arrived, re-sending would duplicate it, so recovery uses a fresh
/// `ResumeConversation` request.
fn recovery_action(
    failure: AttemptFailure,
    may_retry: bool,
    retries_done: u32,
    can_attempt_resume_on_error: bool,
    is_online: bool,
) -> RecoveryAction {
    if failure.before_reply {
        match retry::decide(
            may_retry,
            retries_done,
            failure.retryable,
            failure.retry_after,
        ) {
            Decision::Retry { delay, .. } if is_online => RecoveryAction::RetryAfter(delay),
            Decision::Retry { .. } => RecoveryAction::RetryWhenOnline,
            Decision::GiveUp => RecoveryAction::Fail,
        }
    } else if failure.is_recoverable && can_attempt_resume_on_error {
        RecoveryAction::Resume
    } else {
        RecoveryAction::Fail
    }
}

/// An automatic retry the stream is about to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryNotice {
    /// Which retry this is, from 1.
    pub retry: u32,
    pub max_retries: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResponseStreamId(String);

impl ResponseStreamId {
    #[cfg(test)]
    pub fn new_for_test() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

/// Model wrapping an agent API response stream.
///
/// Emits events when the output corresponding to the stream is updated, typically after receiving
/// each response chunk.
///
/// Handles retries internally - retries are only attempted if no ClientActions events have been
/// received yet, ensuring we don't retry after the AI has started executing actions.
pub struct ResponseStream {
    id: ResponseStreamId,
    params: api::RequestParams,
    retry_count: u32,
    /// Whether the retry policy allowed a retry when the current attempt was sent.
    attempt_may_retry: bool,
    /// Whether a `Retrying` notice is showing while a retry waits.
    is_retry_notice_shown: bool,
    cancellation_tx: Option<oneshot::Sender<()>>,
    /// Track whether we've received any client actions
    /// If true, we cannot retry on subsequent errors since actions may have been executed
    has_received_client_actions: bool,

    /// Whether this request can attempt to resume the conversation on error.
    /// This is true for all requests except those that are themselves the result of a resume
    /// triggered by a previous error.
    can_attempt_resume_on_error: bool,

    /// Whether we should attempt to resume the conversation after the stream finishes.
    ///
    /// This is set when a transient network/server failure occurs after client actions
    /// have been received (so an in-request retry is unsafe) and
    /// `can_attempt_resume_on_error` is true.
    should_resume_conversation_after_stream_finished: bool,

    /// Whether a `StreamFinished` event was received for the current request. A
    /// stream that completes without one was truncated in transit.
    stream_finished_received: bool,

    /// Whether a terminal error event has already been emitted for the current
    /// request, so stream completion doesn't synthesize a second failure for it.
    error_event_emitted: bool,

    /// Whether a retry is parked waiting for connectivity. While set, completion of
    /// the failed attempt's underlying stream is ignored.
    deferred_retry_pending: bool,

    /// Unique, internal id for the current request.
    ///
    /// This ensures that the model never emits events for a request that was already cancelled (or
    /// retried) and is still receiving lagging events.
    ///
    /// Note this is unique compared to `id`; this is unique across retry requests while the response
    /// stream id remains stable.
    current_request_id: Option<Uuid>,
}

impl ResponseStream {
    /// Emits a synthetic successful response event through the normal controller subscription.
    #[cfg(test)]
    pub fn emit_response_event_for_test(
        &mut self,
        event: warp_multi_agent_api::ResponseEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        ctx.emit(ResponseStreamEvent::ReceivedEvent(Consumable::new(Ok(
            event,
        ))));
    }
    #[cfg(test)]
    pub fn new_for_test(id: ResponseStreamId) -> Self {
        let (cancellation_tx, _rx) = oneshot::channel();
        Self {
            id,
            params: api::RequestParams::new_for_test(),
            retry_count: 0,
            attempt_may_retry: false,
            is_retry_notice_shown: false,
            cancellation_tx: Some(cancellation_tx),
            has_received_client_actions: false,
            can_attempt_resume_on_error: false,
            should_resume_conversation_after_stream_finished: false,
            stream_finished_received: false,
            error_event_emitted: false,
            deferred_retry_pending: false,
            current_request_id: Some(Uuid::new_v4()),
        }
    }

    pub fn new(
        mut params: api::RequestParams,
        can_attempt_resume_on_error: bool,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let (cancellation_tx, cancellation_rx) = oneshot::channel();

        let attempt_may_retry = retry::will_retry(*AISettings::as_ref(ctx).auto_retry_enabled, 0);
        params.will_retry_failures_before_reply = attempt_may_retry;
        let request_id = Uuid::new_v4();
        Self::spawn_request(request_id, params.clone(), cancellation_rx, ctx);
        Self {
            id: ResponseStreamId(Uuid::new_v4().to_string()),
            params,
            cancellation_tx: Some(cancellation_tx),
            retry_count: 0,
            attempt_may_retry,
            is_retry_notice_shown: false,
            has_received_client_actions: false,
            can_attempt_resume_on_error,
            should_resume_conversation_after_stream_finished: false,
            stream_finished_received: false,
            error_event_emitted: false,
            deferred_retry_pending: false,
            current_request_id: Some(request_id),
        }
    }

    pub fn id(&self) -> &ResponseStreamId {
        &self.id
    }

    /// Returns true if we should attempt to resume the conversation after the stream finishes.
    pub fn should_resume_conversation_after_stream_finished(&self) -> bool {
        self.should_resume_conversation_after_stream_finished
    }

    fn retry(&mut self, ctx: &mut ModelContext<Self>) {
        self.retry_count += 1;
        self.attempt_may_retry = retry::will_retry(
            *AISettings::as_ref(ctx).auto_retry_enabled,
            self.retry_count,
        );
        self.params.will_retry_failures_before_reply = self.attempt_may_retry;
        self.refresh_conversation_state(ctx);
        if self.is_retry_notice_shown {
            self.is_retry_notice_shown = false;
            ctx.emit(ResponseStreamEvent::Retrying(None));
        }
        // Reset per-attempt state for the new attempt.
        self.has_received_client_actions = false;
        self.stream_finished_received = false;
        self.error_event_emitted = false;
        self.deferred_retry_pending = false;

        let (cancellation_tx, cancellation_rx) = oneshot::channel();
        if let Some(old_cancellation_tx) = self.cancellation_tx.take() {
            let _ = old_cancellation_tx.send(());
        }
        self.cancellation_tx = Some(cancellation_tx);

        let request_id = Uuid::new_v4();
        self.current_request_id = Some(request_id);
        Self::spawn_request(request_id, self.params.clone(), cancellation_rx, ctx);
    }

    /// Takes the conversation's tasks and server token as they are now into the request.
    ///
    /// A failed attempt before its reply is rolled back, which leaves the conversation as the
    /// request found it, except that the first request of a new conversation has created its root
    /// task (outside the transaction) and been given a token. Sending the original snapshot again
    /// would create the root a second time.
    fn refresh_conversation_state(&mut self, ctx: &ModelContext<Self>) {
        let history = BlocklistAIHistoryModel::as_ref(ctx);
        let Some(conversation) = history
            .conversation_for_response_stream(&self.id)
            .and_then(|conversation_id| history.conversation(&conversation_id))
        else {
            return;
        };
        self.params.tasks = conversation.compute_active_tasks();
        if let Some(token) = conversation.server_conversation_token() {
            self.params.conversation_token = Some(token.clone());
        }
    }

    /// Sends the request for `request_id`.
    fn spawn_request(
        request_id: Uuid,
        params: api::RequestParams,
        cancellation_rx: oneshot::Receiver<()>,
        ctx: &mut ModelContext<Self>,
    ) {
        let _ = ctx.spawn(
            async move { generate_multi_agent_output(params, cancellation_rx).await },
            move |me, stream, ctx| {
                me.handle_response_stream_result(request_id, stream, ctx);
            },
        );
    }

    /// Cancels the stream. The conversation_id is preserved in the emitted event for async handling.
    pub(super) fn cancel(
        &mut self,
        reason: CancellationReason,
        conversation_id: AIConversationId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.current_request_id = None;
        let Some(cancellation_tx) = self.cancellation_tx.take() else {
            return;
        };
        let _ = cancellation_tx.send(());
        ctx.emit(ResponseStreamEvent::AfterStreamFinished {
            cancellation: Some(StreamCancellation {
                reason,
                conversation_id,
            }),
        });
    }

    fn handle_response_stream_result(
        &mut self,
        request_id: Uuid,
        stream_result: Result<api::ResponseStream, ConvertToAPITypeError>,
        ctx: &mut ModelContext<Self>,
    ) {
        match stream_result {
            Ok(stream) => {
                ctx.spawn_stream_local(
                    stream,
                    move |me, event, ctx| {
                        me.handle_response_stream_event(request_id, event, ctx);
                    },
                    move |me, ctx| {
                        me.on_response_stream_complete(request_id, ctx);
                    },
                );
            }
            Err(e) => {
                // Own the converted error so it can be reported fully typed (preserving
                // is_actionable() classification) and still moved into `AIApiError::Other`
                // below; borrowing it for the report would drop the static context message
                // and log only the conversion error's own message.
                let converted_error = anyhow::Error::new(e);
                report_error!(&converted_error);
                if self.current_request_id.is_none_or(|id| id != request_id) {
                    return;
                }
                // A request-conversion failure is a deterministic client-side error and
                // no stream was ever created: retrying would fail identically, and
                // letting completion synthesize `UnexpectedEof` would misreport it as
                // a transient network failure. Surface the original error and finish
                // terminally. (HTTP send failures don't take this path — they arrive as
                // in-stream error events.)
                let error = Arc::new(AIApiError::Other(converted_error));
                self.error_event_emitted = true;
                self.report_request_failure(&error, NetworkStatus::as_ref(ctx).is_online());
                ctx.emit(ResponseStreamEvent::ReceivedEvent(Consumable::new(Err(
                    error,
                ))));
                self.on_response_stream_complete(request_id, ctx);
            }
        }
    }

    fn handle_response_stream_event(
        &mut self,
        request_id: Uuid,
        event: api::Event,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.current_request_id.is_none_or(|id| id != request_id) {
            return;
        }

        match &event {
            Ok(response_event) => {
                if let Some(event_type) = &response_event.r#type {
                    match event_type {
                        warp_multi_agent_api::response_event::Type::Init(_) => {}
                        warp_multi_agent_api::response_event::Type::ClientActions(_) => {
                            // Mark that we've received client actions
                            self.has_received_client_actions = true;
                        }
                        warp_multi_agent_api::response_event::Type::Finished(_) => {
                            self.stream_finished_received = true;
                        }
                    }
                }
                ctx.emit(ResponseStreamEvent::ReceivedEvent(Consumable::new(event)));
            }
            Err(e) => {
                if self.recover_or_retry(e, ctx) {
                    return;
                }
                ctx.emit(ResponseStreamEvent::ReceivedEvent(Consumable::new(event)));
            }
        }
    }

    /// Recovers from a failed attempt. Returns true when the request will be sent again, in which
    /// case the failure is not surfaced; otherwise records it as surfaced, with a resume scheduled
    /// when one applies.
    fn recover_or_retry(&mut self, error: &Arc<AIApiError>, ctx: &mut ModelContext<Self>) -> bool {
        let is_online = NetworkStatus::as_ref(ctx).is_online();
        match recovery_action(
            AttemptFailure::of(error, self.has_received_client_actions),
            self.attempt_may_retry,
            self.retry_count,
            self.can_attempt_resume_on_error,
            is_online,
        ) {
            RecoveryAction::RetryAfter(delay) => {
                log::warn!(
                    "Agent request failed before its reply; retrying ({}/{MAX_RETRIES}) in {delay:?}",
                    self.retry_count + 1,
                );
                self.retry_after(delay, ctx);
                return true;
            }
            RecoveryAction::RetryWhenOnline => {
                log::warn!(
                    "Agent request failed while offline; retrying ({}/{MAX_RETRIES}) once connectivity returns",
                    self.retry_count + 1,
                );
                self.defer_retry_until_online(ctx);
                return true;
            }
            RecoveryAction::Resume => {
                // Recoverable failure after part of the reply: we'll resume the conversation once
                // the stream finishes rather than surface the error, so the UI suppresses the
                // banner. Log it so the auto-recovery isn't completely silent.
                log::warn!(
                    "Agent request failed after its reply started; resuming conversation after stream finishes"
                );
                // The resume spawn itself waits for connectivity.
                self.should_resume_conversation_after_stream_finished = true;
            }
            RecoveryAction::Fail => {}
        }
        self.error_event_emitted = true;
        self.report_request_failure(error, is_online);
        false
    }

    fn on_response_stream_complete(&mut self, request_id: Uuid, ctx: &mut ModelContext<Self>) {
        if self.current_request_id.is_none_or(|id| id != request_id) {
            return;
        }
        // A retry is parked waiting for connectivity; the request is logically still
        // active, so don't complete the stream for the failed attempt.
        if self.deferred_retry_pending {
            return;
        }

        // The server always sends a StreamFinished event before ending the response,
        // but a transport cut between chunks surfaces as a clean EOF. Synthesize the
        // failure and recover like any transient error.
        if !self.stream_finished_received && !self.error_event_emitted {
            log::warn!(
                "generate_multi_agent_output stream ended without emitting StreamFinished event."
            );
            let unexpected_eof = Arc::new(AIApiError::UnexpectedEof);
            if self.recover_or_retry(&unexpected_eof, ctx) {
                return;
            }
            ctx.emit(ResponseStreamEvent::ReceivedEvent(Consumable::new(Err(
                unexpected_eof,
            ))));
        }

        ctx.emit(ResponseStreamEvent::AfterStreamFinished { cancellation: None });
        self.cancellation_tx = None;
    }

    /// Reports a non-retried request failure with classification details.
    fn report_request_failure(&self, error: &Arc<AIApiError>, is_online: bool) {
        report_error!(
            error.as_ref(),
            extra: {
                "has_received_client_actions" => self.has_received_client_actions,
                "is_recoverable" => error.is_recoverable(),
                "will_attempt_resume" => self.should_resume_conversation_after_stream_finished,
                "is_online" => is_online,
                "retry_count" => self.retry_count,
                "error_debug" => %format!("{error:?}"),
            }
        );
    }

    /// Re-sends the request after `delay`, showing that it is retrying meanwhile; cancellation
    /// invalidates the pending retry through `current_request_id`.
    fn retry_after(&mut self, delay: Duration, ctx: &mut ModelContext<Self>) {
        self.deferred_retry_pending = true;
        self.is_retry_notice_shown = true;
        ctx.emit(ResponseStreamEvent::Retrying(Some(RetryNotice {
            retry: self.retry_count + 1,
            max_retries: MAX_RETRIES,
        })));
        let request_id_at_defer = self.current_request_id;
        let _ = ctx.spawn(Timer::after(delay), move |me, _, ctx| {
            if request_id_at_defer.is_none() || me.current_request_id != request_id_at_defer {
                return;
            }
            me.retry(ctx);
        });
    }

    /// Parks a retry until connectivity returns; cancellation invalidates the parked
    /// retry through `current_request_id`.
    fn defer_retry_until_online(&mut self, ctx: &mut ModelContext<Self>) {
        self.deferred_retry_pending = true;
        ctx.emit(ResponseStreamEvent::WaitingForNetwork { waiting: true });
        let request_id_at_defer = self.current_request_id;
        let wait_for_online = NetworkStatus::as_ref(ctx).wait_until_online();
        let _ = ctx.spawn(wait_for_online, move |me, _, ctx| {
            // Cancelled or superseded while waiting — drop the parked retry.
            if request_id_at_defer.is_none() || me.current_request_id != request_id_at_defer {
                return;
            }
            ctx.emit(ResponseStreamEvent::WaitingForNetwork { waiting: false });
            me.retry(ctx);
        });
    }
}

#[derive(Debug)]
pub struct Consumable<T> {
    value: Rc<RefCell<Option<T>>>,
}

impl<T> Consumable<T> {
    fn new(value: T) -> Self {
        Consumable {
            value: Rc::new(RefCell::new(Some(value))),
        }
    }

    pub(super) fn consume(&self) -> Option<T> {
        self.value.borrow_mut().take()
    }
}

impl<T> Clone for Consumable<T> {
    fn clone(&self) -> Self {
        Consumable {
            value: Rc::clone(&self.value),
        }
    }
}

/// Cancellation context preserved for async event handling.
/// Includes conversation_id because truncation can remove exchange mappings before the event is processed.
#[derive(Debug, Clone)]
pub struct StreamCancellation {
    pub reason: CancellationReason,
    pub conversation_id: AIConversationId,
}

#[derive(Debug, Clone)]
pub enum ResponseStreamEvent {
    ReceivedEvent(Consumable<api::Event>),
    /// A retry is parked until connectivity returns (`waiting: true`) or has just
    /// fired (`waiting: false`). The controller mirrors this on the conversation
    /// status (`TransientError` ↔ `InProgress`).
    ///
    /// Only emitted from `defer_retry_until_online`, i.e. always after a recoverable
    /// request failure while offline — never speculatively before an attempt. Consumers
    /// can therefore treat `waiting: true` as a transient-error (reconnecting) state.
    WaitingForNetwork {
        waiting: bool,
    },
    /// A failed attempt will be sent again after a wait (`Some`), or the wait is over and the
    /// request has been sent again (`None`).
    Retrying(Option<RetryNotice>),
    AfterStreamFinished {
        /// Some for cancellation (with context), None for natural completion (uses dynamic lookup).
        cancellation: Option<StreamCancellation>,
    },
}

impl Entity for ResponseStream {
    type Event = ResponseStreamEvent;
}

#[cfg(test)]
#[path = "response_stream_tests.rs"]
mod tests;
