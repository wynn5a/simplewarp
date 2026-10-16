//! The tool set that the local agent offers to the model.
//!
//! Warp's client already runs every one of these tools. The server used to decide which one to
//! call; now the model does, and this module is the contract between the two. Each tool has a
//! JSON schema for the provider, and a pair of converters between the model's JSON arguments and
//! the proto [`api::message::ToolCall`] that the client applies.

use std::borrow::Cow;

use serde_json::{Value, json};
use warp_multi_agent_api as api;
use warp_multi_agent_api::message::tool_call;
use warp_multi_agent_api::message::tool_call::insert_review_comments;

/// The tools that this crate can translate. The client may support more; anything outside this
/// list is left out of the request, so the model never calls a tool that we cannot map.
pub const SUPPORTED: [api::ToolType; 9] = [
    api::ToolType::RunShellCommand,
    api::ToolType::ReadFiles,
    api::ToolType::ApplyFileDiffs,
    api::ToolType::Grep,
    api::ToolType::FileGlobV2,
    api::ToolType::ReadShellCommandOutput,
    api::ToolType::WriteToLongRunningShellCommand,
    api::ToolType::TransferShellCommandControlToUser,
    api::ToolType::InsertReviewComments,
];

/// A tool as the provider sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSchema {
    pub name: Cow<'static, str>,
    pub description: Cow<'static, str>,
    /// A JSON Schema object that describes the arguments.
    pub input_schema: Value,
}

/// A built-in tool, whose name and description are fixed text.
struct Builtin {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
}

impl From<Builtin> for ToolSchema {
    fn from(tool: Builtin) -> Self {
        Self {
            name: tool.name.into(),
            description: tool.description.into(),
            input_schema: tool.input_schema,
        }
    }
}

/// The wire name that the model uses for a tool type.
pub fn wire_name(tool: api::ToolType) -> Option<&'static str> {
    Some(match tool {
        api::ToolType::RunShellCommand => "run_shell_command",
        api::ToolType::ReadFiles => "read_files",
        api::ToolType::ApplyFileDiffs => "apply_file_diffs",
        api::ToolType::Grep => "grep",
        api::ToolType::FileGlobV2 => "file_glob",
        api::ToolType::ReadShellCommandOutput => "read_shell_command_output",
        api::ToolType::WriteToLongRunningShellCommand => "write_to_long_running_shell_command",
        api::ToolType::TransferShellCommandControlToUser => "transfer_control_to_user",
        api::ToolType::InsertReviewComments => "insert_review_comments",
        _ => return None,
    })
}

/// Builds the tool list for a request.
///
/// `client_supported` is `Settings::supported_tools`. An empty list means "the client accepts
/// any tool", which matches the server contract, so in that case we offer everything we can map.
pub fn schemas_for(client_supported: &[i32]) -> Vec<ToolSchema> {
    SUPPORTED
        .iter()
        .filter(|tool| client_supported.is_empty() || client_supported.contains(&(**tool as i32)))
        .filter_map(|tool| schema_for(*tool))
        .collect()
}

