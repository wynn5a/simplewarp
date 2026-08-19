//! Turns the proto conversation into a provider-neutral turn list.
//!
//! A [`api::Request`] carries the whole conversation: every task, every message, plus the new
//! input. Anthropic and OpenAI both want a flat list of turns, so this module flattens the proto
//! into [`Turn`]s that either provider module can render.
//!
//! Tool results are rendered to text here, not in the provider modules, so that both providers
//! show the model the same thing.

use serde_json::{Value, json};
use warp_multi_agent_api as api;
use warp_multi_agent_api::message::{self, tool_call, tool_call_result};

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

/// Flattens a request into a turn list.
///
/// Adjacent messages of the same kind are merged, because a provider rejects two assistant turns
/// in a row, and the proto history splits one agent reply across several messages.
pub fn turns_from_request(request: &api::Request) -> Vec<Turn> {
    let mut turns = Vec::new();

    if let Some(context) = request.task_context.as_ref() {
        for task in &context.tasks {
            for proto_message in &task.messages {
                push_message(&mut turns, proto_message);
            }
        }
    }

    if let Some(input) = request.input.as_ref() {
        push_input(&mut turns, input);
    }

    pair_tool_calls(&mut turns);
    turns
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
/// The client never persists tool results. `agent_tasks` rows hold only the reasoning, the calls,
/// and the agent text, so a replayed history has a call for every result that is missing. The
/// results do arrive for the turn in flight, in `Request::input`, which is why the first reply of
/// a conversation works and the next one fails.
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

// The deprecated `UserQuery` and `ToolCallResult` input variants are still handled, because a
// conversation that an older client started can carry them.
#[allow(deprecated)]
fn push_input(turns: &mut Vec<Turn>, input: &api::request::Input) {
    use api::request::input::Type;
    use api::request::input::user_inputs::user_input::Input as UserInput;

    match input.r#type.as_ref() {
        Some(Type::UserInputs(inputs)) => {
            for entry in &inputs.inputs {
                match entry.input.as_ref() {
                    Some(UserInput::UserQuery(query)) => push_user(turns, &query.query),
                    Some(UserInput::ToolCallResult(result)) => {
                        push_rendered(turns, render_input_result(result));
                    }
                    _ => {}
                }
            }
        }
        Some(Type::UserQuery(query)) => push_user(turns, &query.query),
        Some(Type::ToolCallResult(result)) => push_rendered(turns, render_input_result(result)),
        _ => {}
    }
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
        // Todos, summaries, and server events carry no instruction that the model needs
        // replayed, so they are left out.
        _ => {}
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

/// The part of a tool result that this crate knows how to show the model.
///
/// The proto has two separate `ToolCallResult` messages: one in the conversation history, and
/// one in the request input. They carry the same payloads under different oneofs, so both are
/// narrowed to this before rendering.
enum Payload<'a> {
    Shell(&'a api::RunShellCommandResult),
    ReadFiles(&'a api::ReadFilesResult),
    ApplyDiffs(&'a api::ApplyFileDiffsResult),
    Grep(&'a api::GrepResult),
    FileGlob(&'a api::FileGlobV2Result),
    Cancelled,
    Unsupported,
}

/// Renders a tool result from the conversation history.
pub fn render_result(result: &message::ToolCallResult) -> ToolResult {
    use tool_call_result::Result as ProtoResult;

    let payload = match result.result.as_ref() {
        Some(ProtoResult::RunShellCommand(shell)) => Payload::Shell(shell),
        Some(ProtoResult::ReadFiles(read)) => Payload::ReadFiles(read),
        Some(ProtoResult::ApplyFileDiffs(apply)) => Payload::ApplyDiffs(apply),
        Some(ProtoResult::Grep(grep)) => Payload::Grep(grep),
        Some(ProtoResult::FileGlobV2(glob)) => Payload::FileGlob(glob),
        Some(ProtoResult::Cancel(_)) => Payload::Cancelled,
        _ => Payload::Unsupported,
    };
    finish(&result.tool_call_id, payload)
}

/// Renders a tool result that the client just sent as new input.
pub fn render_input_result(result: &api::request::input::ToolCallResult) -> ToolResult {
    use api::request::input::tool_call_result::Result as InputResult;

    let payload = match result.result.as_ref() {
        Some(InputResult::RunShellCommand(shell)) => Payload::Shell(shell),
        Some(InputResult::ReadFiles(read)) => Payload::ReadFiles(read),
        Some(InputResult::ApplyFileDiffs(apply)) => Payload::ApplyDiffs(apply),
        Some(InputResult::Grep(grep)) => Payload::Grep(grep),
        Some(InputResult::FileGlobV2(glob)) => Payload::FileGlob(glob),
        _ => Payload::Unsupported,
    };
    finish(&result.tool_call_id, payload)
}

fn finish(tool_call_id: &str, payload: Payload<'_>) -> ToolResult {
    let (content, is_error) = match payload {
        Payload::Shell(shell) => render_shell(shell),
        Payload::ReadFiles(read) => render_read_files(read),
        Payload::ApplyDiffs(apply) => render_apply_diffs(apply),
        Payload::Grep(grep) => render_grep(grep),
        Payload::FileGlob(glob) => render_file_glob(glob),
        Payload::Cancelled => ("The user cancelled this tool call.".to_string(), true),
        Payload::Unsupported => ("The tool returned no readable result.".to_string(), true),
    };

    ToolResult {
        id: tool_call_id.to_string(),
        content,
        is_error,
    }
}

// The flat `output` and `exit_code` fields are deprecated, but they are the only fields that an
// older client fills in, so the fallback arm still reads them.
#[allow(deprecated)]
fn render_shell(shell: &api::RunShellCommandResult) -> (String, bool) {
    use api::run_shell_command_result::Result as ShellResult;

    match shell.result.as_ref() {
        Some(ShellResult::CommandFinished(finished)) => {
            let text = format!(
                "exit code: {}\n\n{}",
                finished.exit_code,
                truncate(&finished.output)
            );
            (text, finished.exit_code != 0)
        }
        Some(ShellResult::LongRunningCommandSnapshot(snapshot)) => (
            format!(
                "The command is still running. Latest output:\n\n{}",
                truncate(&snapshot.output)
            ),
            false,
        ),
        Some(ShellResult::PermissionDenied(_)) => {
            ("The user did not let this command run.".to_string(), true)
        }
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
        Some(ReadResult::AnyFilesSuccess(_)) => (
            "The files were read, but their content is not text.".to_string(),
            false,
        ),
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

/// The largest tool result that goes to the model, in bytes.
///
/// A long build log can fill a context window on its own. The tail is kept because the end of a
/// command output usually holds the error.
const MAX_RESULT_BYTES: usize = 32_000;

fn truncate(text: &str) -> String {
    if text.len() <= MAX_RESULT_BYTES {
        return text.to_string();
    }
    let mut start = text.len() - MAX_RESULT_BYTES;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    format!(
        "[{} bytes were cut from the start]\n{}",
        start,
        &text[start..]
    )
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
