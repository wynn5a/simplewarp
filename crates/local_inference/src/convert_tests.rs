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
                reasoning: String::new(),
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
            reasoning: String::new(),
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
    assert_eq!(
        turns.len(),
        3,
        "an unanswered call gains a placeholder result: {turns:?}"
    );
    let Turn::Assistant {
        text, tool_calls, ..
    } = &turns[1]
    else {
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
    assert_eq!(
        turns.len(),
        3,
        "an unanswered call gains a placeholder result: {turns:?}"
    );
    let Turn::Assistant {
        text, tool_calls, ..
    } = &turns[1]
    else {
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
    use api::request::input::user_inputs::UserInput;
    use api::request::input::user_inputs::user_input::Input;

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

fn user_inputs_request(
    query: &str,
    referenced: std::collections::HashMap<String, api::Attachment>,
    context: Option<api::InputContext>,
) -> api::Request {
    use api::request::input::user_inputs::UserInput;
    use api::request::input::user_inputs::user_input::Input;

    api::Request {
        input: Some(api::request::Input {
            context,
            r#type: Some(api::request::input::Type::UserInputs(
                api::request::input::UserInputs {
                    inputs: vec![UserInput {
                        input: Some(Input::UserQuery(api::request::input::UserQuery {
                            query: query.to_string(),
                            referenced_attachments: referenced,
                            ..Default::default()
                        })),
                    }],
                },
            )),
        }),
        ..Default::default()
    }
}

#[test]
fn attachments_follow_the_query_in_the_same_user_turn() {
    let referenced = std::collections::HashMap::from([(
        "block".to_string(),
        api::Attachment {
            value: Some(api::attachment::Value::PlainText("the output".to_string())),
        },
    )]);
    let mut context = api::InputContext::default();
    context
        .selected_text
        .push(api::input_context::SelectedText {
            text: "picked".to_string(),
        });
    let request = user_inputs_request("why did it fail?", referenced, Some(context));

    let turns = turns_from_request(&request);
    assert_eq!(
        turns.len(),
        1,
        "the attachments join the user turn: {turns:?}"
    );
    let Turn::User(text) = &turns[0] else {
        panic!("expected a user turn");
    };
    assert!(text.starts_with("why did it fail?"));
    assert!(text.contains("Attachment `block`"));
    assert!(text.contains("the output"));
    assert!(text.contains("picked"));
}

#[test]
fn a_tool_result_input_carries_no_attachments() {
    use api::request::input::user_inputs::UserInput;
    use api::request::input::user_inputs::user_input::Input;

    let mut context = api::InputContext::default();
    context
        .selected_text
        .push(api::input_context::SelectedText {
            text: "stale selection".to_string(),
        });
    let mut request = request_with_messages(vec![user_message("go"), shell_call("call-1", "ls")]);
    request.input = Some(api::request::Input {
        context: Some(context),
        r#type: Some(api::request::input::Type::UserInputs(
            api::request::input::UserInputs {
                inputs: vec![UserInput {
                    input: Some(Input::ToolCallResult(input_result(
                        api::request::input::tool_call_result::Result::RunShellCommand(
                            Default::default(),
                        ),
                    ))),
                }],
            },
        )),
    });

    let turns = turns_from_request(&request);
    assert!(
        !turns
            .iter()
            .any(|turn| matches!(turn, Turn::User(text) if text.contains("stale"))),
        "{turns:?}"
    );
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

/// A text of `len` characters whose start and end can be told apart from its middle.
fn marked_text(len: usize) -> String {
    format!("HEAD{}TAIL", "x".repeat(len - 8))
}

#[test]
fn a_short_result_is_not_truncated() {
    let text = marked_text(MAX_RESULT_CHARS);
    assert_eq!(truncate(&text), text);
}

#[test]
fn a_long_result_keeps_its_head_and_tail() {
    let long = marked_text(MAX_RESULT_CHARS * 3);
    let truncated = truncate(&long);

    assert!(truncated.starts_with("HEAD"));
    assert!(truncated.ends_with("TAIL"));
    assert!(truncated.contains("characters were cut from the middle"));
    assert!(truncated.chars().count() <= MAX_RESULT_CHARS);
}

/// A stored result is cut once when it is stored and again when it is rendered, so cutting must
/// not change what was already cut.
#[test]
fn truncation_is_idempotent() {
    let once = truncate(&marked_text(MAX_RESULT_CHARS + 1));
    assert_eq!(truncate(&once), once);
}

#[test]
fn truncation_never_splits_a_character() {
    // Each `é` is two bytes, so a byte-wise cut can land inside one.
    let long = "é".repeat(MAX_RESULT_CHARS * 2);
    let truncated = truncate(&long);

    assert!(truncated.starts_with('é'));
    assert!(truncated.ends_with('é'));
    assert!(truncated.chars().count() <= MAX_RESULT_CHARS);
}

fn history_result(result: tool_call_result::Result) -> message::ToolCallResult {
    message::ToolCallResult {
        tool_call_id: "call-1".to_string(),
        result: Some(result),
        ..Default::default()
    }
}

fn input_result(
    result: api::request::input::tool_call_result::Result,
) -> api::request::input::ToolCallResult {
    api::request::input::ToolCallResult {
        tool_call_id: "call-1".to_string(),
        result: Some(result),
    }
}

fn snapshot_result(command_id: &str, output: &str) -> api::LongRunningShellCommandSnapshot {
    api::LongRunningShellCommandSnapshot {
        command_id: command_id.to_string(),
        output: output.to_string(),
        ..Default::default()
    }
}

#[test]
fn a_snapshot_names_the_command_id_so_the_model_can_poll() {
    let rendered = render_result(&history_result(tool_call_result::Result::RunShellCommand(
        api::RunShellCommandResult {
            result: Some(
                api::run_shell_command_result::Result::LongRunningCommandSnapshot(snapshot_result(
                    "block-9", "loading",
                )),
            ),
            ..Default::default()
        },
    )));

    assert!(!rendered.is_error);
    assert!(rendered.content.contains("block-9"), "{}", rendered.content);
    assert!(rendered.content.contains("still running"));
    assert!(rendered.content.contains("loading"));
}

