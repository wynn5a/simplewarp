use warp_multi_agent_api as api;
use warp_multi_agent_api::message::{self, tool_call, tool_call_result};

use super::*;

fn with_id(id: &str, inner: message::Message) -> api::Message {
    api::Message {
        id: id.to_string(),
        message: Some(inner),
        ..Default::default()
    }
}

fn user(id: &str, text: &str) -> api::Message {
    with_id(
        id,
        message::Message::UserQuery(message::UserQuery {
            query: text.to_string(),
            ..Default::default()
        }),
    )
}

fn agent(id: &str, text: &str) -> api::Message {
    with_id(
        id,
        message::Message::AgentOutput(message::AgentOutput {
            text: text.to_string(),
        }),
    )
}

fn read_call(id: &str, call_id: &str, path: &str) -> api::Message {
    with_id(
        id,
        message::Message::ToolCall(message::ToolCall {
            tool_call_id: call_id.to_string(),
            tool: Some(tool_call::Tool::ReadFiles(tool_call::ReadFiles {
                files: vec![tool_call::read_files::File {
                    name: path.to_string(),
                    line_ranges: Vec::new(),
                }],
            })),
        }),
    )
}

fn edit_call(id: &str, call_id: &str, path: &str) -> api::Message {
    with_id(
        id,
        message::Message::ToolCall(message::ToolCall {
            tool_call_id: call_id.to_string(),
            tool: Some(tool_call::Tool::ApplyFileDiffs(tool_call::ApplyFileDiffs {
                summary: "edit".to_string(),
                diffs: vec![tool_call::apply_file_diffs::FileDiff {
                    file_path: path.to_string(),
                    search: "a".to_string(),
                    replace: "b".to_string(),
                }],
                ..Default::default()
            })),
        }),
    )
}

fn shell_result(id: &str, call_id: &str, output: &str) -> api::Message {
    with_id(
        id,
        message::Message::ToolCallResult(message::ToolCallResult {
            tool_call_id: call_id.to_string(),
            result: Some(tool_call_result::Result::RunShellCommand(
                api::RunShellCommandResult {
                    result: Some(api::run_shell_command_result::Result::CommandFinished(
                        api::ShellCommandFinished {
                            output: output.to_string(),
                            ..Default::default()
                        },
                    )),
                    ..Default::default()
                },
            )),
            ..Default::default()
        }),
    )
}

fn summary(
    id: &str,
    text: &str,
    first_kept: &str,
    read_files: &[&str],
    modified_files: &[&str],
) -> api::Message {
    with_id(
        id,
        message::Message::Summarization(message::Summarization {
            finished_duration: Some(prost_types::Duration {
                seconds: 1,
                nanos: 0,
            }),
            summary_type: Some(message::summarization::SummaryType::ConversationSummary(
                message::summarization::ConversationSummary {
                    summary: text.to_string(),
                    first_kept_message_id: first_kept.to_string(),
                    read_files: read_files.iter().map(|path| path.to_string()).collect(),
                    modified_files: modified_files.iter().map(|path| path.to_string()).collect(),
                    ..Default::default()
                },
            )),
        }),
    )
}

fn summarize_request(
    messages: Vec<api::Message>,
    keep_recent_tokens: u32,
    context_tokens: u64,
) -> api::Request {
    api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: vec![api::Task {
                id: "task".to_string(),
                messages,
                ..Default::default()
            }],
        }),
        input: Some(api::request::Input {
            r#type: Some(api::request::input::Type::SummarizeConversation(
                api::request::input::SummarizeConversation {
                    prompt: String::new(),
                    keep_recent_tokens,
                    context_tokens,
                    max_output_tokens: 8_000,
                },
            )),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Three exchanges whose questions are about 1,000 estimated tokens each.
fn three_exchanges() -> Vec<api::Message> {
    let filler = "x".repeat(4_000);
    vec![
        user("q1", &format!("first question {filler}")),
        agent("a1", "ok"),
        user("q2", &format!("second question {filler}")),
        agent("a2", "ok"),
        user("q3", &format!("third question {filler}")),
        agent("a3", "ok"),
    ]
}

#[test]
fn a_summary_request_summarizes_up_to_the_cut_and_keeps_the_rest() {
    let plan = plan(&summarize_request(three_exchanges(), 1_000, 0)).unwrap();

    assert_eq!(plan.outcome.first_kept_message_id, "q3");
    assert!(plan.prompt_text.contains("[User]: first question"));
    assert!(plan.prompt_text.contains("[User]: second question"));
    assert!(!plan.prompt_text.contains("third question"));
    assert!(!plan.prompt_text.contains("<turn-prefix>"));
}

#[test]
fn an_old_client_summarizes_everything() {
    let plan = plan(&summarize_request(three_exchanges(), 0, 0)).unwrap();

    assert_eq!(plan.outcome.first_kept_message_id, "");
    assert!(plan.prompt_text.contains("third question"));
}

