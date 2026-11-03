//! Turns the proto conversation into a provider-neutral turn list.
//!
//! A [`api::Request`] carries the whole conversation: every task, every message, plus the new
//! input. Anthropic and OpenAI both want a flat list of turns, so this module flattens the proto
//! into [`Turn`]s that either provider module can render.
//!
//! Tool results are rendered to text here, not in the provider modules, so that both providers
//! show the model the same thing.

use std::collections::HashSet;

use serde_json::{Value, json};
use warp_multi_agent_api as api;
use warp_multi_agent_api::message::{self, tool_call, tool_call_result};

use crate::{context, inputs, mcp};

/// One turn of the conversation, in a shape that both providers can render.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    /// Something the user said.
    User(String),
    /// Something the agent said, and any tools it decided to call.
    Assistant {
        text: String,
        /// The thinking that came with the reply, when the provider streamed any.
        ///
        /// Most providers treat this as display-only. A reasoning model behind an
        /// OpenAI-compatible gateway can require it back, so it is carried rather than dropped.
        /// See [`crate::provider::openai`].
        reasoning: String,
        tool_calls: Vec<ToolUse>,
    },
    /// The results of the tool calls in the turn before.
    ToolResults(Vec<ToolResult>),
}

/// A tool call, as the model made it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// The result of running a tool, already rendered to text for the model.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub id: String,
    pub content: String,
    pub is_error: bool,
}

/// The conversation a request carries before its input, as the model sees it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct History {
    /// The turns after the newest summary, or all of them when there is none.
    pub turns: Vec<Turn>,
    /// For each turn, the id of the message that opened it.
    pub openers: Vec<String>,
    /// The newest finished summary, which stands in for everything before `turns`.
    pub summary: Option<StoredSummary>,
}

/// A finished conversation summary in the history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StoredSummary {
    pub text: String,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

impl History {
    fn push_message(&mut self, proto_message: &api::Message) {
        if let Some(message::Message::Summarization(summarization)) = proto_message.message.as_ref()
        {
            self.apply_summary(summarization);
            return;
        }
        let before = self.turns.len();
        push_message(&mut self.turns, proto_message);
        self.openers
            .extend((before..self.turns.len()).map(|_| proto_message.id.clone()));
    }

    /// A finished summary stands in for everything before the message it names as the first one
    /// kept, or for everything before it when it names none (or one no turn opens, which a summary
    /// written by an older build can do). A summary still being written stands in for nothing yet.
    fn apply_summary(&mut self, summarization: &message::Summarization) {
        let Some(summary) = finished_summary(summarization) else {
            return;
        };
        let first_kept = (!summary.first_kept_message_id.is_empty())
            .then(|| {
                self.openers
                    .iter()
                    .position(|opener| *opener == summary.first_kept_message_id)
            })
            .flatten()
            .unwrap_or(self.turns.len());
        self.turns.drain(..first_kept);
        self.openers.drain(..first_kept);
        self.summary = Some(StoredSummary {
            text: summary.summary.clone(),
            read_files: summary.read_files.clone(),
            modified_files: summary.modified_files.clone(),
        });
    }
}

/// Reads the history out of a request.
pub(crate) fn history_from_request(request: &api::Request) -> History {
    let mut history = History::default();
    if let Some(context) = request.task_context.as_ref() {
        for task in &context.tasks {
            for proto_message in &task.messages {
                history.push_message(proto_message);
            }
        }
    }
    history
}

/// Flattens a request into a turn list.
///
/// Adjacent messages of the same kind are merged, because a provider rejects two assistant turns
/// in a row, and the proto history splits one agent reply across several messages.
pub fn turns_from_request(request: &api::Request) -> Vec<Turn> {
    let history = history_from_request(request);
    let mut turns = Vec::new();
    if let Some(summary) = history.summary.as_ref() {
        push_user(&mut turns, &inputs::summary_turn(&summary.text));
    }
    for turn in history.turns {
        match turn {
            Turn::User(text) => push_user(&mut turns, &text),
            turn @ (Turn::Assistant { .. } | Turn::ToolResults(_)) => turns.push(turn),
        }
    }

    if let Some(input) = request.input.as_ref() {
        let answered = turns
            .iter()
            .filter_map(|turn| match turn {
                Turn::ToolResults(results) => Some(results),
                Turn::User(_) | Turn::Assistant { .. } => None,
            })
            .flatten()
            .map(|result| result.id.clone())
            .collect::<HashSet<_>>();
        push_input(&mut turns, input, &answered);
    }

    let mut turns = unorphan_results(turns);
    pair_tool_calls(&mut turns);
    turns
}

/// Turns a tool result whose call is not in the conversation into user text.
///
/// After a compaction the call a result answers can be in the summary rather than in the turns,
/// and a provider rejects a result that answers no call. The results that do answer a call stay
/// first, so they still follow it directly.
fn unorphan_results(turns: Vec<Turn>) -> Vec<Turn> {
    let mut called = HashSet::new();
    let mut fixed = Vec::with_capacity(turns.len());
    for turn in turns {
        match turn {
            Turn::User(text) => push_user(&mut fixed, &text),
            Turn::Assistant {
                text,
                reasoning,
                tool_calls,
            } => {
                called.extend(tool_calls.iter().map(|call| call.id.clone()));
                fixed.push(Turn::Assistant {
                    text,
                    reasoning,
                    tool_calls,
                });
            }
            Turn::ToolResults(results) => {
                let (answering, orphans) = results
                    .into_iter()
                    .partition::<Vec<_>, _>(|result| called.contains(&result.id));
                if !answering.is_empty() {
                    fixed.push(Turn::ToolResults(answering));
                }
                for orphan in orphans {
                    push_user(
                        &mut fixed,
                        &format!(
                            "Result of a tool call made before the summary ({}):\n{}",
                            orphan.id, orphan.content
                        ),
                    );
                }
            }
        }
    }
    fixed
}