#[test]
fn a_poll_of_a_running_command_carries_its_command_id() {
    let rendered = render_input_result(&input_result(
        api::request::input::tool_call_result::Result::ReadShellCommandOutput(
            api::ReadShellCommandOutputResult {
                result: Some(
                    api::read_shell_command_output_result::Result::LongRunningCommandSnapshot(
                        snapshot_result("block-9", "still installing"),
                    ),
                ),
                ..Default::default()
            },
        ),
    ));

    assert!(!rendered.is_error);
    assert!(rendered.content.contains("block-9"), "{}", rendered.content);
    assert!(rendered.content.contains("still installing"));
}

#[test]
fn a_finished_poll_reads_like_a_finished_command() {
    let rendered = render_input_result(&input_result(
        api::request::input::tool_call_result::Result::ReadShellCommandOutput(
            api::ReadShellCommandOutputResult {
                result: Some(
                    api::read_shell_command_output_result::Result::CommandFinished(
                        api::ShellCommandFinished {
                            output: "added 1 package".to_string(),
                            exit_code: 0,
                            ..Default::default()
                        },
                    ),
                ),
                ..Default::default()
            },
        ),
    ));

    assert!(!rendered.is_error);
    assert!(rendered.content.contains("exit code: 0"));
    assert!(rendered.content.contains("added 1 package"));
}

#[test]
fn a_poll_for_an_unknown_command_is_an_error() {
    let rendered = render_input_result(&input_result(
        api::request::input::tool_call_result::Result::ReadShellCommandOutput(
            api::ReadShellCommandOutputResult {
                result: Some(api::read_shell_command_output_result::Result::Error(
                    api::ShellCommandError::default(),
                )),
                ..Default::default()
            },
        ),
    ));

    assert!(rendered.is_error);
}

#[test]
fn a_write_to_a_running_command_reports_the_output() {
    let rendered = render_input_result(&input_result(
        api::request::input::tool_call_result::Result::WriteToLongRunningShellCommand(
            api::WriteToLongRunningShellCommandResult {
                result: Some(
                    api::write_to_long_running_shell_command_result::Result::LongRunningCommandSnapshot(
                        snapshot_result("block-9", "proceed? y"),
                    ),
                ),
            },
        ),
    ));

    assert!(!rendered.is_error);
    assert!(rendered.content.contains("block-9"), "{}", rendered.content);
    assert!(rendered.content.contains("proceed? y"));
}

#[test]
fn a_denied_command_tells_the_model_how_to_recover() {
    let rendered = render_result(&history_result(tool_call_result::Result::RunShellCommand(
        api::RunShellCommandResult {
            result: Some(api::run_shell_command_result::Result::PermissionDenied(
                api::PermissionDenied::default(),
            )),
            ..Default::default()
        },
    )));

    assert!(rendered.is_error);
    assert!(
        rendered.content.contains("read_shell_command_output"),
        "{}",
        rendered.content
    );
}

#[test]
fn poll_and_write_calls_replay_with_their_arguments() {
    let poll_call = message::ToolCall {
        tool_call_id: "call-1".to_string(),
        tool: Some(tool_call::Tool::ReadShellCommandOutput(
            tool_call::ReadShellCommandOutput {
                command_id: "block-9".to_string(),
                delay: Some(tool_call::read_shell_command_output::Delay::Duration(
                    prost_types::Duration {
                        seconds: 30,
                        nanos: 0,
                    },
                )),
            },
        )),
    };
    let ToolUse {
        name, arguments, ..
    } = tool_use_from_proto(&poll_call).expect("a known tool");
    assert_eq!(name, "read_shell_command_output");
    assert_eq!(arguments["command_id"], "block-9");
    assert_eq!(arguments["max_wait_seconds"], 30);

    let write_call = message::ToolCall {
        tool_call_id: "call-2".to_string(),
        tool: Some(tool_call::Tool::WriteToLongRunningShellCommand(
            tool_call::WriteToLongRunningShellCommand {
                command_id: "block-9".to_string(),
                input: b"y\n".to_vec(),
                mode: Some(tool_call::write_to_long_running_shell_command::Mode {
                    mode: Some(
                        tool_call::write_to_long_running_shell_command::mode::Mode::Line(()),
                    ),
                }),
            },
        )),
    };
    let ToolUse {
        name, arguments, ..
    } = tool_use_from_proto(&write_call).expect("a known tool");
    assert_eq!(name, "write_to_long_running_shell_command");
    assert_eq!(arguments["command_id"], "block-9");
    assert_eq!(arguments["input"], "y\n");
    assert_eq!(arguments["mode"], "line");
}

fn reasoning_message(text: &str) -> api::Message {
    message(message::Message::AgentReasoning(message::AgentReasoning {
        reasoning: text.to_string(),
        ..Default::default()
    }))
}

#[test]
fn reasoning_joins_the_reply_it_belongs_to() {
    // The emitter writes the reasoning message before the text and the tool calls of the same
    // reply, so all three must land on one assistant turn.
    let request = request_with_messages(vec![
        user_message("what is here"),
        reasoning_message("The user wants a listing."),
        agent_message("I will look."),
        shell_call("call-1", "ls"),
    ]);

    let turns = turns_from_request(&request);
    assert_eq!(
        turns.len(),
        3,
        "an unanswered call gains a placeholder result: {turns:?}"
    );
    let Turn::Assistant {
        text,
        reasoning,
        tool_calls,
    } = &turns[1]
    else {
        panic!("expected an assistant turn, got {:?}", turns[1]);
    };
    assert_eq!(reasoning, "The user wants a listing.");
    assert_eq!(text, "I will look.");
    assert_eq!(tool_calls.len(), 1);
}

