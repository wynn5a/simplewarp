//! The entry point: opens a provider stream and turns it into response events.
//!
//! This is the drop-in replacement for
//! `warp_multi_agent_client::generate_multi_agent_output`. The signature is the same shape, less
//! the server client, because nothing here needs an access token.

use std::collections::VecDeque;

use agent_harness::compaction::SUMMARIZATION_SYSTEM_PROMPT;
use agent_harness::retry;
use futures::{StreamExt, stream};
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
use warp_multi_agent_api as api;

use crate::config::Schema;
use crate::emit::Emitter;
use crate::mcp::McpCatalog;
use crate::provider::{Delta, StopReason};
use crate::{
    Error, Failure, OutputStream, compaction, config, convert, overflow, prompt, provider, tools,
};

/// Sends one agent request to the user's own provider, and streams the reply back as the
/// response events that the client applies.
pub async fn generate_local_output(request: &api::Request) -> Result<OutputStream, Error> {
    let target = config::resolve_target(request)?;
    let compaction = compaction::plan(request);
    // A summary request shows the model the conversation as text to summarize, with a prompt of
    // its own and no tools, so that it writes the summary instead of carrying on.
    let (prompt, turns, tool_schemas) = match compaction.as_ref() {
        Some(plan) => (
            prompt::Prompt {
                system: SUMMARIZATION_SYSTEM_PROMPT.to_string(),
                current_state: String::new(),
                max_output_tokens: plan.max_output_tokens,
            },
            vec![convert::Turn::User(plan.prompt_text.clone())],
            Vec::new(),
        ),
        None => {
            let supported_tools = request
                .settings
                .as_ref()
                .map(|settings| settings.supported_tools.clone())
                .unwrap_or_default();
            let mut tool_schemas = tools::schemas_for(&supported_tools);
            tool_schemas.extend(McpCatalog::from_request(request).schemas(&supported_tools));
            (
                prompt::Prompt::for_request(request)
                    .with_appended_system(prompt::read_appended_system().as_deref()),
                convert::turns_from_request(request),
                tool_schemas,
            )
        }
    };
    if turns.is_empty() {
        return Err(Error::NoInput);
    }

    let body = provider::build_body(&target, &prompt, &turns, &tool_schemas);

    let mut builder = reqwest::Client::new()
        .post(target.endpoint_url())
        .header("content-type", "application/json")
        .json(&body);
    for (name, value) in provider::headers(&target) {
        builder = builder.header(name, value);
    }

    let source = builder
        .eventsource()
        .map_err(|error| Error::ProviderStatus {
            status: 0,
            body: error.to_string(),
            retry_after: None,
        })?;

    let mut emitter = Emitter::new(request).replaying_reasoning(target.replays_reasoning());
    if let Some(plan) = compaction {
        emitter = emitter.with_compaction(plan.outcome);
    }
    let reply = Reply::new(emitter, target.schema, request);

    Ok(stream::unfold(State { source, reply }, next_event).boxed())
}

struct State {
    source: EventSource,
    reply: Reply,
}

/// What the stream hands the client, built from the provider's events. Kept apart from the
/// connection so that it can be driven without one.
struct Reply {
    emitter: Emitter,
    schema: Schema,
    /// Events that are ready to hand to the client. The reply's opening (`Init`, the transaction,
    /// the stored inputs, a summary's placeholder) is queued at once, so the conversation holds
    /// what the request sent, and shows a summary in progress, however long the provider takes.
    queue: VecDeque<api::ResponseEvent>,
    /// Whether the provider has started replying: some event of the reply itself was queued.
    reply_started: bool,
    /// The failure that ends the stream, handed over after the queue.
    failure: Option<Failure>,
    /// The stop reason from the provider, kept until the stream closes.
    stop: Option<StopReason>,
    done: bool,
    /// Whether the client sends the request again when it fails retryably before the reply.
    client_retries: bool,
    /// A summary request stores nothing worth keeping when it fails: its stored input is the
    /// `/compact` instruction, which is not replayed, and its opening holds an unfinished summary.
    is_summary: bool,
}

