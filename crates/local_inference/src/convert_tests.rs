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
                token_count: 0,
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