#[test]
fn split_reasoning_is_joined_into_one_turn() {
    let request = request_with_messages(vec![
        user_message("hello"),
        reasoning_message("first "),
        reasoning_message("second"),
    ]);

    let turns = turns_from_request(&request);
    assert_eq!(turns.len(), 2);
    let Turn::Assistant { reasoning, .. } = &turns[1] else {
        panic!("expected an assistant turn, got {:?}", turns[1]);
    };
    assert_eq!(reasoning, "first second");
}

#[test]
fn reasoning_after_a_tool_result_opens_a_new_turn() {
    // The second reply must not have its thinking folded into the first one.
    let request = request_with_messages(vec![
        user_message("count the files"),
        shell_call("call-1", "ls"),
        shell_result("call-1", "a\nb\n", 0),
        reasoning_message("Two files."),
        agent_message("There are two."),
    ]);

    let turns = turns_from_request(&request);
    assert_eq!(turns.len(), 4, "got {turns:?}");
    let Turn::Assistant { reasoning, .. } = &turns[3] else {
        panic!("expected an assistant turn, got {:?}", turns[3]);
    };
    assert_eq!(reasoning, "Two files.");
}

/// The shape that the client actually replays. `agent_tasks` rows hold the reasoning, the call,
/// and the agent text, and never a result, so an unrepaired turn list puts two assistant turns
/// next to each other and the provider answers 400.
#[test]
fn a_replayed_history_with_no_tool_result_still_pairs_up() {
    let request = request_with_messages(vec![
        reasoning_message("I will count the files."),
        shell_call("call-1", "find . -name '*.rs' | wc -l"),
        agent_message("There are 4,053 .rs files."),
        user_message("what do you think of this project"),
    ]);

    let turns = turns_from_request(&request);

    // assistant(call) → results → assistant(text) → user
    assert_eq!(turns.len(), 4, "got {turns:?}");
    let Turn::Assistant { tool_calls, .. } = &turns[0] else {
        panic!("expected an assistant turn, got {:?}", turns[0]);
    };
    assert_eq!(tool_calls.len(), 1);
    let Turn::ToolResults(results) = &turns[1] else {
        panic!("expected a placeholder result, got {:?}", turns[1]);
    };
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "call-1");
    assert!(!results[0].is_error);
    assert!(matches!(&turns[2], Turn::Assistant { text, .. } if text.contains("4,053")));
    assert!(matches!(&turns[3], Turn::User(_)));
}

#[test]
fn a_real_tool_result_is_not_replaced_by_a_placeholder() {
    let request = request_with_messages(vec![
        user_message("count the files"),
        shell_call("call-1", "ls"),
        shell_result("call-1", "a\nb\n", 0),
        agent_message("Two files."),
    ]);

    let turns = turns_from_request(&request);

    let Turn::ToolResults(results) = &turns[2] else {
        panic!("expected the real result, got {:?}", turns[2]);
    };
    assert_eq!(results.len(), 1, "no placeholder may be added");
    assert!(
        results[0].content.contains("exit code"),
        "the real output must survive: {:?}",
        results[0].content
    );
}

#[test]
fn only_the_unanswered_call_of_a_parallel_pair_gets_a_placeholder() {
    let request = request_with_messages(vec![
        user_message("look around"),
        shell_call("call-1", "ls"),
        shell_call("call-2", "pwd"),
        shell_result("call-1", "a\n", 0),
    ]);

    let turns = turns_from_request(&request);

    let Turn::ToolResults(results) = &turns[2] else {
        panic!("expected results, got {:?}", turns[2]);
    };
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, "call-1");
    assert!(results[0].content.contains("exit code"));
    assert_eq!(results[1].id, "call-2");
    assert!(results[1].content.contains("not kept"));
}

#[test]
fn a_trailing_tool_call_gets_a_result_after_it() {
    // Nothing follows the call, so the placeholder has to be appended, not inserted.
    let request = request_with_messages(vec![user_message("run ls"), shell_call("call-1", "ls")]);

    let turns = turns_from_request(&request);

    assert_eq!(turns.len(), 3, "got {turns:?}");
    assert!(matches!(&turns[2], Turn::ToolResults(results) if results[0].id == "call-1"));
}

#[test]
fn a_reply_with_no_tool_calls_gains_nothing() {
    let request = request_with_messages(vec![user_message("hello"), agent_message("hi there")]);

    let turns = turns_from_request(&request);
    assert_eq!(turns.len(), 2, "got {turns:?}");
}

fn request_with_input_type(
    history: Vec<api::Message>,
    r#type: api::request::input::Type,
) -> api::Request {
    let mut request = request_with_messages(history);
    request.input = Some(api::request::Input {
        r#type: Some(r#type),
        ..Default::default()
    });
    request
}

