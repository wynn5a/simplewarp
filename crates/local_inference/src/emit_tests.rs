use warp_multi_agent_api as api;

use super::*;

fn request_with_task(task_id: &str) -> api::Request {
    api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: vec![api::Task {
                id: task_id.to_string(),
                ..Default::default()
            }],
        }),
        ..Default::default()
    }
}

/// Collects the actions out of a list of events, so that a test can read them in order.
fn actions_of(events: &[api::ResponseEvent]) -> Vec<Action> {
    events
        .iter()
        .filter_map(|event| match event.r#type.as_ref() {
            Some(api::response_event::Type::ClientActions(actions)) => Some(actions),
            _ => None,
        })
        .flat_map(|actions| actions.actions.iter())
        .filter_map(|action| action.action.clone())
        .collect()
}

#[test]
fn the_reply_opens_with_init_and_a_transaction() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let events = emitter.start();

    assert!(matches!(
        events[0].r#type,
        Some(api::response_event::Type::Init(_))
    ));
    assert!(matches!(
        actions_of(&events).as_slice(),
        [Action::BeginTransaction(_)]
    ));
}

#[test]
fn a_request_with_no_task_creates_one() {
    let mut emitter = Emitter::new(&api::Request::default());
    let events = emitter.start();

    let actions = actions_of(&events);
    assert!(matches!(actions[0], Action::BeginTransaction(_)));
    let Action::CreateTask(create) = &actions[1] else {
        panic!("expected a CreateTask, got {:?}", actions[1]);
    };
    assert!(!create.task.as_ref().expect("a task").id.is_empty());
}

#[test]
fn a_request_with_a_task_does_not_create_one() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let actions = actions_of(&emitter.start());
    assert_eq!(actions.len(), 1, "expected only BeginTransaction");
}

#[test]
fn the_first_text_adds_a_message_and_the_rest_append_to_it() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let _ = emitter.start();

    let first = actions_of(&emitter.on_delta(Delta::Text("Hel".to_string())));
    let Action::AddMessagesToTask(add) = &first[0] else {
        panic!("expected an AddMessagesToTask, got {:?}", first[0]);
    };
    assert_eq!(add.task_id, "task-1");
    let message_id = add.messages[0].id.clone();
    assert!(!message_id.is_empty());

    let second = actions_of(&emitter.on_delta(Delta::Text("lo".to_string())));
    let Action::AppendToMessageContent(append) = &second[0] else {
        panic!("expected an AppendToMessageContent, got {:?}", second[0]);
    };
    assert_eq!(append.task_id, "task-1");
    assert_eq!(
        append.message.as_ref().expect("a message").id,
        message_id,
        "the append must target the message that the first delta made"
    );
    assert_eq!(
        append.mask.as_ref().expect("a mask").paths,
        vec!["agent_output.text".to_string()]
    );
}

#[test]
fn reasoning_and_text_go_to_separate_messages() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let _ = emitter.start();

    let reasoning = actions_of(&emitter.on_delta(Delta::Reasoning("hmm".to_string())));
    let Action::AddMessagesToTask(add_reasoning) = &reasoning[0] else {
        panic!("expected an AddMessagesToTask");
    };
    let reasoning_id = add_reasoning.messages[0].id.clone();

    let text = actions_of(&emitter.on_delta(Delta::Text("Hello".to_string())));
    let Action::AddMessagesToTask(add_text) = &text[0] else {
        panic!("expected an AddMessagesToTask");
    };
    assert_ne!(add_text.messages[0].id, reasoning_id);
}

