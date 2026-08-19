use warp_multi_agent_api as api;

use super::*;

fn message(inner: message::Message) -> api::Message {
    api::Message {
        message: Some(inner),
        ..Default::default()
    }
}

fn user_message(text: &str) -> api::Message {
    message(message::Message::UserQuery(message::UserQuery {
        query: text.to_string(),
        ..Default::default()
    }))
}

fn agent_message(text: &str) -> api::Message {
    message(message::Message::AgentOutput(message::AgentOutput {
        text: text.to_string(),
    }))
}

fn shell_call(id: &str, command: &str) -> api::Message {
    message(message::Message::ToolCall(message::ToolCall {
        tool_call_id: id.to_string(),
        tool: Some(tool_call::Tool::RunShellCommand(
            tool_call::RunShellCommand {
                command: command.to_string(),
                is_read_only: true,
                ..Default::default()
            },
        )),
    }))
}

fn shell_result(id: &str, output: &str, exit_code: i32) -> api::Message {
    message(message::Message::ToolCallResult(message::ToolCallResult {
        tool_call_id: id.to_string(),
        result: Some(tool_call_result::Result::RunShellCommand(
            api::RunShellCommandResult {
                result: Some(api::run_shell_command_result::Result::CommandFinished(
                    api::ShellCommandFinished {
                        output: output.to_string(),
                        exit_code,
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
        )),
        ..Default::default()
    }))
}

fn request_with_messages(messages: Vec<api::Message>) -> api::Request {
    api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: vec![api::Task {
                messages,
                ..Default::default()
            }],
        }),
        ..Default::default()
    }
}

#[test]
fn a_plain_exchange_becomes_two_turns() {
    let request = request_with_messages(vec![user_message("hello"), agent_message("hi there")]);

    let turns = turns_from_request(&request);
    assert_eq!(
        turns,
        vec![
            Turn::User("hello".to_string()),
            Turn::Assistant {
                text: "hi there".to_string(),
                tool_calls: Vec::new(),
            },
        ]
    );
}

#[test]
fn split_agent_output_is_joined_into_one_turn() {
    let request = request_with_messages(vec![
        user_message("hello"),
        agent_message("hi "),
        agent_message("there"),
    ]);

    let turns = turns_from_request(&request);
    assert_eq!(turns.len(), 2);
    assert_eq!(
        turns[1],
        Turn::Assistant {
            text: "hi there".to_string(),
            tool_calls: Vec::new(),
        }
    );
}

#[test]
fn a_tool_call_attaches_to_the_agent_turn_before_it() {
    let request = request_with_messages(vec![
        user_message("what is here"),
        agent_message("I will look."),
        shell_call("call-1", "ls"),
    ]);

    let turns = turns_from_request(&request);
    assert_eq!(turns.len(), 2);
    let Turn::Assistant { text, tool_calls } = &turns[1] else {
        panic!("expected an assistant turn, got {:?}", turns[1]);
    };
    assert_eq!(text, "I will look.");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].id, "call-1");
    assert_eq!(tool_calls[0].name, "run_shell_command");
    assert_eq!(tool_calls[0].arguments["command"], "ls");
}

#[test]
fn a_tool_call_with_no_text_still_makes_an_agent_turn() {
    let request = request_with_messages(vec![user_message("run ls"), shell_call("call-1", "ls")]);

    let turns = turns_from_request(&request);
    assert_eq!(turns.len(), 2);
    let Turn::Assistant { text, tool_calls } = &turns[1] else {
        panic!("expected an assistant turn, got {:?}", turns[1]);
    };
    assert!(text.is_empty());
    assert_eq!(tool_calls.len(), 1);
}

#[test]
fn parallel_tool_results_are_grouped_into_one_turn() {
    let request = request_with_messages(vec![
        user_message("look"),
        shell_call("call-1", "ls"),
        shell_call("call-2", "pwd"),
        shell_result("call-1", "a.txt", 0),
        shell_result("call-2", "/home", 0),
    ]);

    let turns = turns_from_request(&request);
    assert_eq!(turns.len(), 3);
    let Turn::ToolResults(results) = &turns[2] else {
        panic!("expected a tool-result turn, got {:?}", turns[2]);
    };
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, "call-1");
    assert!(results[0].content.contains("a.txt"));
    assert!(!results[0].is_error);
}

#[test]
fn a_non_zero_exit_code_marks_the_result_as_an_error() {
    let request = request_with_messages(vec![
        user_message("build"),
        shell_call("call-1", "make"),
        shell_result("call-1", "no rule to make target", 2),
    ]);

    let turns = turns_from_request(&request);
    let Turn::ToolResults(results) = turns.last().expect("expected a turn") else {
        panic!("expected a tool-result turn");
    };
    assert!(results[0].is_error);
    assert!(results[0].content.starts_with("exit code: 2"));
}

#[test]
fn the_new_input_is_appended_after_the_history() {
    use api::request::input::user_inputs::{UserInput, user_input::Input};

    let mut request = request_with_messages(vec![user_message("first"), agent_message("ok")]);
    request.input = Some(api::request::Input {
        r#type: Some(api::request::input::Type::UserInputs(
            api::request::input::UserInputs {
                inputs: vec![UserInput {
                    input: Some(Input::UserQuery(api::request::input::UserQuery {
                        query: "second".to_string(),
                        ..Default::default()
                    })),
                }],
            },
        )),
        ..Default::default()
    });

    let turns = turns_from_request(&request);
    assert_eq!(turns.len(), 3);
    assert_eq!(turns[2], Turn::User("second".to_string()));
}

#[test]
fn an_unsupported_tool_call_is_left_out() {
    let call = message(message::Message::ToolCall(message::ToolCall {
        tool_call_id: "call-1".to_string(),
        tool: Some(tool_call::Tool::UseComputer(Default::default())),
    }));
    let request = request_with_messages(vec![user_message("go"), call]);

    let turns = turns_from_request(&request);
    assert_eq!(turns, vec![Turn::User("go".to_string())]);
}

#[test]
fn a_long_result_keeps_its_tail() {
    let long = "x".repeat(MAX_RESULT_BYTES + 500);
    let truncated = truncate(&long);

    assert!(truncated.len() < long.len());
    assert!(truncated.starts_with('['));
    assert!(truncated.ends_with('x'));
}

#[test]
fn truncation_never_splits_a_character() {
    // Each `é` is two bytes, so a byte-wise cut can land inside one.
    let long = "é".repeat(MAX_RESULT_BYTES);
    let truncated = truncate(&long);

    assert!(truncated.is_char_boundary(truncated.len()));
    assert!(truncated.contains('é'));
}
