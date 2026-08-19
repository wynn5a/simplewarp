//! The tool set that the local agent offers to the model.
//!
//! Warp's client already runs every one of these tools. The server used to decide which one to
//! call; now the model does, and this module is the contract between the two. Each tool has a
//! JSON schema for the provider, and a pair of converters between the model's JSON arguments and
//! the proto [`api::message::ToolCall`] that the client applies.

use serde_json::{Value, json};
use warp_multi_agent_api as api;
use warp_multi_agent_api::message::tool_call;

/// The tools that this crate can translate. The client may support more; anything outside this
/// list is left out of the request, so the model never calls a tool that we cannot map.
pub const SUPPORTED: [api::ToolType; 5] = [
    api::ToolType::RunShellCommand,
    api::ToolType::ReadFiles,
    api::ToolType::ApplyFileDiffs,
    api::ToolType::Grep,
    api::ToolType::FileGlobV2,
];

/// A tool as the provider sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSchema {
    pub name: &'static str,
    pub description: &'static str,
    /// A JSON Schema object that describes the arguments.
    pub input_schema: Value,
}

/// The wire name that the model uses for a tool type.
pub fn wire_name(tool: api::ToolType) -> Option<&'static str> {
    Some(match tool {
        api::ToolType::RunShellCommand => "run_shell_command",
        api::ToolType::ReadFiles => "read_files",
        api::ToolType::ApplyFileDiffs => "apply_file_diffs",
        api::ToolType::Grep => "grep",
        api::ToolType::FileGlobV2 => "file_glob",
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
        api::ToolType::RunShellCommand => ToolSchema {
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
        api::ToolType::ReadFiles => ToolSchema {
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
        api::ToolType::ApplyFileDiffs => ToolSchema {
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
        api::ToolType::Grep => ToolSchema {
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
        api::ToolType::FileGlobV2 => ToolSchema {
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
        _ => return None,
    };
    Some(schema)
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
        _ => return None,
    };

    Some(api::message::ToolCall {
        tool_call_id: tool_call_id.to_string(),
        tool: Some(tool),
    })
}

fn string_field(value: &Value, key: &str) -> Option<String> {
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
