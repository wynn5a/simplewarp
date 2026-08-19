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