fn user_text(turns: &[Turn]) -> String {
    turns
        .iter()
        .filter_map(|turn| match turn {
            Turn::User(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n---\n")
}

#[test]
fn compact_becomes_the_summarize_prompt() {
    let request = request_with_input_type(
        vec![user_message("first"), agent_message("ok")],
        api::request::input::Type::SummarizeConversation(
            api::request::input::SummarizeConversation {
                prompt: "the tests".to_string(),
                ..Default::default()
            },
        ),
    );
    let turns = turns_from_request(&request);
    assert_eq!(turns.len(), 3);
    let Turn::User(text) = &turns[2] else {
        panic!("expected a user turn");
    };
    assert!(text.contains("Summarize this conversation"));
    assert!(text.ends_with("the tests"));
}

#[test]
fn init_resume_clone_and_new_project_become_prompts() {
    use api::request::input::Type;

    let init = turns_from_request(&request_with_input_type(
        vec![],
        Type::InitProjectRules(Default::default()),
    ));
    assert!(user_text(&init).contains("AGENTS.md"));

    let resume = turns_from_request(&request_with_input_type(
        vec![user_message("go"), agent_message("partial")],
        Type::ResumeConversation(Default::default()),
    ));
    assert!(user_text(&resume).contains("Continue from where you left off"));

    let clone = turns_from_request(&request_with_input_type(
        vec![],
        Type::CloneRepository(api::request::input::CloneRepository {
            url: "https://example.com/a.git".to_string(),
        }),
    ));
    assert!(user_text(&clone).contains("https://example.com/a.git"));

    let project = turns_from_request(&request_with_input_type(
        vec![],
        Type::CreateNewProject(api::request::input::CreateNewProject {
            query: "a todo app".to_string(),
        }),
    ));
    assert!(user_text(&project).contains("a todo app"));
}

#[test]
fn inputs_with_no_text_here_add_no_turn() {
    use api::request::input::Type;

    for input in [
        Type::CreateEnvironment(Default::default()),
        Type::GeneratePassiveSuggestions(Default::default()),
        Type::AutoCodeDiffQuery(Default::default()),
    ] {
        let request = request_with_input_type(vec![], input);
        assert!(turns_from_request(&request).is_empty());
    }
}

fn summary_message(text: &str, finished: bool) -> api::Message {
    message(message::Message::Summarization(message::Summarization {
        finished_duration: finished.then_some(prost_types::Duration {
            seconds: 3,
            nanos: 0,
        }),
        summary_type: Some(message::summarization::SummaryType::ConversationSummary(
            message::summarization::ConversationSummary {
                summary: text.to_string(),
                ..Default::default()
            },
        )),
    }))
}

#[test]
fn a_finished_summary_replaces_everything_before_it() {
    let history = vec![
        user_message("old question"),
        agent_message("old answer"),
        summary_message("We fixed the build.", true),
        user_message("next question"),
    ];
    let turns = turns_from_request(&request_with_messages(history));

    assert_eq!(turns.len(), 1, "{turns:?}");
    let Turn::User(text) = &turns[0] else {
        panic!("expected a user turn");
    };
    assert!(text.contains("We fixed the build."));
    assert!(text.ends_with("next question"));
    assert!(!text.contains("old question"));
}

fn with_id(id: &str, mut proto_message: api::Message) -> api::Message {
    proto_message.id = id.to_string();
    proto_message
}

fn compacted_summary(text: &str, first_kept: &str) -> api::Message {
    let mut summary = summary_message(text, true);
    if let Some(message::Message::Summarization(summarization)) = summary.message.as_mut()
        && let Some(message::summarization::SummaryType::ConversationSummary(conversation)) =
            summarization.summary_type.as_mut()
    {
        conversation.first_kept_message_id = first_kept.to_string();
    }
    summary
}

#[test]
fn a_summary_with_a_first_kept_message_keeps_the_tail() {
    let history = vec![
        with_id("q1", user_message("old question")),
        with_id("a1", agent_message("old answer")),
        with_id("q2", user_message("go on")),
        with_id("c2", shell_call("call-2", "ls")),
        with_id("r2", shell_result("call-2", "file.txt", 0)),
        with_id("a3", agent_message("kept answer")),
        compacted_summary("We read the files.", "c2"),
        with_id("q4", user_message("next question")),
    ];
    let turns = turns_from_request(&request_with_messages(history));

    assert_eq!(turns.len(), 5, "{turns:?}");
    let Turn::User(summary) = &turns[0] else {
        panic!("expected the summary first, got {turns:?}");
    };
    assert!(summary.contains("We read the files."));
    assert!(!user_text(&turns).contains("old question"));
    assert!(!user_text(&turns).contains("go on"));
    assert!(
        matches!(&turns[1], Turn::Assistant { tool_calls, .. } if tool_calls[0].id == "call-2")
    );
    assert!(matches!(&turns[2], Turn::ToolResults(results) if results[0].id == "call-2"));
    assert!(matches!(&turns[3], Turn::Assistant { text, .. } if text == "kept answer"));
    assert_eq!(turns[4], Turn::User("next question".to_string()));
}

#[test]
fn a_kept_tail_that_opens_with_the_user_absorbs_the_summary() {
    let history = vec![
        with_id("q1", user_message("old question")),
        with_id("a1", agent_message("old answer")),
        with_id("q2", user_message("kept question")),
        with_id("a2", agent_message("kept answer")),
        compacted_summary("Summary text.", "q2"),
    ];
    let turns = turns_from_request(&request_with_messages(history));

    assert_eq!(turns.len(), 2, "{turns:?}");
    let Turn::User(text) = &turns[0] else {
        panic!("expected a user turn");
    };
    assert!(text.contains("Summary text."));
    assert!(text.ends_with("kept question"));
}

#[test]
fn an_unknown_first_kept_message_falls_back_to_replacing_everything() {
    let history = vec![
        with_id("q1", user_message("old question")),
        with_id("a1", agent_message("old answer")),
        compacted_summary("Summary text.", "missing"),
        with_id("q2", user_message("new question")),
    ];
    let turns = turns_from_request(&request_with_messages(history));

    assert_eq!(turns.len(), 1, "{turns:?}");
    assert!(!user_text(&turns).contains("old question"));
}

#[test]
fn a_second_summary_replaces_the_first() {
    let history = vec![
        with_id("q1", user_message("first question")),
        with_id("a1", agent_message("first answer")),
        with_id("q2", user_message("second question")),
        with_id("a2", agent_message("second answer")),
        compacted_summary("First summary.", "q2"),
        with_id("q3", user_message("third question")),
        with_id("a3", agent_message("third answer")),
        compacted_summary("Second summary.", "q3"),
    ];
    let turns = turns_from_request(&request_with_messages(history));

    let text = user_text(&turns);
    assert!(text.contains("Second summary."));
    assert!(!text.contains("First summary."));
    assert!(!text.contains("second question"));
    assert!(text.contains("third question"));
    assert_eq!(turns.len(), 2, "{turns:?}");
}

#[test]
fn the_stored_compact_instruction_is_not_replayed() {
    let instruction = message(message::Message::SystemQuery(message::SystemQuery {
        r#type: Some(message::system_query::Type::SummarizeConversation(
            message::SummarizeConversation {
                prompt: "focus".to_string(),
            },
        )),
        ..Default::default()
    }));
    let turns = turns_from_request(&request_with_messages(vec![
        user_message("question"),
        agent_message("answer"),
        instruction,
        summary_message("unfinished", false),
    ]));

    assert!(!user_text(&turns).contains("Summarize this conversation"));
    assert_eq!(turns.len(), 2, "{turns:?}");
}

#[test]
fn a_result_whose_call_was_summarized_becomes_user_text() {
    let history = vec![
        with_id("q1", user_message("question")),
        with_id("c1", shell_call("call-1", "ls")),
        compacted_summary("We ran ls.", ""),
        with_id("r1", shell_result("call-1", "file.txt", 0)),
    ];
    let turns = turns_from_request(&request_with_messages(history));

    assert!(
        turns
            .iter()
            .all(|turn| !matches!(turn, Turn::ToolResults(_))),
        "{turns:?}"
    );
    let text = user_text(&turns);
    assert!(text.contains("Result of a tool call made before the summary (call-1)"));
    assert!(text.contains("file.txt"));
}

#[test]
fn an_unfinished_or_empty_summary_drops_nothing() {
    for summary in [
        summary_message("partial", false),
        summary_message("  ", true),
    ] {
        let history = vec![user_message("keep me"), agent_message("ok"), summary];
        let turns = turns_from_request(&request_with_messages(history));
        assert!(user_text(&turns).contains("keep me"), "{turns:?}");
    }
}

#[test]
fn stored_inputs_replay_as_the_text_the_input_produced() {
    let skill = message(message::Message::InvokeSkill(message::InvokeSkill {
        skill: Some(api::Skill {
            descriptor: Some(api::SkillDescriptor {
                name: "deploy".to_string(),
                ..Default::default()
            }),
            content: Some(api::FileContent {
                content: "Run ./deploy.sh".to_string(),
                ..Default::default()
            }),
        }),
        user_query: None,
    }));
    let compact = message(message::Message::SystemQuery(message::SystemQuery {
        r#type: Some(message::system_query::Type::ResumeConversation(
            message::ResumeConversation {},
        )),
        ..Default::default()
    }));
    let turns = turns_from_request(&request_with_messages(vec![
        skill,
        agent_message("done"),
        compact,
    ]));
    let text = user_text(&turns);
    assert!(text.contains("Run ./deploy.sh"));
    assert!(text.contains("Continue from where you left off"));
}