/// The text a request's input adds to what the model sees: the question with its attachments, each
/// tool result as rendered for the model, or the expansion of another input. The newest-turn
/// environment (`current_state`) is left out, because no later request carries it.
pub fn input_text(input: &api::request::Input) -> String {
    let mut turns = Vec::new();
    push_input(&mut turns, input, &HashSet::new());
    let mut text = String::new();
    for turn in turns {
        let parts = match turn {
            Turn::User(user) => vec![user],
            Turn::ToolResults(results) => {
                results.into_iter().map(|result| result.content).collect()
            }
            Turn::Assistant { .. } => Vec::new(),
        };
        for part in parts {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&part);
        }
    }
    text
}

/// The text that stands in for a tool result the conversation no longer holds.
const RESULT_NOT_KEPT: &str = "This call already ran. Its output was not kept in the conversation history, so it cannot be \
     shown again. Do not run it a second time unless the answer needs it.";

/// Gives every tool call a result, inserting a placeholder where the conversation has none.
///
/// Both providers reject a conversation where an assistant message calls a tool and no result
/// answers it. OpenAI says "an assistant message with 'tool_calls' must be followed by tool
/// messages responding to each 'tool_call_id'"; Anthropic wants a `tool_result` block for every
/// `tool_use`.
///
/// The emitter stores every result that arrives in `Request::input` (see [`crate::emit`]), but a
/// conversation stored before it did holds the calls without their results, and a call that was
/// cut off by a failed or cancelled request has none either.
///
/// Rather than depend on what the client happens to store, this makes the turn list valid on its
/// own. A placeholder keeps the call visible, so the model still knows the command ran and does
/// not repeat it.
fn pair_tool_calls(turns: &mut Vec<Turn>) {
    let mut index = 0;
    while index < turns.len() {
        let Turn::Assistant { tool_calls, .. } = &turns[index] else {
            index += 1;
            continue;
        };
        if tool_calls.is_empty() {
            index += 1;
            continue;
        }
        let call_ids = tool_calls
            .iter()
            .map(|call| call.id.clone())
            .collect::<Vec<_>>();

        // The results for these calls, if any, are the turn straight after them.
        let existing = match turns.get(index + 1) {
            Some(Turn::ToolResults(results)) => {
                results.iter().map(|result| result.id.clone()).collect()
            }
            _ => Vec::new(),
        };

        let missing = call_ids
            .into_iter()
            .filter(|id| !existing.contains(id))
            .map(|id| ToolResult {
                id,
                content: RESULT_NOT_KEPT.to_string(),
                is_error: false,
            })
            .collect::<Vec<_>>();

        if !missing.is_empty() {
            match turns.get_mut(index + 1) {
                Some(Turn::ToolResults(results)) => results.extend(missing),
                _ => turns.insert(index + 1, Turn::ToolResults(missing)),
            }
        }
        // Step over the assistant turn and the results that now answer it.
        index += 2;
    }
}

/// Pushes the new input after the history.
///
/// A tool result whose call the history already answers is left out. The emitter stores each
/// result before the reply, and the stored copy is the one placed right after its call; a second
/// copy after a later turn would answer no call, and a provider rejects it.
// The deprecated `UserQuery` and `ToolCallResult` input variants are still handled, because a
// conversation that an older client started can carry them.
#[allow(deprecated)]
fn push_input(turns: &mut Vec<Turn>, input: &api::request::Input, answered: &HashSet<String>) {
    use api::request::input::Type;
    use api::request::input::user_inputs::user_input::Input as UserInput;

    match input.r#type.as_ref() {
        Some(Type::UserInputs(inputs)) => {
            let mut has_query = false;
            for entry in &inputs.inputs {
                match entry.input.as_ref() {
                    Some(UserInput::UserQuery(query)) => {
                        push_query(turns, query);
                        has_query = true;
                    }
                    Some(UserInput::ToolCallResult(result)) => {
                        if !answered.contains(&result.tool_call_id) {
                            push_rendered(turns, render_input_result(result));
                        }
                    }
                    _ => {}
                }
            }
            // What the user attached to the request belongs with their message, not with a
            // tool result.
            if has_query {
                push_user(
                    turns,
                    &context::attachments(input.context.as_ref(), &Default::default()),
                );
            }
        }
        Some(Type::UserQuery(query)) => {
            push_query(turns, query);
            push_user(
                turns,
                &context::attachments(input.context.as_ref(), &Default::default()),
            );
        }
        Some(Type::ToolCallResult(result)) => {
            if !answered.contains(&result.tool_call_id) {
                push_rendered(turns, render_input_result(result));
            }
        }
        Some(other) => {
            if let Some(text) = expand_input(other) {
                push_user(turns, &text);
                push_user(
                    turns,
                    &context::attachments(input.context.as_ref(), &Default::default()),
                );
            }
        }
        None => {}
    }
}

/// The model-facing text for an input that is not a plain question. See [`inputs`].
///
/// `None` is for inputs that need a service this build does not have, or that the client no
/// longer sends: passive suggestions, cloud environments and ambient runs.
fn expand_input(input_type: &api::request::input::Type) -> Option<String> {
    use api::request::input::Type;

    Some(match input_type {
        Type::SummarizeConversation(summarize) => inputs::summarize_conversation(&summarize.prompt),
        Type::InitProjectRules(_) => inputs::INIT_PROJECT_RULES.to_string(),
        Type::ResumeConversation(_) => inputs::RESUME_CONVERSATION.to_string(),
        Type::CreateNewProject(project) => inputs::create_new_project(&project.query),
        Type::CloneRepository(clone) => inputs::clone_repository(&clone.url),
        Type::InvokeSkill(invoke) => inputs::invoke_skill(
            invoke.skill.as_ref(),
            invoke
                .user_query
                .as_ref()
                .map(|query| query.query.as_str())
                .unwrap_or_default(),
        ),
        Type::CodeReview(review) => {
            let Some(api::request::input::code_review::Operation::InitialReviewComments(initial)) =
                review.operation.as_ref()
            else {
                return None;
            };
            inputs::code_review(&initial.review_comments, initial.diff_set.as_ref())
        }
        // A chip's own text is what the user asked. The server answered it with a canned
        // reply first; here the model answers it.
        Type::QueryWithCannedResponse(canned) => canned.query.clone(),
        _ => return None,
    })
}