#[test]
fn a_tool_call_is_held_back_until_the_reply_ends() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let _ = emitter.start();

    let start = emitter.on_delta(Delta::ToolCallStart {
        index: 0,
        id: "call-1".to_string(),
        name: "run_shell_command".to_string(),
    });
    assert!(start.is_empty(), "a partial tool call must not be emitted");

    let partial = emitter.on_delta(Delta::ToolCallArguments {
        index: 0,
        fragment: "{\"command\":".to_string(),
    });
    assert!(partial.is_empty());

    let rest = emitter.on_delta(Delta::ToolCallArguments {
        index: 0,
        fragment: "\"ls\"}".to_string(),
    });
    assert!(rest.is_empty());

    let actions = actions_of(&emitter.finish(StopReason::ToolUse));
    let Action::AddMessagesToTask(add) = &actions[0] else {
        panic!("expected an AddMessagesToTask, got {:?}", actions[0]);
    };
    let Some(api::message::Message::ToolCall(call)) = &add.messages[0].message else {
        panic!("expected a tool call message");
    };
    assert_eq!(call.tool_call_id, "call-1");
    let Some(api::message::tool_call::Tool::RunShellCommand(run)) = &call.tool else {
        panic!("expected a shell command");
    };
    assert_eq!(run.command, "ls");
}

#[test]
fn parallel_tool_calls_keep_the_order_the_model_gave() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let _ = emitter.start();

    for (index, command) in [(1, "pwd"), (0, "ls")] {
        emitter.on_delta(Delta::ToolCallStart {
            index,
            id: format!("call-{index}"),
            name: "run_shell_command".to_string(),
        });
        emitter.on_delta(Delta::ToolCallArguments {
            index,
            fragment: format!("{{\"command\":\"{command}\"}}"),
        });
    }

    let actions = actions_of(&emitter.finish(StopReason::ToolUse));
    let Action::AddMessagesToTask(add) = &actions[0] else {
        panic!("expected an AddMessagesToTask");
    };
    assert_eq!(add.messages.len(), 2);
    // Index 0 was streamed second, but it must still come first.
    let ids: Vec<_> = add
        .messages
        .iter()
        .filter_map(|message| match &message.message {
            Some(api::message::Message::ToolCall(call)) => Some(call.tool_call_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec!["call-0".to_string(), "call-1".to_string()]);
}

#[test]
fn a_tool_call_with_broken_arguments_is_dropped() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let _ = emitter.start();

    emitter.on_delta(Delta::ToolCallStart {
        index: 0,
        id: "call-1".to_string(),
        name: "run_shell_command".to_string(),
    });
    // The stream was cut off part way through the arguments.
    emitter.on_delta(Delta::ToolCallArguments {
        index: 0,
        fragment: "{\"comm".to_string(),
    });

    let actions = actions_of(&emitter.finish(StopReason::ToolUse));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::AddMessagesToTask(_))),
        "an unusable tool call must not reach the client"
    );
    assert!(matches!(actions[0], Action::CommitTransaction(_)));
}

#[test]
fn an_invented_tool_name_is_dropped() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let _ = emitter.start();

    emitter.on_delta(Delta::ToolCallStart {
        index: 0,
        id: "call-1".to_string(),
        name: "delete_everything".to_string(),
    });
    emitter.on_delta(Delta::ToolCallArguments {
        index: 0,
        fragment: "{}".to_string(),
    });

    let actions = actions_of(&emitter.finish(StopReason::ToolUse));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::AddMessagesToTask(_))),
        "a tool that was never offered must not reach the client"
    );
}

#[test]
fn the_reply_closes_with_a_commit_and_a_finish() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let _ = emitter.start();
    let events = emitter.finish(StopReason::EndTurn);

    assert!(matches!(
        actions_of(&events).last(),
        Some(Action::CommitTransaction(_))
    ));
    let Some(api::response_event::Type::Finished(finished)) =
        &events.last().expect("an event").r#type
    else {
        panic!("expected a Finished event");
    };
    assert!(matches!(
        finished.reason,
        Some(api::response_event::stream_finished::Reason::Done(_))
    ));
}

#[test]
fn a_tool_use_stop_is_a_normal_finish() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let _ = emitter.start();
    let events = emitter.finish(StopReason::ToolUse);

    let Some(api::response_event::Type::Finished(finished)) =
        &events.last().expect("an event").r#type
    else {
        panic!("expected a Finished event");
    };
    assert!(
        matches!(
            finished.reason,
            Some(api::response_event::stream_finished::Reason::Done(_))
        ),
        "a tool-use stop means the client runs the tools, not that the reply failed"
    );
}