#[test]
fn an_mcp_call_in_the_history_keeps_the_name_the_model_gave_it() {
    let call = message::ToolCall {
        tool_call_id: "call-1".to_string(),
        tool: Some(tool_call::Tool::CallMcpTool(tool_call::CallMcpTool {
            name: "search".to_string(),
            args: Some(mcp::json_to_struct(&json!({ "query": "bug" }))),
            server_id: "3f9c2a10-1b2c".to_string(),
        })),
    };
    let used = tool_use_from_proto(&call).expect("an MCP call is kept");

    assert_eq!(used.name, mcp::wire_name("3f9c2a10-1b2c", "search"));
    assert_eq!(used.arguments, json!({ "query": "bug" }));
}

#[test]
fn a_resource_read_in_the_history_is_kept() {
    let call = message::ToolCall {
        tool_call_id: "call-1".to_string(),
        tool: Some(tool_call::Tool::ReadMcpResource(
            tool_call::ReadMcpResource {
                uri: "file:///a".to_string(),
                server_id: String::new(),
            },
        )),
    };
    let used = tool_use_from_proto(&call).expect("a resource read is kept");

    assert_eq!(used.name, mcp::READ_RESOURCE_TOOL);
    assert_eq!(used.arguments, json!({ "uri": "file:///a" }));
}

#[test]
fn an_mcp_tool_result_shows_its_text_and_names_what_it_cannot_show() {
    use api::call_mcp_tool_result::success::result::{Image, Result as Item, Text};
    use api::call_mcp_tool_result::{Result as CallResult, Success};

    let item = |inner| api::call_mcp_tool_result::success::Result {
        result: Some(inner),
    };
    let result = api::CallMcpToolResult {
        result: Some(CallResult::Success(Success {
            results: vec![
                item(Item::Text(Text {
                    text: "found 2".to_string(),
                })),
                item(Item::Image(Image {
                    data: vec![1, 2, 3],
                    mime_type: "image/png".to_string(),
                })),
            ],
        })),
    };
    let (content, is_error) = render_call_mcp(&result);

    assert_eq!(content, "found 2\n[image: image/png]");
    assert!(!is_error);
}

#[test]
fn an_mcp_tool_error_is_an_error_result() {
    use api::call_mcp_tool_result::{Error, Result as CallResult};

    let (content, is_error) = render_call_mcp(&api::CallMcpToolResult {
        result: Some(CallResult::Error(Error {
            message: "boom".to_string(),
        })),
    });

    assert_eq!(content, "boom");
    assert!(is_error);
}

#[test]
fn a_resource_read_shows_text_and_names_binary() {
    use api::mcp_resource_content::{Binary, ContentType, Text};
    use api::read_mcp_resource_result::{Result as ReadResult, Success};

    let result = api::ReadMcpResourceResult {
        result: Some(ReadResult::Success(Success {
            contents: vec![
                api::McpResourceContent {
                    uri: "file:///a.txt".to_string(),
                    content_type: Some(ContentType::Text(Text {
                        content: "hello".to_string(),
                        mime_type: "text/plain".to_string(),
                    })),
                },
                api::McpResourceContent {
                    uri: "file:///b.bin".to_string(),
                    content_type: Some(ContentType::Binary(Binary {
                        data: vec![0],
                        mime_type: "application/octet-stream".to_string(),
                    })),
                },
            ],
        })),
    };
    let (content, is_error) = render_read_mcp(&result);

    assert_eq!(
        content,
        "hello\n[binary resource file:///b.bin: application/octet-stream]"
    );
    assert!(!is_error);
}