impl Reply {
    fn new(mut emitter: Emitter, schema: Schema, request: &api::Request) -> Self {
        let opening = emitter.start();
        Self {
            emitter,
            schema,
            queue: opening.into(),
            reply_started: false,
            failure: None,
            stop: None,
            done: false,
            client_retries: request
                .settings
                .as_ref()
                .is_some_and(|settings| settings.will_retry_failures_before_reply),
            is_summary: matches!(
                request
                    .input
                    .as_ref()
                    .and_then(|input| input.r#type.as_ref()),
                Some(api::request::input::Type::SummarizeConversation(_))
            ),
        }
    }

    /// The next item for the client, if one is ready.
    fn next(&mut self) -> Option<Result<api::ResponseEvent, Failure>> {
        match self.queue.pop_front() {
            Some(event) => Some(Ok(event)),
            None => self.failure.take().map(Err),
        }
    }

    /// Queues events of the reply.
    fn push(&mut self, events: Vec<api::ResponseEvent>) {
        if !events.is_empty() {
            self.reply_started = true;
            self.queue.extend(events);
        }
    }

    /// Reads one server-sent event.
    fn on_message(&mut self, event_name: &str, data: &str) {
        for delta in provider::parse_event(self.schema, event_name, data) {
            match delta {
                Delta::Stop(reason) => self.stop = Some(reason),
                Delta::Error { code, message } if overflow::is_overflow_error(&code, &message) => {
                    self.finish_overflowed();
                    return;
                }
                Delta::Error { code, message } => {
                    self.fail(Error::ProviderStream { code, message });
                    return;
                }
                delta => {
                    let events = self.emitter.on_delta(delta);
                    self.push(events);
                }
            }
        }
    }

    /// Ends the reply when the provider closes the stream. It sent its stop reason just before, so
    /// the reply is finished with that reason.
    fn on_end(&mut self) {
        let reason = self.stop.unwrap_or(StopReason::EndTurn);
        let events = self.emitter.finish(reason);
        self.push(events);
        self.done = true;
    }

    /// Ends the reply as one that did not fit in the context window.
    fn finish_overflowed(&mut self) {
        log::info!(
            "the provider rejected a request to {} for exceeding the context window",
            self.emitter.model_id()
        );
        let events = self.emitter.finish(StopReason::ContextWindowExceeded);
        self.push(events);
        self.done = true;
    }

    /// Ends the stream with `error`.
    ///
    /// Before the reply started, what the opening stored is rolled back when the client is about
    /// to send the same request again (which stores it then) or the request is a summary;
    /// otherwise the conversation keeps it, as for any failed request.
    fn fail(&mut self, error: Error) {
        self.done = true;
        let before_reply = !self.reply_started;
        if before_reply && (self.is_summary || (self.client_retries && error.is_retryable())) {
            let rollback = self.emitter.rollback();
            self.queue.push_back(rollback);
        }
        self.failure = Some(Failure {
            error,
            before_reply,
        });
    }
}

/// Pulls the next event, reading more of the provider stream when the queue runs dry.
async fn next_event(mut state: State) -> Option<(Result<api::ResponseEvent, Failure>, State)> {
    loop {
        if let Some(item) = state.reply.next() {
            return Some((item, state));
        }
        if state.reply.done {
            return None;
        }

        match state.source.next().await {
            Some(Ok(Event::Open)) => continue,
            Some(Ok(Event::Message(message))) => {
                state.reply.on_message(&message.event, &message.data);
            }
            Some(Err(reqwest_eventsource::Error::StreamEnded)) | None => state.reply.on_end(),
            Some(Err(reqwest_eventsource::Error::InvalidStatusCode(status, response))) => {
                let header = |name: &str| {
                    response
                        .headers()
                        .get(name)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned)
                };
                let retry_after = retry::parse_retry_after(
                    header("retry-after").as_deref(),
                    header("retry-after-ms").as_deref(),
                );
                let body = response.text().await.unwrap_or_default();
                if overflow::is_overflow_response(status.as_u16(), &body) {
                    state.reply.finish_overflowed();
                    continue;
                }
                state.reply.fail(Error::ProviderStatus {
                    status: status.as_u16(),
                    body,
                    retry_after,
                });
            }
            Some(Err(error)) => state.reply.fail(Error::EventSource(Box::new(error))),
        }
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