/// Pushes a user query, followed by the attachments that its text names by key.
fn push_query(turns: &mut Vec<Turn>, query: &api::request::input::UserQuery) {
    push_user(turns, &query.query);
    let referenced = context::attachments(None, &query.referenced_attachments);
    push_user(turns, &referenced);
}

fn push_message(turns: &mut Vec<Turn>, proto_message: &api::Message) {
    match proto_message.message.as_ref() {
        Some(message::Message::UserQuery(query)) => push_user(turns, &query.query),
        Some(message::Message::AgentOutput(output)) => push_agent_text(turns, &output.text),
        Some(message::Message::ToolCall(call)) => push_tool_call(turns, call),
        Some(message::Message::ToolCallResult(result)) => push_tool_result(turns, result),
        Some(message::Message::AgentReasoning(reasoning)) => {
            push_agent_reasoning(turns, &reasoning.reasoning)
        }
        // The inputs that the client stores in place of a question. See [`crate::emit`].
        Some(message::Message::SystemQuery(query)) => {
            if let Some(text) = system_query_text(query) {
                push_user(turns, &text);
            }
        }
        Some(message::Message::InvokeSkill(invoke)) => push_user(
            turns,
            &inputs::invoke_skill(
                invoke.skill.as_ref(),
                invoke
                    .user_query
                    .as_ref()
                    .map(|query| query.query.as_str())
                    .unwrap_or_default(),
            ),
        ),
        Some(message::Message::CodeReview(review)) => {
            if let Some(comments) = review.comments.as_ref() {
                push_user(
                    turns,
                    &inputs::code_review(&comments.pending_comments, comments.diff_set.as_ref()),
                );
            }
        }
        // Todos and server events carry no instruction that the model needs replayed, so they
        // are left out. A summary is placed by [`History`].
        _ => {}
    }
}

/// The text of a stored system query, in the same words the input carried.
fn system_query_text(query: &message::SystemQuery) -> Option<String> {
    use message::system_query::Type;

    Some(match query.r#type.as_ref()? {
        // The instruction was for the model that wrote the summary, not part of the conversation.
        Type::SummarizeConversation(_) => return None,
        Type::ResumeConversation(_) => inputs::RESUME_CONVERSATION.to_string(),
        Type::CreateNewProject(project) => inputs::create_new_project(&project.query),
        Type::CloneRepository(clone) => inputs::clone_repository(&clone.url),
        // Passive suggestions and the rest were never answered by this build.
        _ => return None,
    })
}

/// The conversation summary, if this is one that has finished.
fn finished_summary(
    summarization: &message::Summarization,
) -> Option<&message::summarization::ConversationSummary> {
    summarization.finished_duration?;
    match summarization.summary_type.as_ref()? {
        message::summarization::SummaryType::ConversationSummary(summary)
            if !summary.summary.trim().is_empty() =>
        {
            Some(summary)
        }
        _ => None,
    }
}

fn push_user(turns: &mut Vec<Turn>, text: &str) {
    if text.is_empty() {
        return;
    }
    match turns.last_mut() {
        Some(Turn::User(existing)) => {
            existing.push_str("\n\n");
            existing.push_str(text);
        }
        _ => turns.push(Turn::User(text.to_string())),
    }
}

