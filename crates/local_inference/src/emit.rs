//! Turns a stream of provider [`Delta`]s into the [`api::ResponseEvent`]s that the client
//! applies.
//!
//! The client owns the conversation state. Before it sends a request it has already made the
//! task and the exchange that this reply belongs to, so the normal path only adds messages to
//! that task. A `CreateTask` goes out only when the request carried no task at all.
//!
//! The event order for one reply is:
//!
//! ```text
//! Init
//! BeginTransaction
//!   AddMessagesToTask        (the user's question, when the request carried one)
//!   AddMessagesToTask        (the first piece of text, which makes the message)
//!   AppendToMessageContent   (every later piece, appended to the same message)
//!   AddMessagesToTask        (one message per tool call, once the arguments are whole)
//! CommitTransaction
//! Finished(Done)
//! ```

use std::collections::BTreeMap;

use serde_json::Value;
use uuid::Uuid;
use warp_multi_agent_api as api;
use warp_multi_agent_api::client_action::Action;

use crate::provider::{Delta, StopReason};
use crate::tools;

/// Builds the response events for one reply.
pub struct Emitter {
    conversation_id: String,
    request_id: String,
    task_id: String,
    /// True when the request carried no task, so the client has nothing to add messages to.
    needs_create_task: bool,
    /// The message that streamed text is appended to, once it exists.
    text_message_id: Option<String>,
    /// The message that streamed reasoning is appended to, once it exists.
    reasoning_message_id: Option<String>,
    /// The question this reply answers, when the request carried one.
    ///
    /// The client sends the question in `Request::input` and keeps its own copy for the block it
    /// draws, but it never puts one in the task. Warp's server did that, and everything that reads
    /// a conversation back still expects it to be there. See [`Self::start`].
    user_query: Option<api::message::UserQuery>,
    /// Tool calls being assembled, keyed by the index in the reply.
    pending_tools: BTreeMap<usize, PendingTool>,
}

struct PendingTool {
    id: String,
    name: String,
    /// The JSON arguments, which arrive in fragments.
    arguments: String,
}

impl Emitter {
    /// Reads the conversation and task identity out of the request.
    ///
    /// The proto no longer carries an active task id, so the last task is taken as the active
    /// one. That matches the client, which registers exactly one exchange for each request and
    /// appends the newest task last.
    pub fn new(request: &api::Request) -> Self {
        let conversation_id = request
            .metadata
            .as_ref()
            .map(|metadata| metadata.conversation_id.clone())
            .filter(|id| !id.is_empty())
            .unwrap_or_else(new_id);

        let existing_task_id = request
            .task_context
            .as_ref()
            .and_then(|context| context.tasks.last())
            .map(|task| task.id.clone())
            .filter(|id| !id.is_empty());

        let needs_create_task = existing_task_id.is_none();

        Self {
            conversation_id,
            request_id: new_id(),
            task_id: existing_task_id.unwrap_or_else(new_id),
            needs_create_task,
            text_message_id: None,
            reasoning_message_id: None,
            user_query: user_query_from_request(request),
            pending_tools: BTreeMap::new(),
        }
    }

    /// The events that open the reply.
    pub fn start(&mut self) -> Vec<api::ResponseEvent> {
        let mut events = vec![event(api::response_event::Type::Init(
            api::response_event::StreamInit {
                conversation_id: self.conversation_id.clone(),
                request_id: self.request_id.clone(),
                run_id: String::new(),
            },
        ))];

        let mut actions = vec![Action::BeginTransaction(Default::default())];
        if self.needs_create_task {
            actions.push(Action::CreateTask(api::client_action::CreateTask {
                task: Some(api::Task {
                    id: self.task_id.clone(),
                    ..Default::default()
                }),
            }));
        }

        // Store the question before the reply to it.
        //
        // The client sends the question in `Request::input`, not as a message, and Warp's server
        // was what echoed it back so the client could keep it. Without that echo the task holds
        // the reply and the tool calls but not the question, which breaks two things at once:
        //
        // - The history panel drops the conversation. `AgentConversationSummary` reads its
        //   `initial_query` by looking for a `UserQuery` in the root task, finds none, and logs
        //   `missing an initial query`.
        // - A later request replays the task to the model, so the model is shown its own past
        //   replies and tool calls without the questions that prompted them.
        //
        // This does not double up in the UI. A `UserQuery` message only becomes a rendered input
        // when `Task::add_messages` is told to convert input messages, which the client does for
        // a shared-session viewer alone; in a normal session its own copy already fills the
        // exchange. Here the message lands in the task's message list, which is what gets
        // persisted and replayed.
        if let Some(query) = self.user_query.take() {
            actions.push(add_action(
                &self.task_id,
                &new_id(),
                api::message::Message::UserQuery(query),
            ));
        }

        events.push(actions_event(actions));
        events
    }