#[test]
fn a_transfer_result_says_the_user_handed_control_back() {
    let rendered = render_input_result(&input_result(
        api::request::input::tool_call_result::Result::TransferShellCommandControlToUser(
            api::TransferShellCommandControlToUserResult {
                result: Some(
                    api::transfer_shell_command_control_to_user_result::Result::LongRunningCommandSnapshot(
                        snapshot_result("block-9", "logged in"),
                    ),
                ),
            },
        ),
    ));

    assert!(!rendered.is_error);
    assert!(
        rendered.content.contains("handed control back"),
        "{}",
        rendered.content
    );
    assert!(rendered.content.contains("block-9"));
    assert!(rendered.content.contains("logged in"));
}

#[test]
fn an_inserted_review_comments_result_names_the_repo() {
    let rendered = render_result(&history_result(
        tool_call_result::Result::InsertReviewComments(api::InsertReviewCommentsResult {
            repo_path: "/repo".to_string(),
            result: Some(api::insert_review_comments_result::Result::Success(
                Default::default(),
            )),
        }),
    ));

    assert!(!rendered.is_error);
    assert!(rendered.content.contains("/repo"), "{}", rendered.content);
}

#[test]
fn transfer_and_review_comment_calls_replay_through_the_model_arguments() {
    for arguments in [
        json!({ "reason": "Enter your password" }),
        json!({
            "repo_path": "/repo",
            "base_branch": "main",
            "comments": [{
                "id": "c1",
                "author": "reviewer",
                "body": "Rename this.",
                "updated_at": "2026-10-01T12:00:00Z",
                "path": "src/lib.rs",
                "diff_hunk": "@@ -1 +1 @@",
                "start_line": 4,
                "line": 4,
                "side": "RIGHT",
            }],
        }),
    ] {
        let name = if arguments.get("reason").is_some() {
            "transfer_control_to_user"
        } else {
            "insert_review_comments"
        };
        let call = crate::tools::to_proto("call-1", name, &arguments).expect("a known tool");
        let replayed = tool_use_from_proto(&call).expect("a replayable tool");
        assert_eq!(replayed.name, name);
        assert_eq!(
            crate::tools::to_proto("call-1", &replayed.name, &replayed.arguments),
            Some(call)
        );
    }
}

fn shell_input(id: &str, output: &str) -> api::request::input::ToolCallResult {
    api::request::input::ToolCallResult {
        tool_call_id: id.to_string(),
        result: Some(
            api::request::input::tool_call_result::Result::RunShellCommand(
                api::RunShellCommandResult {
                    result: Some(api::run_shell_command_result::Result::CommandFinished(
                        api::ShellCommandFinished {
                            output: output.to_string(),
                            ..Default::default()
                        },
                    )),
                    ..Default::default()
                },
            ),
        ),
    }
}

/// A read of `path`, in the form that the client sends every read in.
fn read_input(id: &str, path: &str, content: &str) -> api::request::input::ToolCallResult {
    use api::any_file_content::Content;

    api::request::input::ToolCallResult {
        tool_call_id: id.to_string(),
        result: Some(api::request::input::tool_call_result::Result::ReadFiles(
            api::ReadFilesResult {
                result: Some(api::read_files_result::Result::AnyFilesSuccess(
                    api::read_files_result::AnyFilesSuccess {
                        files: vec![
                            api::AnyFileContent {
                                content: Some(Content::TextContent(api::FileContent {
                                    file_path: path.to_string(),
                                    content: content.to_string(),
                                    line_range: None,
                                })),
                            },
                            api::AnyFileContent {
                                content: Some(Content::BinaryContent(api::BinaryFileContent {
                                    file_path: "logo.png".to_string(),
                                    data: vec![0; 64],
                                })),
                            },
                        ],
                        failed_reads: Vec::new(),
                    },
                )),
            },
        )),
    }
}

fn mcp_input(text: &str) -> api::request::input::ToolCallResult {
    use api::call_mcp_tool_result::success::result::{Image, Result as Item, Text};
    use api::call_mcp_tool_result::{Result as CallResult, Success};

    let item = |inner| api::call_mcp_tool_result::success::Result {
        result: Some(inner),
    };
    input_result(api::request::input::tool_call_result::Result::CallMcpTool(
        api::CallMcpToolResult {
            result: Some(CallResult::Success(Success {
                results: vec![
                    item(Item::Text(Text {
                        text: text.to_string(),
                    })),
                    item(Item::Image(Image {
                        data: vec![1; 64],
                        mime_type: "image/png".to_string(),
                    })),
                ],
            })),
        },
    ))
}

#[test]
fn a_file_read_shows_its_text_content() {
    let rendered = render_input_result(&read_input("call-1", "Cargo.toml", "[workspace]"));

    assert!(!rendered.is_error);
    assert!(rendered.content.contains("--- Cargo.toml ---\n[workspace]"));
    assert!(
        rendered
            .content
            .contains("--- logo.png ---\nThe file is not text"),
        "{}",
        rendered.content
    );
}