fn schema_for(tool: api::ToolType) -> Option<ToolSchema> {
    let schema = match tool {
        api::ToolType::RunShellCommand => Builtin {
            name: "run_shell_command",
            description: "Run a shell command on the user's machine and read its output. \
                Prefer a single command over a long pipeline. Mark read-only commands so that \
                they can run without asking the user.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command to run, exactly as it must be typed.",
                    },
                    "is_read_only": {
                        "type": "boolean",
                        "description": "True if the command only reads state and changes nothing.",
                    },
                },
                "required": ["command"],
            }),
        },
        api::ToolType::ReadFiles => Builtin {
            name: "read_files",
            description: "Read one or more files. Give a line range to read only part of a \
                large file; leave the range out to read the whole file.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "files": {
                        "type": "array",
                        "description": "The files to read.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "start_line": {
                                    "type": "integer",
                                    "description": "First line to read, 1-based.",
                                },
                                "end_line": {
                                    "type": "integer",
                                    "description": "Last line to read, inclusive.",
                                },
                            },
                            "required": ["path"],
                        },
                    },
                },
                "required": ["files"],
            }),
        },
        api::ToolType::ApplyFileDiffs => Builtin {
            name: "apply_file_diffs",
            description: "Change files. Use `diffs` to replace an exact block of text, \
                `new_files` to create a file, and `deleted_files` to remove one. The `search` \
                text must match the file exactly, including indentation.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "One short line that tells the user what changed.",
                    },
                    "diffs": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "file_path": { "type": "string" },
                                "search": {
                                    "type": "string",
                                    "description": "The exact text to replace.",
                                },
                                "replace": {
                                    "type": "string",
                                    "description": "The text that replaces it.",
                                },
                            },
                            "required": ["file_path", "search", "replace"],
                        },
                    },
                    "new_files": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "file_path": { "type": "string" },
                                "content": { "type": "string" },
                            },
                            "required": ["file_path", "content"],
                        },
                    },
                    "deleted_files": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": { "file_path": { "type": "string" } },
                            "required": ["file_path"],
                        },
                    },
                },
                "required": ["summary"],
            }),
        },
        api::ToolType::Grep => Builtin {
            name: "grep",
            description: "Search the contents of files for a pattern. Use this to find where \
                something is written; use file_glob to find files by name.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "queries": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "The patterns to look for.",
                    },
                    "path": {
                        "type": "string",
                        "description": "The file or directory to search, relative to the \
                            working directory.",
                    },
                },
                "required": ["queries"],
            }),
        },
        api::ToolType::FileGlobV2 => Builtin {
            name: "file_glob",
            description: "Find files by name pattern. Supports ?, *, and [].",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "patterns": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "The name patterns to match, for example `*.rs`.",
                    },
                    "search_dir": {
                        "type": "string",
                        "description": "The directory to search, relative to the working \
                            directory.",
                    },
                    "max_matches": {
                        "type": "integer",
                        "description": "Stop after this many matches. Leave out for no limit.",
                    },
                },
                "required": ["patterns"],
            }),
        },
        api::ToolType::ReadShellCommandOutput => Builtin {
            name: "read_shell_command_output",
            description: "Read the output of a command that is still running, given the \
                command id from the still-running result. This is the way to wait for a long \
                command; the client will not start a new command while one is running.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command_id": {
                        "type": "string",
                        "description": "The id of the running command, taken from the \
                            still-running result.",
                    },
                    "max_wait_seconds": {
                        "type": "integer",
                        "description": "Wait up to this many seconds before returning. Leave \
                            out to check right away.",
                    },
                    "wait_for_completion": {
                        "type": "boolean",
                        "description": "True to wait for the command to finish instead of \
                            returning a snapshot. The client caps how long it waits.",
                    },
                },
                "required": ["command_id"],
            }),
        },
        api::ToolType::WriteToLongRunningShellCommand => Builtin {
            name: "write_to_long_running_shell_command",
            description: "Write text to a command that is still running, for example to \
                answer a yes/no prompt. Returns the output after the write.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command_id": {
                        "type": "string",
                        "description": "The id of the running command, taken from the \
                            still-running result.",
                    },
                    "input": {
                        "type": "string",
                        "description": "The text to write.",
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["raw", "line", "block"],
                        "description": "`line` (the default) writes the text and presses \
                            Enter. `block` writes multi-line text as one paste. `raw` writes \
                            the exact bytes.",
                    },
                },
                "required": ["command_id", "input"],
            }),
        },
        api::ToolType::TransferShellCommandControlToUser => Builtin {
            name: "transfer_control_to_user",
            description: "Hand the running command to the user when it needs input that only \
                they can give, such as a password, a one-time code, or a choice in an \
                interactive screen. The user types into the command and then hands control \
                back, or the command finishes. Returns the command's output at that point.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "reason": {
                        "type": "string",
                        "description": "One short line that tells the user what to do, for \
                            example `Enter your sudo password`.",
                    },
                },
                "required": ["reason"],
            }),
        },
        api::ToolType::InsertReviewComments => Builtin {
            name: "insert_review_comments",
            description: "Show pull request review comments in the user's code review panel, \
                attached to the lines they are about, so the user can read and address them. \
                Fetch the comments first, for example with `gh api \
                repos/OWNER/REPO/pulls/NUMBER/comments`, and pass them on as they are.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "repo_path": {
                        "type": "string",
                        "description": "The absolute path of the repository root.",
                    },
                    "base_branch": {
                        "type": "string",
                        "description": "The branch the pull request merges into.",
                    },
                    "comments": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "author": { "type": "string" },
                                "body": { "type": "string" },
                                "updated_at": {
                                    "type": "string",
                                    "description": "RFC 3339 time of the last change.",
                                },
                                "in_reply_to_id": {
                                    "type": "string",
                                    "description": "The id of the comment this one replies to.",
                                },
                                "html_url": { "type": "string" },
                                "path": {
                                    "type": "string",
                                    "description": "The file, relative to the repository \
                                        root. Leave out for a comment on the whole pull \
                                        request.",
                                },
                                "diff_hunk": {
                                    "type": "string",
                                    "description": "The diff hunk the comment is on.",
                                },
                                "line": {
                                    "type": "integer",
                                    "description": "The last line the comment is on. Leave \
                                        out for a comment on the whole file.",
                                },
                                "start_line": {
                                    "type": "integer",
                                    "description": "The first line of a multi-line comment.",
                                },
                                "side": {
                                    "type": "string",
                                    "enum": ["RIGHT", "LEFT"],
                                    "description": "RIGHT for the new version of the file \
                                        (the default), LEFT for the old one.",
                                },
                            },
                            "required": ["id", "author", "body", "updated_at"],
                        },
                    },
                },
                "required": ["repo_path", "comments"],
            }),
        },
        _ => return None,
    };
    Some(schema.into())
}