#[test]
fn hitting_the_token_limit_is_reported_as_such() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    let _ = emitter.start();
    let events = emitter.finish(StopReason::MaxTokens);

    let Some(api::response_event::Type::Finished(finished)) =
        &events.last().expect("an event").r#type
    else {
        panic!("expected a Finished event");
    };
    assert!(matches!(
        finished.reason,
        Some(api::response_event::stream_finished::Reason::MaxTokenLimit(
            _
        ))
    ));
}

#[test]
fn empty_arguments_become_an_empty_object() {
    assert_eq!(parse_arguments(""), serde_json::json!({}));
    assert_eq!(parse_arguments("   "), serde_json::json!({}));
    assert_eq!(parse_arguments("{\"a\":1}"), serde_json::json!({"a": 1}));
}

/// Builds a request whose input is a question, the way the client sends the first turn.
fn request_with_query(task_id: &str, query: &str) -> api::Request {
    use api::request::input::user_inputs::{UserInput, user_input};

    let mut request = request_with_task(task_id);
    request.input = Some(api::request::Input {
        r#type: Some(api::request::input::Type::UserInputs(
            api::request::input::UserInputs {
                inputs: vec![UserInput {
                    input: Some(user_input::Input::UserQuery(
                        api::request::input::UserQuery {
                            query: query.to_string(),
                            ..Default::default()
                        },
                    )),
                }],
            },
        )),
        ..Default::default()
    });
    request
}

/// Reads the queries that a list of actions stores on the task.
fn stored_queries(actions: &[Action]) -> Vec<String> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::AddMessagesToTask(add) => Some(&add.messages),
            _ => None,
        })
        .flatten()
        .filter_map(|message| match message.message.as_ref()? {
            api::message::Message::UserQuery(query) => Some(query.query.clone()),
            _ => None,
        })
        .collect()
}

/// Without this the task holds the reply but not the question, so the history panel drops the
/// conversation and a later request replays the answers without the questions.
#[test]
fn the_question_is_stored_before_the_reply_to_it() {
    let mut emitter = Emitter::new(&request_with_query("task-1", "how many rs files are here"));
    let events = emitter.start();

    let actions = actions_of(&events);
    assert!(
        matches!(actions[0], Action::BeginTransaction(_)),
        "the transaction must open first"
    );
    assert_eq!(stored_queries(&actions), vec!["how many rs files are here"]);
}

#[test]
fn the_question_is_stored_on_the_task_the_reply_belongs_to() {
    let mut emitter = Emitter::new(&request_with_query("task-1", "hello"));
    let events = emitter.start();

    let Some(Action::AddMessagesToTask(add)) = actions_of(&events)
        .into_iter()
        .find(|action| matches!(action, Action::AddMessagesToTask(_)))
    else {
        panic!("expected the question to be added to a task");
    };
    assert_eq!(add.task_id, "task-1");
    assert_eq!(add.messages[0].task_id, "task-1");
    assert!(!add.messages[0].id.is_empty(), "a message needs an id");
}

/// The later steps of an agent loop send tool results, not a question. Inventing one there would
/// put a second question in the conversation for a turn the user only asked once.
#[test]
fn a_request_carrying_tool_results_stores_no_question() {
    let mut request = request_with_task("task-1");
    request.input = Some(api::request::Input {
        r#type: Some(api::request::input::Type::UserInputs(
            api::request::input::UserInputs {
                inputs: vec![api::request::input::user_inputs::UserInput {
                    input: Some(
                        api::request::input::user_inputs::user_input::Input::ToolCallResult(
                            Default::default(),
                        ),
                    ),
                }],
            },
        )),
        ..Default::default()
    });

    let mut emitter = Emitter::new(&request);
    assert!(stored_queries(&actions_of(&emitter.start())).is_empty());
}