fn push_agent_text(turns: &mut Vec<Turn>, text: &str) {
    if text.is_empty() {
        return;
    }
    match turns.last_mut() {
        Some(Turn::Assistant {
            text: existing,
            tool_calls,
            ..
        }) if tool_calls.is_empty() => {
            existing.push_str(text);
        }
        _ => turns.push(Turn::Assistant {
            text: text.to_string(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
        }),
    }
}

/// Adds streamed thinking to the reply it belongs to.
///
/// The proto puts reasoning in its own message, and the emitter writes it before the text and the
/// tool calls of the same reply, so this normally opens the assistant turn that they then join.
fn push_agent_reasoning(turns: &mut Vec<Turn>, reasoning: &str) {
    if reasoning.is_empty() {
        return;
    }
    match turns.last_mut() {
        Some(Turn::Assistant {
            reasoning: existing,
            ..
        }) => existing.push_str(reasoning),
        _ => turns.push(Turn::Assistant {
            text: String::new(),
            reasoning: reasoning.to_string(),
            tool_calls: Vec::new(),
        }),
    }
}

fn push_tool_call(turns: &mut Vec<Turn>, call: &message::ToolCall) {
    let Some(use_) = tool_use_from_proto(call) else {
        return;
    };
    match turns.last_mut() {
        Some(Turn::Assistant { tool_calls, .. }) => tool_calls.push(use_),
        _ => turns.push(Turn::Assistant {
            text: String::new(),
            reasoning: String::new(),
            tool_calls: vec![use_],
        }),
    }
}

fn push_tool_result(turns: &mut Vec<Turn>, result: &message::ToolCallResult) {
    push_rendered(turns, render_result(result));
}

fn push_rendered(turns: &mut Vec<Turn>, rendered: ToolResult) {
    match turns.last_mut() {
        Some(Turn::ToolResults(results)) => results.push(rendered),
        _ => turns.push(Turn::ToolResults(vec![rendered])),
    }
}

/// The reverse of [`crate::tools::to_proto`]: turns a proto tool call back into the name and
/// JSON arguments that the model produced.
pub fn tool_use_from_proto(call: &message::ToolCall) -> Option<ToolUse> {
    let (name, arguments) = match call.tool.as_ref()? {
        tool_call::Tool::RunShellCommand(run) => (
            "run_shell_command",
            json!({ "command": run.command, "is_read_only": run.is_read_only }),
        ),
        tool_call::Tool::ReadFiles(read) => {
            let files = read
                .files
                .iter()
                .map(|file| {
                    let mut entry = json!({ "path": file.name });
                    if let Some(range) = file.line_ranges.first() {
                        entry["start_line"] = json!(range.start);
                        entry["end_line"] = json!(range.end);
                    }
                    entry
                })
                .collect::<Vec<_>>();
            ("read_files", json!({ "files": files }))
        }
        tool_call::Tool::ApplyFileDiffs(apply) => {
            let diffs = apply
                .diffs
                .iter()
                .map(|diff| {
                    json!({
                        "file_path": diff.file_path,
                        "search": diff.search,
                        "replace": diff.replace,
                    })
                })
                .collect::<Vec<_>>();
            let new_files = apply
                .new_files
                .iter()
                .map(|file| json!({ "file_path": file.file_path, "content": file.content }))
                .collect::<Vec<_>>();
            let deleted_files = apply
                .deleted_files
                .iter()
                .map(|file| json!({ "file_path": file.file_path }))
                .collect::<Vec<_>>();
            (
                "apply_file_diffs",
                json!({
                    "summary": apply.summary,
                    "diffs": diffs,
                    "new_files": new_files,
                    "deleted_files": deleted_files,
                }),
            )
        }
        tool_call::Tool::Grep(grep) => (
            "grep",
            json!({ "queries": grep.queries, "path": grep.path }),
        ),
        tool_call::Tool::FileGlobV2(glob) => (
            "file_glob",
            json!({
                "patterns": glob.patterns,
                "search_dir": glob.search_dir,
                "max_matches": glob.max_matches,
            }),
        ),
        tool_call::Tool::ReadShellCommandOutput(poll) => {
            let mut arguments = json!({ "command_id": poll.command_id });
            match poll.delay.as_ref() {
                Some(tool_call::read_shell_command_output::Delay::Duration(duration)) => {
                    arguments["max_wait_seconds"] = json!(duration.seconds);
                }
                Some(tool_call::read_shell_command_output::Delay::OnCompletion(_)) => {
                    arguments["wait_for_completion"] = json!(true);
                }
                None => {}
            }
            ("read_shell_command_output", arguments)
        }
        tool_call::Tool::WriteToLongRunningShellCommand(write) => {
            let mut arguments = json!({
                "command_id": write.command_id,
                "input": String::from_utf8_lossy(&write.input),
            });
            if let Some(mode) = write.mode.as_ref()
                && let Some(kind) = mode.mode.as_ref()
            {
                arguments["mode"] = json!(match kind {
                    tool_call::write_to_long_running_shell_command::mode::Mode::Raw(_) => "raw",
                    tool_call::write_to_long_running_shell_command::mode::Mode::Line(_) => "line",
                    tool_call::write_to_long_running_shell_command::mode::Mode::Block(_) => "block",
                });
            }
            ("write_to_long_running_shell_command", arguments)
        }
        tool_call::Tool::CallMcpTool(mcp_call) => {
            return Some(ToolUse {
                id: call.tool_call_id.clone(),
                name: mcp::wire_name(&mcp_call.server_id, &mcp_call.name),
                arguments: mcp_call
                    .args
                    .as_ref()
                    .map(mcp::struct_to_json)
                    .unwrap_or_else(|| json!({})),
            });
        }
        tool_call::Tool::ReadMcpResource(read) => {
            (mcp::READ_RESOURCE_TOOL, json!({ "uri": read.uri }))
        }
        tool_call::Tool::TransferShellCommandControlToUser(transfer) => (
            "transfer_control_to_user",
            json!({ "reason": transfer.reason }),
        ),
        tool_call::Tool::InsertReviewComments(insert) => {
            let comments = insert
                .comments
                .iter()
                .map(review_comment_to_json)
                .collect::<Vec<_>>();
            (
                "insert_review_comments",
                json!({
                    "repo_path": insert.repo_path,
                    "base_branch": insert.base_branch,
                    "comments": comments,
                }),
            )
        }
        tool_call::Tool::Remember(remember) => (
            "remember",
            json!({
                "content": remember.content,
                "scope": match remember.scope() {
                    tool_call::remember::Scope::Global => "global",
                    tool_call::remember::Scope::Project => "project",
                },
            }),
        ),
        // A tool that this crate never offers cannot appear in a local conversation, but a
        // history from a cloud conversation may hold one. Leaving it out keeps the turn list
        // valid; the matching result is rendered as plain text below.
        _ => return None,
    };

    Some(ToolUse {
        id: call.tool_call_id.clone(),
        name: name.to_string(),
        arguments,
    })
}

fn review_comment_to_json(comment: &tool_call::insert_review_comments::Comment) -> Value {
    let mut entry = json!({
        "id": comment.comment_id,
        "author": comment.author,
        "body": comment.comment_body,
        "updated_at": comment.last_modified_timestamp,
    });
    if !comment.parent_comment_id.is_empty() {
        entry["in_reply_to_id"] = json!(comment.parent_comment_id);
    }
    if !comment.html_url.is_empty() {
        entry["html_url"] = json!(comment.html_url);
    }
    if let Some(location) = comment.location.as_ref() {
        entry["path"] = json!(location.file_path);
        if let Some(line) = location.line.as_ref() {
            entry["diff_hunk"] = json!(line.diff_hunk);
            if let Some(range) = line.range.as_ref() {
                entry["start_line"] = json!(range.start);
                entry["line"] = json!(range.end);
            }
            entry["side"] = json!(match line.side() {
                tool_call::insert_review_comments::CommentSide::New => "RIGHT",
                tool_call::insert_review_comments::CommentSide::Old => "LEFT",
            });
        }
    }
    entry
}

/// Renders a tool result from the conversation history.
pub fn render_result(result: &message::ToolCallResult) -> ToolResult {
    use tool_call_result::Result as ProtoResult;

    let (content, is_error) = match result.result.as_ref() {
        Some(ProtoResult::RunShellCommand(shell)) => render_shell(shell),
        Some(ProtoResult::ReadFiles(read)) => render_read_files(read),
        Some(ProtoResult::ApplyFileDiffs(apply)) => render_apply_diffs(apply),
        Some(ProtoResult::Grep(grep)) => render_grep(grep),
        Some(ProtoResult::FileGlobV2(glob)) => render_file_glob(glob),
        Some(ProtoResult::ReadShellCommandOutput(poll)) => render_shell_poll(poll),
        Some(ProtoResult::WriteToLongRunningShellCommand(write)) => render_shell_write(write),
        Some(ProtoResult::TransferShellCommandControlToUser(transfer)) => render_transfer(transfer),
        Some(ProtoResult::InsertReviewComments(insert)) => render_insert_review_comments(insert),
        Some(ProtoResult::CallMcpTool(call)) => render_call_mcp(call),
        Some(ProtoResult::ReadMcpResource(read)) => render_read_mcp(read),
        Some(ProtoResult::Remember(remember)) => render_remember(remember),
        Some(ProtoResult::Cancel(_)) => ("The user cancelled this tool call.".to_string(), true),
        _ => ("The tool returned no readable result.".to_string(), true),
    };

    ToolResult {
        id: result.tool_call_id.clone(),
        content,
        is_error,
    }
}

/// Renders a tool result that the client just sent as new input.
///
/// The result is rendered from its stored form, so that the model sees now exactly what a later
/// request replays from the conversation history.
pub fn render_input_result(result: &api::request::input::ToolCallResult) -> ToolResult {
    render_result(&stored_result(result))
}

/// The form in which a tool result that the client sent is kept in the conversation.
///
/// Text that the model is shown is cut with [`truncate`], and bulk that it is never shown (binary
/// data, the full content of an edited file) is dropped, so that a stored conversation stays small
/// and renders to the same text that the model saw. `context` is left empty, as it is on a stored
/// question.
// The deprecated `FileGlob` result is still carried, because an older client can send it.
#[allow(deprecated)]
pub fn stored_result(input: &api::request::input::ToolCallResult) -> message::ToolCallResult {
    use api::request::input::tool_call_result::Result as In;
    use tool_call_result::Result as Out;

    let result = input.result.clone().map(|result| match result {
        In::RunShellCommand(result) => Out::RunShellCommand(result),
        In::ReadFiles(result) => Out::ReadFiles(result),
        In::SearchCodebase(result) => Out::SearchCodebase(result),
        In::ApplyFileDiffs(result) => Out::ApplyFileDiffs(result),
        In::SuggestPlan(result) => Out::SuggestPlan(result),
        In::SuggestCreatePlan(result) => Out::SuggestCreatePlan(result),
        In::Grep(result) => Out::Grep(result),
        In::FileGlob(result) => Out::FileGlob(result),
        In::ReadMcpResource(result) => Out::ReadMcpResource(result),
        In::CallMcpTool(result) => Out::CallMcpTool(result),
        In::WriteToLongRunningShellCommand(result) => Out::WriteToLongRunningShellCommand(result),
        In::SuggestNewConversation(result) => Out::SuggestNewConversation(result),
        In::FileGlobV2(result) => Out::FileGlobV2(result),
        In::SuggestPrompt(result) => Out::SuggestPrompt(result),
        In::OpenCodeReview(result) => Out::OpenCodeReview(result),
        In::InitProject(result) => Out::InitProject(result),
        In::ReadDocuments(result) => Out::ReadDocuments(result),
        In::EditDocuments(result) => Out::EditDocuments(result),
        In::CreateDocuments(result) => Out::CreateDocuments(result),
        In::ReadShellCommandOutput(result) => Out::ReadShellCommandOutput(result),
        In::UseComputer(result) => Out::UseComputer(result),
        In::InsertReviewComments(result) => Out::InsertReviewComments(result),
        In::RequestComputerUse(result) => Out::RequestComputerUseResult(result),
        In::ReadSkill(result) => Out::ReadSkill(result),
        In::FetchConversation(result) => Out::FetchConversation(result),
        In::SendMessageToAgent(result) => Out::SendMessageToAgent(result),
        In::TransferShellCommandControlToUser(result) => {
            Out::TransferShellCommandControlToUser(result)
        }
        In::AskUserQuestion(result) => Out::AskUserQuestion(result),
        In::UploadFileArtifact(result) => Out::UploadFileArtifact(result),
        In::RunAgentsResult(result) => Out::RunAgentsResult(result),
        In::WaitForEvents(result) => Out::WaitForEvents(result),
        In::StartRecording(result) => Out::StartRecording(result),
        In::StopRecording(result) => Out::StopRecording(result),
        In::Remember(result) => Out::Remember(result),
    });

    let mut stored = message::ToolCallResult {
        tool_call_id: input.tool_call_id.clone(),
        context: None,
        result,
    };
    if let Some(result) = stored.result.as_mut() {
        fit_payload(result);
    }
    stored
}

/// Cuts what the model is shown to [`MAX_RESULT_CHARS`] and drops what it is never shown.
#[allow(deprecated)]
fn fit_payload(result: &mut tool_call_result::Result) {
    use tool_call_result::Result as Out;

    match result {
        Out::RunShellCommand(shell) => {
            use api::run_shell_command_result::Result as Shell;
            shell.output = truncate(&shell.output);
            match shell.result.as_mut() {
                Some(Shell::CommandFinished(finished)) => fit_finished(finished),
                Some(Shell::LongRunningCommandSnapshot(snapshot)) => fit_snapshot(snapshot),
                Some(Shell::PermissionDenied(_)) | None => {}
            }
        }
        Out::ReadShellCommandOutput(poll) => {
            use api::read_shell_command_output_result::Result as Poll;
            match poll.result.as_mut() {
                Some(Poll::CommandFinished(finished)) => fit_finished(finished),
                Some(Poll::LongRunningCommandSnapshot(snapshot)) => fit_snapshot(snapshot),
                Some(Poll::Error(_)) | None => {}
            }
        }
        Out::WriteToLongRunningShellCommand(write) => {
            use api::write_to_long_running_shell_command_result::Result as Write;
            match write.result.as_mut() {
                Some(Write::CommandFinished(finished)) => fit_finished(finished),
                Some(Write::LongRunningCommandSnapshot(snapshot)) => fit_snapshot(snapshot),
                Some(Write::Error(_)) | None => {}
            }
        }
        Out::TransferShellCommandControlToUser(transfer) => {
            use api::transfer_shell_command_control_to_user_result::Result as Transfer;
            match transfer.result.as_mut() {
                Some(Transfer::CommandFinished(finished)) => fit_finished(finished),
                Some(Transfer::LongRunningCommandSnapshot(snapshot)) => fit_snapshot(snapshot),
                Some(Transfer::Error(_)) | None => {}
            }
        }
        Out::ReadFiles(read) => {
            use api::any_file_content::Content;
            use api::read_files_result::Result as Read;
            match read.result.as_mut() {
                Some(Read::TextFilesSuccess(success)) => {
                    for file in &mut success.files {
                        file.content = truncate(&file.content);
                    }
                }
                Some(Read::AnyFilesSuccess(success)) => {
                    for file in &mut success.files {
                        match file.content.as_mut() {
                            Some(Content::TextContent(text)) => {
                                text.content = truncate(&text.content);
                            }
                            Some(Content::BinaryContent(binary)) => binary.data.clear(),
                            None => {}
                        }
                    }
                }
                Some(Read::Error(error)) => error.message = truncate(&error.message),
                None => {}
            }
        }
        // The model is shown only the paths of an edit, so the changed lines and their context
        // that the client attaches are not kept.
        Out::ApplyFileDiffs(apply) => {
            use api::apply_file_diffs_result::Result as Apply;
            match apply.result.as_mut() {
                Some(Apply::Success(success)) => {
                    for file in &mut success.updated_files {
                        file.content.clear();
                    }
                    for updated in &mut success.updated_files_v2 {
                        if let Some(file) = updated.file.as_mut() {
                            file.content.clear();
                        }
                    }
                }
                Some(Apply::Error(error)) => error.message = truncate(&error.message),
                None => {}
            }
        }
        Out::Grep(grep) => {
            use api::grep_result::Result as Grep;
            match grep.result.as_mut() {
                Some(Grep::Success(success)) => {
                    for file in &mut success.matched_files {
                        fit_list(&mut file.matched_lines, |line| {
                            line.line_number.to_string().len() + 1
                        });
                    }
                    fit_list(&mut success.matched_files, |file| {
                        file.file_path.chars().count()
                            + file
                                .matched_lines
                                .iter()
                                .map(|line| line.line_number.to_string().len() + 2)
                                .sum::<usize>()
                            + ": lines ".len()
                    });
                }
                Some(Grep::Error(error)) => error.message = truncate(&error.message),
                None => {}
            }
        }
        Out::FileGlobV2(glob) => {
            use api::file_glob_v2_result::Result as Glob;
            match glob.result.as_mut() {
                Some(Glob::Success(success)) => {
                    success.warnings = truncate(&success.warnings);
                    fit_list(&mut success.matched_files, |file| {
                        file.file_path.chars().count()
                    });
                }
                Some(Glob::Error(error)) => error.message = truncate(&error.message),
                None => {}
            }
        }
        Out::InsertReviewComments(insert) => {
            use api::insert_review_comments_result::Result as Insert;
            insert.repo_path = truncate(&insert.repo_path);
            match insert.result.as_mut() {
                Some(Insert::Error(error)) => error.message = truncate(&error.message),
                Some(Insert::Success(_)) | None => {}
            }
        }
        Out::CallMcpTool(call) => {
            use api::call_mcp_tool_result::Result as Call;
            use api::call_mcp_tool_result::success::result::Result as Item;
            match call.result.as_mut() {
                Some(Call::Success(success)) => {
                    for item in success
                        .results
                        .iter_mut()
                        .filter_map(|item| item.result.as_mut())
                    {
                        match item {
                            Item::Text(text) => text.text = truncate(&text.text),
                            Item::Image(image) => image.data.clear(),
                            Item::Resource(resource) => fit_mcp_content(resource),
                        }
                    }
                }
                Some(Call::Error(error)) => error.message = truncate(&error.message),
                None => {}
            }
        }
        Out::ReadMcpResource(read) => {
            use api::read_mcp_resource_result::Result as Read;
            match read.result.as_mut() {
                Some(Read::Success(success)) => {
                    success.contents.iter_mut().for_each(fit_mcp_content)
                }
                Some(Read::Error(error)) => error.message = truncate(&error.message),
                None => {}
            }
        }
        Out::SearchCodebase(_)
        | Out::Server(_)
        | Out::SuggestPlan(_)
        | Out::SuggestCreatePlan(_)
        | Out::FileGlob(_)
        | Out::Cancel(_)
        | Out::SuggestNewConversation(_)
        | Out::SuggestPrompt(_)
        | Out::OpenCodeReview(_)
        | Out::InitProject(_)
        | Out::Subagent(_)
        | Out::ReadDocuments(_)
        | Out::EditDocuments(_)
        | Out::CreateDocuments(_)
        | Out::UseComputer(_)
        | Out::ReadSkill(_)
        | Out::RequestComputerUseResult(_)
        | Out::FetchConversation(_)
        | Out::SendMessageToAgent(_)
        | Out::AskUserQuestion(_)
        | Out::UploadFileArtifact(_)
        | Out::RunAgentsResult(_)
        | Out::WaitForEvents(_)
        | Out::StartRecording(_)
        | Out::StopRecording(_)
        | Out::Remember(_) => {}
    }
}

/// Drops entries from the middle of a list that is rendered one entry per line, when its text
/// would be cut by [`truncate`] anyway.
///
/// Enough entries are kept at each end that the kept text is still longer than
/// [`MAX_RESULT_CHARS`] and starts and ends with the same characters, so it is cut to the same
/// head and tail; only the count in the marker is smaller. `rendered_len` is an entry's length in
/// characters.
fn fit_list<T>(items: &mut Vec<T>, rendered_len: impl Fn(&T) -> usize) {
    // One more character for the separator between entries.
    let len = |item: &T| rendered_len(item) + 1;
    if items.iter().map(len).sum::<usize>() <= 2 * MAX_RESULT_CHARS {
        return;
    }
    let mut head = 0;
    let mut head_chars = 0;
    while head < items.len() && head_chars < 2 * KEEP_HEAD_CHARS {
        head_chars += len(&items[head]);
        head += 1;
    }
    let mut tail = items.len();
    let mut tail_chars = 0;
    while tail > head && tail_chars < KEEP_HEAD_CHARS + KEEP_TAIL_CHARS {
        tail -= 1;
        tail_chars += len(&items[tail]);
    }
    items.drain(head..tail);
}

fn fit_finished(finished: &mut api::ShellCommandFinished) {
    finished.output = truncate(&finished.output);
}

fn fit_snapshot(snapshot: &mut api::LongRunningShellCommandSnapshot) {
    snapshot.output = truncate(&snapshot.output);
}

fn fit_mcp_content(content: &mut api::McpResourceContent) {
    use api::mcp_resource_content::ContentType;

    match content.content_type.as_mut() {
        Some(ContentType::Text(text)) => text.content = truncate(&text.content),
        Some(ContentType::Binary(binary)) => binary.data.clear(),
        None => {}
    }
}

// The flat `output` and `exit_code` fields are deprecated, but they are the only fields that an
// older client fills in, so the fallback arm still reads them.
#[allow(deprecated)]
fn render_shell(shell: &api::RunShellCommandResult) -> (String, bool) {
    use api::run_shell_command_result::Result as ShellResult;

    match shell.result.as_ref() {
        Some(ShellResult::CommandFinished(finished)) => render_finished(finished),
        Some(ShellResult::LongRunningCommandSnapshot(snapshot)) => {
            (render_snapshot(snapshot), false)
        }
        Some(ShellResult::PermissionDenied(_)) => (
            "The command was not allowed to run. If another command is still running, the \
             client refuses to start a new one: poll that command with \
             read_shell_command_output instead."
                .to_string(),
            true,
        ),
        None => (
            // Older clients set only the deprecated flat fields.
            format!(
                "exit code: {}\n\n{}",
                shell.exit_code,
                truncate(&shell.output)
            ),
            shell.exit_code != 0,
        ),
    }
}

/// Renders a poll of a running command.
fn render_shell_poll(poll: &api::ReadShellCommandOutputResult) -> (String, bool) {
    use api::read_shell_command_output_result::Result as PollResult;

    match poll.result.as_ref() {
        Some(PollResult::CommandFinished(finished)) => render_finished(finished),
        Some(PollResult::LongRunningCommandSnapshot(snapshot)) => {
            (render_snapshot(snapshot), false)
        }
        Some(PollResult::Error(_)) | None => (
            "No running command with that id. It may have finished and its output was \
             already returned; run the next command."
                .to_string(),
            true,
        ),
    }
}

/// Renders a write to a running command. The result carries the same shapes as a poll.
fn render_shell_write(write: &api::WriteToLongRunningShellCommandResult) -> (String, bool) {
    use api::write_to_long_running_shell_command_result::Result as WriteResult;

    match write.result.as_ref() {
        Some(WriteResult::CommandFinished(finished)) => render_finished(finished),
        Some(WriteResult::LongRunningCommandSnapshot(snapshot)) => {
            (render_snapshot(snapshot), false)
        }
        Some(WriteResult::Error(_)) | None => (
            "The write failed: no running command with that id.".to_string(),
            true,
        ),
    }
}

/// Renders the state of a command after the user had control of it.
fn render_transfer(transfer: &api::TransferShellCommandControlToUserResult) -> (String, bool) {
    use api::transfer_shell_command_control_to_user_result::Result as TransferResult;

    match transfer.result.as_ref() {
        Some(TransferResult::CommandFinished(finished)) => render_finished(finished),
        Some(TransferResult::LongRunningCommandSnapshot(snapshot)) => (
            format!(
                "The user handed control back. {}",
                render_snapshot(snapshot)
            ),
            false,
        ),
        Some(TransferResult::Error(_)) | None => (
            "No command is running, so there was nothing to hand to the user.".to_string(),
            true,
        ),
    }
}

fn render_insert_review_comments(insert: &api::InsertReviewCommentsResult) -> (String, bool) {
    use api::insert_review_comments_result::Result as InsertResult;

    match insert.result.as_ref() {
        Some(InsertResult::Success(_)) => (
            format!(
                "The comments are shown in the code review panel for {}.",
                insert.repo_path
            ),
            false,
        ),
        Some(InsertResult::Error(error)) => (error.message.clone(), true),
        None => ("The comments were not inserted.".to_string(), true),
    }
}

fn render_remember(remember: &api::RememberResult) -> (String, bool) {
    use api::remember_result::Result as RememberResult;

    match remember.result.as_ref() {
        Some(RememberResult::Success(success)) if success.already_saved => (
            "This is already in memory; nothing new was saved.".to_string(),
            false,
        ),
        Some(RememberResult::Success(success)) if success.project_root.is_empty() => {
            ("Saved to memory for all projects.".to_string(), false)
        }
        Some(RememberResult::Success(success)) => (
            format!(
                "Saved to memory for the project at `{}`.",
                success.project_root
            ),
            false,
        ),
        Some(RememberResult::Error(error)) => (error.message.clone(), true),
        None => ("Nothing was saved to memory.".to_string(), true),
    }
}

/// The text for a command that is still running. The command id is what lets the model poll
/// again, so it must always be part of the output.
fn render_snapshot(snapshot: &api::LongRunningShellCommandSnapshot) -> String {
    format!(
        "The command is still running (command id: {}). Latest output:\n\n{}",
        snapshot.command_id,
        truncate(&snapshot.output)
    )
}

fn render_finished(finished: &api::ShellCommandFinished) -> (String, bool) {
    (
        format!(
            "exit code: {}\n\n{}",
            finished.exit_code,
            truncate(&finished.output)
        ),
        finished.exit_code != 0,
    )
}

fn render_read_files(read: &api::ReadFilesResult) -> (String, bool) {
    use api::read_files_result::Result as ReadResult;

    match read.result.as_ref() {
        Some(ReadResult::TextFilesSuccess(success)) => {
            let mut text = String::new();
            for file in &success.files {
                text.push_str(&format!("--- {} ---\n{}\n", file.file_path, file.content));
            }
            for failed in &success.failed_reads {
                text.push_str(&format!(
                    "--- {} ---\ncould not read: {}\n",
                    failed.path, failed.message
                ));
            }
            (truncate(&text), false)
        }
        // The client sends every read in this form, text files included.
        Some(ReadResult::AnyFilesSuccess(success)) => {
            use api::any_file_content::Content;

            let mut text = String::new();
            for file in &success.files {
                match file.content.as_ref() {
                    Some(Content::TextContent(file)) => {
                        text.push_str(&format!("--- {} ---\n{}\n", file.file_path, file.content));
                    }
                    Some(Content::BinaryContent(file)) => {
                        text.push_str(&format!(
                            "--- {} ---\nThe file is not text, so it cannot be shown.\n",
                            file.file_path
                        ));
                    }
                    None => {}
                }
            }
            for failed in &success.failed_reads {
                text.push_str(&format!(
                    "--- {} ---\ncould not read: {}\n",
                    failed.path, failed.message
                ));
            }
            (truncate(&text), false)
        }
        Some(ReadResult::Error(error)) => (error.message.clone(), true),
        None => ("No files were read.".to_string(), true),
    }
}

fn render_apply_diffs(apply: &api::ApplyFileDiffsResult) -> (String, bool) {
    use api::apply_file_diffs_result::Result as ApplyResult;

    match apply.result.as_ref() {
        Some(ApplyResult::Success(success)) => {
            let mut lines = Vec::new();
            for updated in &success.updated_files_v2 {
                if let Some(file) = updated.file.as_ref() {
                    lines.push(format!("changed {}", file.file_path));
                }
            }
            for deleted in &success.deleted_files {
                lines.push(format!("deleted {}", deleted.file_path));
            }
            if lines.is_empty() {
                lines.push("The change was applied.".to_string());
            }
            (lines.join("\n"), false)
        }
        Some(ApplyResult::Error(error)) => (error.message.clone(), true),
        None => ("The change was not applied.".to_string(), true),
    }
}

fn render_grep(grep: &api::GrepResult) -> (String, bool) {
    use api::grep_result::Result as GrepResultKind;

    match grep.result.as_ref() {
        Some(GrepResultKind::Success(success)) => {
            if success.matched_files.is_empty() {
                return ("No file matched.".to_string(), false);
            }
            let text = success
                .matched_files
                .iter()
                .map(|file| {
                    let lines = file
                        .matched_lines
                        .iter()
                        .map(|line| line.line_number.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{}: lines {}", file.file_path, lines)
                })
                .collect::<Vec<_>>()
                .join("\n");
            (truncate(&text), false)
        }
        Some(GrepResultKind::Error(error)) => (error.message.clone(), true),
        None => ("The search returned nothing.".to_string(), true),
    }
}

fn render_file_glob(glob: &api::FileGlobV2Result) -> (String, bool) {
    use api::file_glob_v2_result::Result as GlobResult;

    match glob.result.as_ref() {
        Some(GlobResult::Success(success)) => {
            if success.matched_files.is_empty() {
                return ("No file matched.".to_string(), false);
            }
            let text = success
                .matched_files
                .iter()
                .map(|file| file.file_path.clone())
                .collect::<Vec<_>>()
                .join("\n");
            (truncate(&text), false)
        }
        Some(GlobResult::Error(error)) => (error.message.clone(), true),
        None => ("The search returned nothing.".to_string(), true),
    }
}

fn render_call_mcp(call: &api::CallMcpToolResult) -> (String, bool) {
    use api::call_mcp_tool_result::Result as CallResult;
    use api::call_mcp_tool_result::success::result::Result as Item;

    match call.result.as_ref() {
        Some(CallResult::Success(success)) => {
            if success.results.is_empty() {
                return ("The tool returned nothing.".to_string(), false);
            }
            let text = success
                .results
                .iter()
                .filter_map(|result| result.result.as_ref())
                .map(|item| match item {
                    Item::Text(text) => text.text.clone(),
                    Item::Image(image) => format!("[image: {}]", image.mime_type),
                    Item::Resource(resource) => render_mcp_content(resource),
                })
                .collect::<Vec<_>>()
                .join("\n");
            (truncate(&text), false)
        }
        Some(CallResult::Error(error)) => (error.message.clone(), true),
        None => ("The tool returned nothing.".to_string(), true),
    }
}

fn render_read_mcp(read: &api::ReadMcpResourceResult) -> (String, bool) {
    use api::read_mcp_resource_result::Result as ReadResult;

    match read.result.as_ref() {
        Some(ReadResult::Success(success)) => {
            if success.contents.is_empty() {
                return ("The resource is empty.".to_string(), false);
            }
            let text = success
                .contents
                .iter()
                .map(render_mcp_content)
                .collect::<Vec<_>>()
                .join("\n");
            (truncate(&text), false)
        }
        Some(ReadResult::Error(error)) => (error.message.clone(), true),
        None => ("The resource returned nothing.".to_string(), true),
    }
}

/// Text content is shown as it is. Binary content cannot be shown to the model as text, so it is
/// named instead.
fn render_mcp_content(content: &api::McpResourceContent) -> String {
    use api::mcp_resource_content::ContentType;

    match content.content_type.as_ref() {
        Some(ContentType::Text(text)) => text.content.clone(),
        Some(ContentType::Binary(binary)) => {
            format!("[binary resource {}: {}]", content.uri, binary.mime_type)
        }
        None => format!("[resource {} had no content]", content.uri),
    }
}

/// The most characters of one tool output that the model is shown and the conversation keeps.
///
/// Every stored result is replayed on every later request, so this bounds how fast a long agent
/// loop fills the context window as well as how large a stored conversation grows.
pub const MAX_RESULT_CHARS: usize = 16_000;

/// The characters kept from the start of a cut output: command echoes, file headers, usage text.
const KEEP_HEAD_CHARS: usize = 4_000;

/// The characters kept from the end of a cut output, which is where a command reports its error.
///
/// Head, tail and the marker between them stay within [`MAX_RESULT_CHARS`], so cutting an output
/// that was already cut leaves it unchanged.
const KEEP_TAIL_CHARS: usize = 11_800;

/// Cuts text longer than [`MAX_RESULT_CHARS`] to its head and tail, with a marker that says how
/// much was left out of the middle.
pub fn truncate(text: &str) -> String {
    let total = text.chars().count();
    if total <= MAX_RESULT_CHARS {
        return text.to_string();
    }
    let head_end = text
        .char_indices()
        .nth(KEEP_HEAD_CHARS)
        .map_or(text.len(), |(index, _)| index);
    let tail_start = text
        .char_indices()
        .nth(total - KEEP_TAIL_CHARS)
        .map_or(text.len(), |(index, _)| index);
    let cut = total - KEEP_HEAD_CHARS - KEEP_TAIL_CHARS;
    format!(
        "{}\n\n[... {cut} characters were cut from the middle of this output. Read a narrower range \
         or filter the output to see them.]\n\n{}",
        &text[..head_end],
        &text[tail_start..]
    )
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