/// Turns the model's JSON arguments into the proto tool call that the client runs.
///
/// Returns `None` when the tool name is not one that we offered, so that a model which invents a
/// tool cannot make us build a malformed call.
pub fn to_proto(
    tool_call_id: &str,
    name: &str,
    arguments: &Value,
) -> Option<api::message::ToolCall> {
    let tool = match name {
        "run_shell_command" => tool_call::Tool::RunShellCommand(tool_call::RunShellCommand {
            command: string_field(arguments, "command")?,
            is_read_only: bool_field(arguments, "is_read_only"),
            ..Default::default()
        }),
        "read_files" => {
            let files = array_field(arguments, "files")
                .iter()
                .filter_map(|entry| {
                    let name = string_field(entry, "path")?;
                    let start = u32_field(entry, "start_line");
                    let end = u32_field(entry, "end_line");
                    let line_ranges = match (start, end) {
                        (Some(start), Some(end)) => {
                            vec![api::FileContentLineRange { start, end }]
                        }
                        _ => Vec::new(),
                    };
                    Some(tool_call::read_files::File { name, line_ranges })
                })
                .collect::<Vec<_>>();
            if files.is_empty() {
                return None;
            }
            tool_call::Tool::ReadFiles(tool_call::ReadFiles { files })
        }
        "apply_file_diffs" => {
            let diffs = array_field(arguments, "diffs")
                .iter()
                .filter_map(|entry| {
                    Some(tool_call::apply_file_diffs::FileDiff {
                        file_path: string_field(entry, "file_path")?,
                        search: string_field(entry, "search")?,
                        replace: string_field(entry, "replace").unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>();
            let new_files = array_field(arguments, "new_files")
                .iter()
                .filter_map(|entry| {
                    Some(tool_call::apply_file_diffs::NewFile {
                        file_path: string_field(entry, "file_path")?,
                        content: string_field(entry, "content").unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>();
            let deleted_files = array_field(arguments, "deleted_files")
                .iter()
                .filter_map(|entry| {
                    Some(tool_call::apply_file_diffs::DeleteFile {
                        file_path: string_field(entry, "file_path")?,
                    })
                })
                .collect::<Vec<_>>();
            if diffs.is_empty() && new_files.is_empty() && deleted_files.is_empty() {
                return None;
            }
            tool_call::Tool::ApplyFileDiffs(tool_call::ApplyFileDiffs {
                summary: string_field(arguments, "summary").unwrap_or_default(),
                diffs,
                new_files,
                deleted_files,
                ..Default::default()
            })
        }
        "grep" => {
            let queries = string_array_field(arguments, "queries");
            if queries.is_empty() {
                return None;
            }
            tool_call::Tool::Grep(tool_call::Grep {
                queries,
                path: string_field(arguments, "path").unwrap_or_default(),
            })
        }
        "file_glob" => {
            let patterns = string_array_field(arguments, "patterns");
            if patterns.is_empty() {
                return None;
            }
            tool_call::Tool::FileGlobV2(tool_call::FileGlobV2 {
                patterns,
                search_dir: string_field(arguments, "search_dir").unwrap_or_default(),
                max_matches: u32_field(arguments, "max_matches").unwrap_or_default() as i32,
                ..Default::default()
            })
        }
        "read_shell_command_output" => {
            let delay = if bool_field(arguments, "wait_for_completion") {
                Some(tool_call::read_shell_command_output::Delay::OnCompletion(()))
            } else {
                u32_field(arguments, "max_wait_seconds").map(|seconds| {
                    tool_call::read_shell_command_output::Delay::Duration(prost_types::Duration {
                        seconds: i64::from(seconds),
                        nanos: 0,
                    })
                })
            };
            tool_call::Tool::ReadShellCommandOutput(tool_call::ReadShellCommandOutput {
                command_id: string_field(arguments, "command_id")?,
                delay,
            })
        }
        "write_to_long_running_shell_command" => {
            let kind = match string_field(arguments, "mode").as_deref() {
                Some("raw") => tool_call::write_to_long_running_shell_command::mode::Mode::Raw(()),
                Some("block") => {
                    tool_call::write_to_long_running_shell_command::mode::Mode::Block(())
                }
                // The schema documents `line` as the default: a prompt answer usually
                // needs the Enter that `line` sends.
                _ => tool_call::write_to_long_running_shell_command::mode::Mode::Line(()),
            };
            tool_call::Tool::WriteToLongRunningShellCommand(
                tool_call::WriteToLongRunningShellCommand {
                    command_id: string_field(arguments, "command_id")?,
                    input: string_field(arguments, "input")?.into_bytes(),
                    mode: Some(tool_call::write_to_long_running_shell_command::Mode {
                        mode: Some(kind),
                    }),
                },
            )
        }
        "transfer_control_to_user" => tool_call::Tool::TransferShellCommandControlToUser(
            tool_call::TransferShellCommandControlToUser {
                reason: string_field(arguments, "reason").unwrap_or_default(),
            },
        ),
        "insert_review_comments" => {
            let comments = array_field(arguments, "comments")
                .iter()
                .filter_map(review_comment_to_proto)
                .collect::<Vec<_>>();
            if comments.is_empty() {
                return None;
            }
            tool_call::Tool::InsertReviewComments(tool_call::InsertReviewComments {
                repo_path: string_field(arguments, "repo_path")?,
                comments,
                base_branch: string_field(arguments, "base_branch").unwrap_or_default(),
            })
        }
        _ => return None,
    };

    Some(api::message::ToolCall {
        tool_call_id: tool_call_id.to_string(),
        tool: Some(tool),
    })
}

fn review_comment_to_proto(entry: &Value) -> Option<insert_review_comments::Comment> {
    let location = string_field(entry, "path").map(|file_path| {
        let line = u32_field(entry, "line").map(|end| {
            let side = match string_field(entry, "side").as_deref() {
                Some("LEFT") => insert_review_comments::CommentSide::Old,
                _ => insert_review_comments::CommentSide::New,
            };
            insert_review_comments::CommentLineRange {
                diff_hunk: string_field(entry, "diff_hunk").unwrap_or_default(),
                range: Some(api::FileContentLineRange {
                    start: u32_field(entry, "start_line").unwrap_or(end),
                    end,
                }),
                side: side as i32,
            }
        });
        insert_review_comments::CommentLocation { file_path, line }
    });

    Some(insert_review_comments::Comment {
        comment_id: id_field(entry, "id")?,
        author: string_field(entry, "author")?,
        last_modified_timestamp: string_field(entry, "updated_at")?,
        comment_body: string_field(entry, "body")?,
        parent_comment_id: id_field(entry, "in_reply_to_id").unwrap_or_default(),
        location,
        html_url: string_field(entry, "html_url").unwrap_or_default(),
    })
}

/// Reads an id that the model may pass as a string or, copying GitHub's JSON, as a number.
fn id_field(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(id) => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

pub(crate) fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn u32_field(value: &Value, key: &str) -> Option<u32> {
    value.get(key)?.as_u64().map(|number| number as u32)
}

fn array_field<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn string_array_field(value: &Value, key: &str) -> Vec<String> {
    array_field(value, key)
        .iter()
        .filter_map(|entry| entry.as_str().map(str::to_string))
        .collect()
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