#[test]
fn a_second_compaction_starts_at_the_previous_boundary_and_updates_the_summary() {
    let mut messages = three_exchanges();
    messages.extend([
        summary(
            "s1",
            "## Goal\nOld goal\n\n<read-files>\nold.rs\n</read-files>",
            "q3",
            &["old.rs"],
            &[],
        ),
        user("q4", &format!("fourth question {}", "y".repeat(4_000))),
        agent("a4", "ok"),
    ]);
    let plan = plan(&summarize_request(messages, 1_000, 0)).unwrap();

    assert_eq!(plan.outcome.first_kept_message_id, "q4");
    assert!(
        plan.prompt_text
            .contains("<previous-summary>\n## Goal\nOld goal\n</previous-summary>")
    );
    assert!(!plan.prompt_text.contains("first question"));
    assert!(!plan.prompt_text.contains("second question"));
    assert!(plan.prompt_text.contains("[User]: third question"));
    assert!(!plan.prompt_text.contains("fourth question"));
    assert!(!plan.prompt_text.contains("old.rs"));
}

#[test]
fn file_operations_accumulate_across_compactions() {
    let messages = vec![
        summary(
            "s1",
            "## Goal\nOld",
            "",
            &["old.rs", "both.rs"],
            &["changed.rs"],
        ),
        user("q1", "go on"),
        read_call("c1", "call-1", "new.rs"),
        shell_result("r1", "call-1", "contents"),
        edit_call("c2", "call-2", "both.rs"),
        user("q2", "and now"),
        agent("a2", "ok"),
    ];
    let plan = plan(&summarize_request(messages, 1, 0)).unwrap();

    assert_eq!(plan.outcome.read_files, vec!["new.rs", "old.rs"]);
    assert_eq!(plan.outcome.modified_files, vec!["both.rs", "changed.rs"]);
}

#[test]
fn a_split_turn_sends_the_turn_prefix() {
    let big = "z".repeat(8_000);
    let messages = vec![
        user("q1", "earlier question"),
        agent("a1", "earlier answer"),
        user("q2", "one long task"),
        read_call("c1", "call-1", "a.rs"),
        shell_result("r1", "call-1", &big),
        read_call("c2", "call-2", "b.rs"),
        shell_result("r2", "call-2", &big),
        agent("a2", "almost there"),
    ];
    let plan = plan(&summarize_request(messages, 2_500, 0)).unwrap();

    assert_eq!(plan.outcome.first_kept_message_id, "c2");
    assert!(plan.prompt_text.contains("[User]: earlier question"));
    let prefix_start = plan.prompt_text.find("<turn-prefix>").unwrap();
    let prefix = &plan.prompt_text[prefix_start..];
    assert!(prefix.contains("[User]: one long task"));
    assert!(prefix.contains("read_files("));
    assert!(plan.prompt_text.contains("Turn Context (split turn)"));
    assert_eq!(plan.outcome.read_files, vec!["a.rs"]);
}

#[test]
fn the_reported_context_is_used_as_tokens_before() {
    let reported = plan(&summarize_request(three_exchanges(), 1_000, 50_000)).unwrap();
    assert_eq!(reported.outcome.tokens_before, 50_000);
    assert!(reported.outcome.summarized_tokens >= 2_000);

    let estimated = plan(&summarize_request(three_exchanges(), 1_000, 0)).unwrap();
    assert!(estimated.outcome.tokens_before > estimated.outcome.summarized_tokens);
    assert!(estimated.outcome.tokens_before >= 3_000);
}

#[test]
fn a_request_that_asks_for_something_else_has_no_plan() {
    let mut request = summarize_request(three_exchanges(), 1_000, 0);
    request.input = Some(api::request::Input {
        r#type: Some(api::request::input::Type::ResumeConversation(
            Default::default(),
        )),
        ..Default::default()
    });
    assert_eq!(plan(&request), None);
}

#[test]
fn a_split_turn_with_no_history_before_it_is_summarized_as_the_history() {
    let big = "z".repeat(8_000);
    let messages = vec![
        user("q1", "one long task"),
        read_call("c1", "call-1", "a.rs"),
        shell_result("r1", "call-1", &big),
        read_call("c2", "call-2", "b.rs"),
        shell_result("r2", "call-2", &big),
        agent("a2", "almost there"),
    ];
    let plan = plan(&summarize_request(messages, 2_500, 0)).unwrap();

    assert_eq!(plan.outcome.first_kept_message_id, "c2");
    assert!(!plan.prompt_text.contains("<turn-prefix>"));
    assert!(!plan.prompt_text.contains("Turn Context"));
    assert!(
        plan.prompt_text
            .starts_with("<conversation>\n[User]: one long task")
    );
}

#[test]
fn the_output_cap_is_the_one_the_client_asked_for() {
    assert_eq!(
        plan(&summarize_request(three_exchanges(), 1_000, 0))
            .unwrap()
            .max_output_tokens,
        Some(8_000)
    );
    let mut request = summarize_request(three_exchanges(), 1_000, 0);
    if let Some(api::request::input::Type::SummarizeConversation(summarize)) = request
        .input
        .as_mut()
        .and_then(|input| input.r#type.as_mut())
    {
        summarize.max_output_tokens = 0;
    }
    assert_eq!(plan(&request).unwrap().max_output_tokens, None);
}