#[test]
fn storing_a_result_truncates_what_the_model_sees_and_drops_what_it_does_not() {
    use api::any_file_content::Content;
    use api::call_mcp_tool_result::success::result::Result as Item;

    let long = marked_text(MAX_RESULT_CHARS * 2);

    let Some(tool_call_result::Result::RunShellCommand(shell)) =
        stored_result(&shell_input("call-1", &long)).result
    else {
        panic!("expected a shell result");
    };
    let Some(api::run_shell_command_result::Result::CommandFinished(finished)) = shell.result
    else {
        panic!("expected a finished command");
    };
    assert_eq!(finished.output, truncate(&long));

    let Some(tool_call_result::Result::ReadFiles(read)) =
        stored_result(&read_input("call-1", "a.rs", &long)).result
    else {
        panic!("expected a read result");
    };
    let Some(api::read_files_result::Result::AnyFilesSuccess(success)) = read.result else {
        panic!("expected a successful read");
    };
    let Some(Content::TextContent(text)) = success.files[0].content.as_ref() else {
        panic!("expected a text file");
    };
    assert_eq!(text.content, truncate(&long));
    let Some(Content::BinaryContent(binary)) = success.files[1].content.as_ref() else {
        panic!("expected a binary file");
    };
    assert!(binary.data.is_empty());
    assert_eq!(binary.file_path, "logo.png");

    let Some(tool_call_result::Result::CallMcpTool(call)) = stored_result(&mcp_input("hi")).result
    else {
        panic!("expected an MCP result");
    };
    let Some(api::call_mcp_tool_result::Result::Success(success)) = call.result else {
        panic!("expected a successful call");
    };
    let Some(Item::Image(image)) = success.results[1].result.as_ref() else {
        panic!("expected an image");
    };
    assert!(image.data.is_empty());
    assert_eq!(image.mime_type, "image/png");

    let edit = input_result(
        api::request::input::tool_call_result::Result::ApplyFileDiffs(api::ApplyFileDiffsResult {
            result: Some(api::apply_file_diffs_result::Result::Success(
                api::apply_file_diffs_result::Success {
                    updated_files_v2: vec![
                        api::apply_file_diffs_result::success::UpdatedFileContent {
                            file: Some(api::FileContent {
                                file_path: "a.rs".to_string(),
                                content: long.clone(),
                                line_range: None,
                            }),
                            was_edited_by_user: false,
                        },
                    ],
                    ..Default::default()
                },
            )),
        }),
    );
    let Some(tool_call_result::Result::ApplyFileDiffs(apply)) = stored_result(&edit).result else {
        panic!("expected an edit result");
    };
    let Some(api::apply_file_diffs_result::Result::Success(success)) = apply.result else {
        panic!("expected a successful edit");
    };
    let file = success.updated_files_v2[0].file.as_ref().expect("a file");
    assert_eq!(file.file_path, "a.rs");
    assert!(file.content.is_empty());
}

fn history_with_inputs(
    messages: Vec<api::Message>,
    inputs: Vec<api::request::input::user_inputs::user_input::Input>,
) -> api::Request {
    use api::request::input::user_inputs::UserInput;

    let mut request = request_with_messages(messages);
    request.input = Some(api::request::Input {
        r#type: Some(api::request::input::Type::UserInputs(
            api::request::input::UserInputs {
                inputs: inputs
                    .into_iter()
                    .map(|input| UserInput { input: Some(input) })
                    .collect(),
            },
        )),
        ..Default::default()
    });
    request
}

fn user_query_input(query: &str) -> api::request::input::user_inputs::user_input::Input {
    api::request::input::user_inputs::user_input::Input::UserQuery(api::request::input::UserQuery {
        query: query.to_string(),
        ..Default::default()
    })
}

fn tool_result_input(
    result: api::request::input::ToolCallResult,
) -> api::request::input::user_inputs::user_input::Input {
    api::request::input::user_inputs::user_input::Input::ToolCallResult(result)
}

fn read_call(id: &str, path: &str) -> api::Message {
    message(message::Message::ToolCall(message::ToolCall {
        tool_call_id: id.to_string(),
        tool: Some(tool_call::Tool::ReadFiles(tool_call::ReadFiles {
            files: vec![tool_call::read_files::File {
                name: path.to_string(),
                line_ranges: Vec::new(),
            }],
        })),
    }))
}

fn stored(input: &api::request::input::ToolCallResult) -> api::Message {
    message(message::Message::ToolCallResult(stored_result(input)))
}