    /// The events for one piece of the reply. A piece often produces nothing, because tool
    /// arguments are only sent once they are whole.
    pub fn on_delta(&mut self, delta: Delta) -> Vec<api::ResponseEvent> {
        let actions = match delta {
            Delta::Text(text) => self.on_text(text),
            Delta::Reasoning(text) => self.on_reasoning(text),
            Delta::ToolCallStart { index, id, name } => {
                self.pending_tools.insert(
                    index,
                    PendingTool {
                        id,
                        name,
                        arguments: String::new(),
                    },
                );
                Vec::new()
            }
            Delta::ToolCallArguments { index, fragment } => {
                if let Some(pending) = self.pending_tools.get_mut(&index) {
                    pending.arguments.push_str(&fragment);
                }
                Vec::new()
            }
            // The caller ends the reply with `finish`, so that a stop from the provider and a
            // stream that simply closes take the same path.
            Delta::Stop(_) => Vec::new(),
        };

        if actions.is_empty() {
            Vec::new()
        } else {
            vec![actions_event(actions)]
        }
    }

    /// The events that close the reply: the tool calls, then the commit and the finish.
    pub fn finish(&mut self, reason: StopReason) -> Vec<api::ResponseEvent> {
        let mut actions = Vec::new();

        let tool_messages = std::mem::take(&mut self.pending_tools)
            .into_values()
            .filter_map(|pending| {
                let arguments = parse_arguments(&pending.arguments);
                let call = tools::to_proto(&pending.id, &pending.name, &arguments)?;
                Some(message(api::message::Message::ToolCall(call)))
            })
            .collect::<Vec<_>>();

        if !tool_messages.is_empty() {
            actions.push(Action::AddMessagesToTask(
                api::client_action::AddMessagesToTask {
                    task_id: self.task_id.clone(),
                    messages: tool_messages,
                },
            ));
        }

        actions.push(Action::CommitTransaction(Default::default()));

        vec![
            actions_event(actions),
            event(api::response_event::Type::Finished(
                api::response_event::StreamFinished {
                    reason: Some(finish_reason(reason)),
                    ..Default::default()
                },
            )),
        ]
    }

    fn on_text(&mut self, text: String) -> Vec<Action> {
        match self.text_message_id.clone() {
            Some(id) => vec![append_action(
                &self.task_id,
                &id,
                api::message::Message::AgentOutput(api::message::AgentOutput { text }),
                "agent_output.text",
            )],
            None => {
                let id = new_id();
                self.text_message_id = Some(id.clone());
                vec![add_action(
                    &self.task_id,
                    &id,
                    api::message::Message::AgentOutput(api::message::AgentOutput { text }),
                )]
            }
        }
    }

    fn on_reasoning(&mut self, reasoning: String) -> Vec<Action> {
        match self.reasoning_message_id.clone() {
            Some(id) => vec![append_action(
                &self.task_id,
                &id,
                api::message::Message::AgentReasoning(api::message::AgentReasoning {
                    reasoning,
                    ..Default::default()
                }),
                "agent_reasoning.reasoning",
            )],
            None => {
                let id = new_id();
                self.reasoning_message_id = Some(id.clone());
                vec![add_action(
                    &self.task_id,
                    &id,
                    api::message::Message::AgentReasoning(api::message::AgentReasoning {
                        reasoning,
                        ..Default::default()
                    }),
                )]
            }
        }
    }
}

