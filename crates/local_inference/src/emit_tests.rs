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