/// The point of storing results: a file read early in a conversation is still there for the model
/// two turns later, not replaced by a placeholder.
#[test]
fn a_result_read_in_turn_one_is_replayed_in_turn_three() {
    let request = history_with_inputs(
        vec![
            user_message("read Cargo.toml"),
            read_call("call-1", "Cargo.toml"),
            stored(&read_input("call-1", "Cargo.toml", "members = 42")),
            agent_message("It has 42 members."),
            user_message("run ls"),
            shell_call("call-2", "ls"),
            stored(&shell_input("call-2", "src")),
            agent_message("There is a src directory."),
        ],
        vec![user_query_input("what did the file say?")],
    );

    let turns = turns_from_request(&request);
    let results = turns
        .iter()
        .filter_map(|turn| match turn {
            Turn::ToolResults(results) => Some(results),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();

    assert_eq!(results.len(), 2, "{turns:?}");
    assert_eq!(results[0].id, "call-1");
    assert!(results[0].content.contains("members = 42"));
    assert_eq!(results[1].id, "call-2");
    assert!(
        results
            .iter()
            .all(|result| result.content != RESULT_NOT_KEPT)
    );
    assert_eq!(
        turns.last(),
        Some(&Turn::User("what did the file say?".to_string()))
    );
}

/// The history already answers the call with the copy the emitter stored, so the input copy adds
/// nothing, wherever in the history that copy sits.
#[test]
fn a_result_both_stored_and_resent_is_sent_once() {
    let request = history_with_inputs(
        vec![
            user_message("run ls"),
            shell_call("call-1", "ls"),
            stored(&shell_input("call-1", "src")),
        ],
        vec![tool_result_input(shell_input("call-1", "src"))],
    );

    let turns = turns_from_request(&request);
    assert_eq!(
        turns.last(),
        Some(&Turn::ToolResults(vec![ToolResult {
            id: "call-1".to_string(),
            content: "exit code: 0\n\nsrc".to_string(),
            is_error: false,
        }]))
    );
}

#[test]
fn a_resent_result_never_follows_a_later_turn() {
    let request = history_with_inputs(
        vec![
            user_message("run ls"),
            shell_call("call-1", "ls"),
            stored(&shell_input("call-1", "src")),
            agent_message("There is a src directory."),
            user_message("thanks"),
        ],
        vec![
            tool_result_input(shell_input("call-1", "src")),
            user_query_input("what else?"),
        ],
    );

    let turns = turns_from_request(&request);
    let result_turns = turns
        .iter()
        .filter(|turn| matches!(turn, Turn::ToolResults(_)))
        .count();
    assert_eq!(result_turns, 1, "{turns:?}");
    assert_eq!(
        turns.last(),
        Some(&Turn::User("thanks\n\nwhat else?".to_string()))
    );
}

fn grep_input(files: usize, error: Option<&str>) -> api::request::input::ToolCallResult {
    use api::grep_result::success::GrepFileMatch;
    use api::grep_result::success::grep_file_match::GrepLineMatch;

    let result = match error {
        Some(message) => api::grep_result::Result::Error(api::grep_result::Error {
            message: message.to_string(),
        }),
        None => api::grep_result::Result::Success(api::grep_result::Success {
            matched_files: (0..files)
                .map(|index| GrepFileMatch {
                    file_path: format!("src/file_{index:05}.rs"),
                    matched_lines: vec![GrepLineMatch { line_number: 1 }],
                })
                .collect(),
        }),
    };
    input_result(api::request::input::tool_call_result::Result::Grep(
        api::GrepResult {
            result: Some(result),
        },
    ))
}

/// Lists and error messages are bounded like every other text the model is shown, and cutting a
/// list does not change what the model reads.
#[test]
fn storing_a_list_or_an_error_keeps_it_bounded() {
    let input = grep_input(20_000, None);
    let full = render_result(&message::ToolCallResult {
        tool_call_id: "call-1".to_string(),
        context: None,
        result: input.result.clone().map(|result| match result {
            api::request::input::tool_call_result::Result::Grep(grep) => {
                tool_call_result::Result::Grep(grep)
            }
            _ => unreachable!(),
        }),
    });
    let stored = stored_result(&input);
    let Some(tool_call_result::Result::Grep(api::GrepResult {
        result: Some(api::grep_result::Result::Success(success)),
    })) = stored.result.as_ref()
    else {
        panic!("expected a grep success");
    };
    assert!(success.matched_files.len() < 20_000);

    let replayed = render_result(&stored);
    let (head, tail) = (4_000, 11_800);
    assert_eq!(
        full.content.chars().take(head).collect::<String>(),
        replayed.content.chars().take(head).collect::<String>()
    );
    assert_eq!(
        full.content.chars().rev().take(tail).collect::<String>(),
        replayed
            .content
            .chars()
            .rev()
            .take(tail)
            .collect::<String>()
    );

    let long = marked_text(MAX_RESULT_CHARS * 2);
    let rendered = render_input_result(&grep_input(0, Some(&long)));
    assert!(rendered.is_error);
    assert_eq!(rendered.content, truncate(&long));
}

fn input_of_result(result: api::request::input::ToolCallResult) -> api::request::Input {
    use api::request::input::user_inputs::UserInput;
    use api::request::input::user_inputs::user_input::Input;

    api::request::Input {
        context: None,
        r#type: Some(api::request::input::Type::UserInputs(
            api::request::input::UserInputs {
                inputs: vec![UserInput {
                    input: Some(Input::ToolCallResult(result)),
                }],
            },
        )),
    }
}

#[test]
fn input_text_holds_a_file_read_as_the_model_sees_it() {
    let content = "x".repeat(10_000);
    let text = input_text(&input_of_result(read_input("call-1", "big.rs", &content)));

    assert!(text.contains("--- big.rs ---"));
    assert!(text.contains(&content));
}

#[test]
fn input_text_holds_a_truncated_result_not_the_whole_output() {
    let long = marked_text(MAX_RESULT_CHARS * 4);
    let text = input_text(&input_of_result(shell_input("call-1", &long)));

    assert!(
        text.chars().count() < MAX_RESULT_CHARS * 2,
        "{}",
        text.len()
    );
    assert!(text.chars().count() > MAX_RESULT_CHARS / 2);
}

#[test]
fn input_text_holds_the_question_and_its_attachments() {
    let referenced = std::collections::HashMap::from([(
        "block".to_string(),
        api::Attachment {
            value: Some(api::attachment::Value::PlainText("the output".to_string())),
        },
    )]);
    let mut context = api::InputContext::default();
    context
        .selected_text
        .push(api::input_context::SelectedText {
            text: "picked".to_string(),
        });
    let request = user_inputs_request("why did it fail?", referenced, Some(context));

    let text = input_text(request.input.as_ref().expect("an input"));
    assert!(text.starts_with("why did it fail?"));
    assert!(text.contains("the output"));
    assert!(text.contains("picked"));
}

fn remember_result(result: api::remember_result::Result) -> api::request::input::ToolCallResult {
    input_result(api::request::input::tool_call_result::Result::Remember(
        api::RememberResult {
            result: Some(result),
        },
    ))
}

#[test]
fn a_remember_result_is_rendered_for_the_model() {
    use api::remember_result::{Error, Result as Remember, Success};

    let global = render_input_result(&remember_result(Remember::Success(Success::default())));
    assert_eq!(global.content, "Saved to memory for all projects.");
    assert!(!global.is_error);

    let project = render_input_result(&remember_result(Remember::Success(Success {
        project_root: "/work/app".to_string(),
        already_saved: false,
    })));
    assert_eq!(
        project.content,
        "Saved to memory for the project at `/work/app`."
    );

    let duplicate = render_input_result(&remember_result(Remember::Success(Success {
        project_root: "/work/app".to_string(),
        already_saved: true,
    })));
    assert!(duplicate.content.contains("already in memory"));
    assert!(!duplicate.is_error);

    let error = render_input_result(&remember_result(Remember::Error(Error {
        message: "The memory is empty.".to_string(),
    })));
    assert_eq!(error.content, "The memory is empty.");
    assert!(error.is_error);
}

#[test]
fn a_remember_result_is_stored_as_sent() {
    let input = remember_result(api::remember_result::Result::Success(
        api::remember_result::Success {
            project_root: "/work/app".to_string(),
            already_saved: false,
        },
    ));

    let stored = stored_result(&input);

    let Some(tool_call_result::Result::Remember(remember)) = stored.result else {
        panic!("expected a remember result, got {:?}", stored.result);
    };
    let Some(api::request::input::tool_call_result::Result::Remember(sent)) = input.result else {
        unreachable!();
    };
    assert_eq!(remember, sent);
}