#[test]
fn a_request_with_no_input_stores_no_question() {
    let mut emitter = Emitter::new(&request_with_task("task-1"));
    assert!(stored_queries(&actions_of(&emitter.start())).is_empty());
}

#[test]
fn an_empty_question_is_not_stored() {
    let mut emitter = Emitter::new(&request_with_query("task-1", ""));
    assert!(stored_queries(&actions_of(&emitter.start())).is_empty());
}

/// `start` is called once per reply, so the question must not be repeated if it is called again.
#[test]
fn the_question_is_stored_once() {
    let mut emitter = Emitter::new(&request_with_query("task-1", "hello"));
    let first = actions_of(&emitter.start());
    let second = actions_of(&emitter.start());

    assert_eq!(stored_queries(&first), vec!["hello"]);
    assert!(stored_queries(&second).is_empty());
}

fn request_with_input_type(task_id: &str, r#type: api::request::input::Type) -> api::Request {
    let mut request = request_with_task(task_id);
    request.input = Some(api::request::Input {
        r#type: Some(r#type),
        ..Default::default()
    });
    request
}

/// The messages that `start` adds to the task, in order.
fn stored_messages(events: &[api::ResponseEvent]) -> Vec<api::message::Message> {
    actions_of(events)
        .into_iter()
        .filter_map(|action| match action {
            Action::AddMessagesToTask(add) => Some(add.messages),
            _ => None,
        })
        .flatten()
        .filter_map(|message| message.message)
        .collect()
}

#[test]
fn compact_is_stored_as_a_system_query() {
    let request = request_with_input_type(
        "task-1",
        api::request::input::Type::SummarizeConversation(
            api::request::input::SummarizeConversation {
                prompt: "focus on tests".to_string(),
            },
        ),
    );
    let stored = stored_messages(&Emitter::new(&request).start());

    let api::message::Message::SystemQuery(query) = &stored[0] else {
        panic!("expected a system query, got {:?}", stored[0]);
    };
    let Some(api::message::system_query::Type::SummarizeConversation(summarize)) = &query.r#type
    else {
        panic!("expected a summarize query");
    };
    assert_eq!(summarize.prompt, "focus on tests");
}

#[test]
fn a_summary_request_adds_an_unfinished_summary_message() {
    let request = request_with_input_type(
        "task-1",
        api::request::input::Type::SummarizeConversation(Default::default()),
    );
    let stored = stored_messages(&Emitter::new(&request).start());

    let Some(api::message::Message::Summarization(summary)) = stored.get(1) else {
        panic!("expected a summarization after the query, got {stored:?}");
    };
    assert!(summary.finished_duration.is_none());
}

#[test]
fn summary_text_is_appended_to_the_summary_and_no_agent_message_is_made() {
    let request = request_with_input_type(
        "task-1",
        api::request::input::Type::SummarizeConversation(Default::default()),
    );
    let mut emitter = Emitter::new(&request);
    let _ = emitter.start();

    let actions = actions_of(&emitter.on_delta(Delta::Text("We did X.".to_string())));
    let Action::AppendToMessageContent(append) = &actions[0] else {
        panic!("expected an append, got {:?}", actions[0]);
    };
    assert_eq!(
        append.mask.as_ref().expect("mask").paths,
        vec!["summarization.conversation_summary.summary".to_string()]
    );
    let Some(api::message::Message::Summarization(summary)) =
        append.message.as_ref().and_then(|m| m.message.as_ref())
    else {
        panic!("expected a summarization");
    };
    let Some(api::message::summarization::SummaryType::ConversationSummary(text)) =
        &summary.summary_type
    else {
        panic!("expected a conversation summary");
    };
    assert_eq!(text.summary, "We did X.");
}