/// Reads the question the user asked in this request, if it asked one.
///
/// A request whose input is a set of tool results — the next step of an agent loop — carries no
/// question, and must not be given one. Only the first request of a turn has it.
///
/// The deprecated `UserQuery` input variant is still read, because a conversation that an older
/// client started can carry it.
#[allow(deprecated)]
fn user_query_from_request(request: &api::Request) -> Option<api::message::UserQuery> {
    use api::request::input::Type;
    use api::request::input::user_inputs::user_input::Input as UserInput;

    let query = match request.input.as_ref()?.r#type.as_ref()? {
        Type::UserInputs(inputs) => inputs.inputs.iter().find_map(|entry| {
            let UserInput::UserQuery(query) = entry.input.as_ref()? else {
                return None;
            };
            // `Attachment` and `UserQueryMode` are the same types on both messages, so the extra
            // fields carry over as they are. `context` has no counterpart on the input, so a
            // stored query has none either.
            Some(api::message::UserQuery {
                query: query.query.clone(),
                referenced_attachments: query.referenced_attachments.clone(),
                mode: query.mode,
                intended_agent: query.intended_agent,
                ..Default::default()
            })
        })?,
        Type::UserQuery(query) => api::message::UserQuery {
            query: query.query.clone(),
            ..Default::default()
        },
        _ => return None,
    };

    // An empty question is not one. Storing it would leave the history panel with a blank title
    // and tell the model nothing.
    if query.query.is_empty() {
        return None;
    }
    Some(query)
}

/// Reads the collected argument fragments.
///
/// A model that calls a tool with no arguments sends nothing, and a model that is cut off part
/// way sends a fragment that is not valid JSON. Both become an empty object, so that the tool
/// converter can decide whether the call is usable.
fn parse_arguments(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return Value::Object(Default::default());
    }
    serde_json::from_str(raw).unwrap_or_else(|_| Value::Object(Default::default()))
}

fn add_action(task_id: &str, message_id: &str, inner: api::message::Message) -> Action {
    Action::AddMessagesToTask(api::client_action::AddMessagesToTask {
        task_id: task_id.to_string(),
        messages: vec![api::Message {
            id: message_id.to_string(),
            task_id: task_id.to_string(),
            message: Some(inner),
            ..Default::default()
        }],
    })
}

fn append_action(
    task_id: &str,
    message_id: &str,
    inner: api::message::Message,
    mask_path: &str,
) -> Action {
    Action::AppendToMessageContent(api::client_action::AppendToMessageContent {
        task_id: task_id.to_string(),
        message: Some(api::Message {
            id: message_id.to_string(),
            task_id: task_id.to_string(),
            message: Some(inner),
            ..Default::default()
        }),
        mask: Some(prost_types::FieldMask {
            paths: vec![mask_path.to_string()],
        }),
    })
}

fn message(inner: api::message::Message) -> api::Message {
    api::Message {
        id: new_id(),
        message: Some(inner),
        ..Default::default()
    }
}

fn event(r#type: api::response_event::Type) -> api::ResponseEvent {
    api::ResponseEvent {
        r#type: Some(r#type),
    }
}

fn actions_event(actions: Vec<Action>) -> api::ResponseEvent {
    event(api::response_event::Type::ClientActions(
        api::response_event::ClientActions {
            actions: actions
                .into_iter()
                .map(|action| api::ClientAction {
                    action: Some(action),
                })
                .collect(),
        },
    ))
}

fn finish_reason(reason: StopReason) -> api::response_event::stream_finished::Reason {
    use api::response_event::stream_finished::Reason;

    match reason {
        StopReason::MaxTokens => Reason::MaxTokenLimit(Default::default()),
        // A tool-use stop is a normal end of the reply: the client runs the tools and sends the
        // results back in the next request.
        StopReason::EndTurn | StopReason::ToolUse => Reason::Done(Default::default()),
        StopReason::Other => Reason::Other(Default::default()),
    }
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
#[path = "emit_tests.rs"]
mod tests;
