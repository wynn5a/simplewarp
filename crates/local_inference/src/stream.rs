//! The entry point: opens a provider stream and turns it into response events.
//!
//! This is the drop-in replacement for
//! `warp_multi_agent_client::generate_multi_agent_output`. The signature is the same shape, less
//! the server client, because nothing here needs an access token.

use std::collections::VecDeque;

use futures::{StreamExt, stream};
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
use warp_multi_agent_api as api;

use crate::config::Schema;
use crate::emit::Emitter;
use crate::provider::{Delta, StopReason};
use crate::{Error, OutputStream, config, convert, prompt, provider, tools};

/// Sends one agent request to the user's own provider, and streams the reply back as the
/// response events that the client applies.
pub async fn generate_local_output(request: &api::Request) -> Result<OutputStream, Error> {
    let target = config::resolve_target(request)?;
    let turns = convert::turns_from_request(request);
    if turns.is_empty() {
        return Err(Error::NoInput);
    }

    let supported_tools = request
        .settings
        .as_ref()
        .map(|settings| settings.supported_tools.clone())
        .unwrap_or_default();
    let tool_schemas = tools::schemas_for(&supported_tools);

    let body = provider::build_body(&target, prompt::SYSTEM_PROMPT, &turns, &tool_schemas);

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
        })?;

    let mut emitter = Emitter::new(request);
    let opening = emitter.start();

    let state = State {
        source,
        emitter,
        schema: target.schema,
        queue: opening.into(),
        stop: None,
        done: false,
    };

    Ok(stream::unfold(state, next_event).boxed())
}

struct State {
    source: EventSource,
    emitter: Emitter,
    schema: Schema,
    /// Events that are ready to hand to the client.
    queue: VecDeque<api::ResponseEvent>,
    /// The stop reason from the provider, kept until the stream closes.
    stop: Option<StopReason>,
    done: bool,
}

/// Pulls the next event, reading more of the provider stream when the queue runs dry.
async fn next_event(mut state: State) -> Option<(Result<api::ResponseEvent, Error>, State)> {
    loop {
        if let Some(event) = state.queue.pop_front() {
            return Some((Ok(event), state));
        }
        if state.done {
            return None;
        }

        match state.source.next().await {
            Some(Ok(Event::Open)) => continue,
            Some(Ok(Event::Message(message))) => {
                for delta in provider::parse_event(state.schema, &message.event, &message.data) {
                    if let Delta::Stop(reason) = delta {
                        state.stop = Some(reason);
                    } else {
                        state.queue.extend(state.emitter.on_delta(delta));
                    }
                }
            }
            // A closed stream is the normal end of a reply. The provider sent its stop reason
            // just before, so the reply is finished with that reason.
            Some(Err(reqwest_eventsource::Error::StreamEnded)) | None => {
                let reason = state.stop.unwrap_or(StopReason::EndTurn);
                state.queue.extend(state.emitter.finish(reason));
                state.done = true;
            }
            Some(Err(reqwest_eventsource::Error::InvalidStatusCode(status, response))) => {
                let body = response.text().await.unwrap_or_default();
                state.done = true;
                return Some((
                    Err(Error::ProviderStatus {
                        status: status.as_u16(),
                        body,
                    }),
                    state,
                ));
            }
            Some(Err(error)) => {
                state.done = true;
                return Some((Err(Error::EventSource(Box::new(error))), state));
            }
        }
    }
}