#[test]
fn finishing_a_summary_sets_its_duration_and_drops_tool_calls() {
    let request = request_with_input_type(
        "task-1",
        api::request::input::Type::SummarizeConversation(Default::default()),
    );
    let mut emitter = Emitter::new(&request);
    let _ = emitter.start();
    let _ = emitter.on_delta(Delta::ToolCallStart {
        index: 0,
        id: "call-1".to_string(),
        name: "run_shell_command".to_string(),
    });
    let _ = emitter.on_delta(Delta::ToolCallArguments {
        index: 0,
        fragment: r#"{"command":"ls","is_read_only":true}"#.to_string(),
    });

    let actions = actions_of(&emitter.finish(StopReason::EndTurn));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::AddMessagesToTask(_))),
        "a summary carries no tool call: {actions:?}"
    );
    let Some(Action::UpdateTaskMessage(update)) = actions
        .iter()
        .find(|action| matches!(action, Action::UpdateTaskMessage(_)))
    else {
        panic!("expected the summary to be finished: {actions:?}");
    };
    assert_eq!(
        update.mask.as_ref().expect("mask").paths,
        vec!["summarization.finished_duration".to_string()]
    );
    let Some(api::message::Message::Summarization(summary)) =
        update.message.as_ref().and_then(|m| m.message.as_ref())
    else {
        panic!("expected a summarization");
    };
    assert!(summary.finished_duration.is_some());
    assert!(matches!(actions.last(), Some(Action::CommitTransaction(_))));
}

#[test]
fn a_normal_reply_makes_no_summary() {
    let mut emitter = Emitter::new(&request_with_query("task-1", "hi"));
    let _ = emitter.start();
    let actions = actions_of(&emitter.finish(StopReason::EndTurn));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::UpdateTaskMessage(_)))
    );
}

#[test]
fn a_skill_and_review_comments_are_stored_as_their_own_messages() {
    let skill = request_with_input_type(
        "task-1",
        api::request::input::Type::InvokeSkill(api::request::input::InvokeSkill {
            skill: Some(api::Skill::default()),
            user_query: Some(api::request::input::UserQuery {
                query: "to staging".to_string(),
                ..Default::default()
            }),
        }),
    );
    let stored = stored_messages(&Emitter::new(&skill).start());
    let api::message::Message::InvokeSkill(invoke) = &stored[0] else {
        panic!("expected a skill, got {:?}", stored[0]);
    };
    assert_eq!(
        invoke.user_query.as_ref().expect("query").query,
        "to staging"
    );

    let review = request_with_input_type(
        "task-1",
        api::request::input::Type::CodeReview(api::request::input::CodeReview {
            operation: Some(
                api::request::input::code_review::Operation::InitialReviewComments(
                    api::request::input::code_review::InitialReviewComments {
                        review_comments: vec![api::ReviewComment {
                            comment: "Rename.".to_string(),
                            ..Default::default()
                        }],
                        diff_set: None,
                    },
                ),
            ),
        }),
    );
    let stored = stored_messages(&Emitter::new(&review).start());
    let api::message::Message::CodeReview(code_review) = &stored[0] else {
        panic!("expected a code review, got {:?}", stored[0]);
    };
    assert_eq!(
        code_review
            .comments
            .as_ref()
            .expect("comments")
            .pending_comments[0]
            .comment,
        "Rename."
    );
}

#[test]
fn init_is_stored_as_a_question_and_a_chip_as_its_own_text() {
    let init = request_with_input_type(
        "task-1",
        api::request::input::Type::InitProjectRules(Default::default()),
    );
    let stored = stored_messages(&Emitter::new(&init).start());
    let api::message::Message::UserQuery(query) = &stored[0] else {
        panic!("expected a question, got {:?}", stored[0]);
    };
    assert_eq!(query.query, inputs::INIT_PROJECT_RULES);

    let chip = request_with_input_type(
        "task-1",
        api::request::input::Type::QueryWithCannedResponse(
            api::request::input::QueryWithCannedResponse {
                query: "Install a package".to_string(),
                ..Default::default()
            },
        ),
    );
    let stored = stored_messages(&Emitter::new(&chip).start());
    let api::message::Message::UserQuery(query) = &stored[0] else {
        panic!("expected a question, got {:?}", stored[0]);
    };
    assert_eq!(query.query, "Install a package");
}
