mod convert;
mod review_comments;

use std::fmt::Display;
use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

use itertools::Itertools as _;
pub use review_comments::{
    ReviewCommentThread, ReviewCommentThreadItem, format_review_comment_thread,
    group_review_comment_threads,
};
use strum_macros::EnumDiscriminants;
use uuid::Uuid;
pub use warp_multi_agent_api::LifecycleEventType;
use warp_terminal::model::BlockId;

use crate::agent::action_result::{
    AIAgentActionResultType, CallMCPToolResult, FetchConversationResult, FileGlobResult,
    FileGlobV2Result, GrepResult, InsertReviewCommentsResult, ReadFilesResult,
    ReadMCPResourceResult, ReadShellCommandOutputResult, RequestCommandOutputResult,
    RequestFileEditsResult, RunAgentsResult, SendMessageToAgentResult,
    SuggestNewConversationResult, SuggestPromptResult, TransferShellCommandControlToUserResult,
    WaitForEventsResult, WriteToLongRunningShellCommandResult,
};
use crate::agent::{AIAgentCitation, FileLocations};
use crate::diff_validation::ParsedDiff;
use crate::skills::SkillReference;

#[derive(Debug, Clone, Eq, PartialEq, EnumDiscriminants)]
pub enum AIAgentActionType {
    /// The AI requested the output for a given command to be retrieved as context in responding to
    /// a user's query.
    RequestCommandOutput {
        command: String,

        /// [`Some(true)`] iff the LLM thinks that the `command` is readonly and doesn't produce side-effects.
        is_read_only: Option<bool>,

        /// [`Some(true)`] iff the LLM thinks that the `command` is risky and should require user confirmation.
        is_risky: Option<bool>,

        /// `true` if the client should wait until the command is completed and report the finish output as the result.
        ///
        /// If `false` _and_ the command is long-running, a snapshot of the command output is taken and reported as the
        /// result instead.
        wait_until_completion: bool,

        /// [`Some(true)`] iff the LLM thinks that the `command` might invoke a pager.
        uses_pager: Option<bool>,

        /// The AI's rationale for requesting a command.
        rationale: Option<String>,

        /// The citations for the command.
        citations: Vec<AIAgentCitation>,
    },

    WriteToLongRunningShellCommand {
        block_id: BlockId,
        input: bytes::Bytes,
        mode: AIAgentPtyWriteMode,
    },

    /// AI requested getting the content of some files.
    ReadFiles(ReadFilesRequest),

    /// AI requested a vector of edits. Each edit holds a list of diffs on a single code file.
    RequestFileEdits {
        file_edits: Vec<FileEdit>,
        title: Option<String>,
    },

    Grep {
        queries: Vec<String>,
        path: String,
    },

    FileGlob {
        patterns: Vec<String>,
        path: Option<String>,
    },

    FileGlobV2 {
        patterns: Vec<String>,
        search_dir: Option<String>,
        // TODO(matthew): Maybe implement client side depth and result limits.
    },

    ReadMCPResource {
        server_id: Option<Uuid>,
        name: String,
        /// The unique URI for the resource. Prefer using this to identify
        /// a resource over [`ReadMCPResource::name`], when available.
        ///
        /// We should phase out `name` eventually and make this non-optional.
        uri: Option<String>,
    },

    CallMCPTool {
        server_id: Option<Uuid>,
        name: String,
        input: serde_json::Value,
    },

    SuggestNewConversation {
        message_id: String,
    },

    SuggestPrompt(SuggestPromptRequest),

    InitProject,
    OpenCodeReview,

    ReadShellCommandOutput {
        block_id: BlockId,
        delay: Option<ShellCommandDelay>,
    },

    InsertCodeReviewComments {
        repo_path: PathBuf,
        comments: Vec<InsertReviewComment>,
        base_branch: Option<String>,
    },

    FetchConversation {
        conversation_id: String,
    },

    SendMessageToAgent {
        addresses: Vec<String>,
        subject: String,
        message: String,
    },
    /// Transfer control of a running shell command to the user.
    TransferShellCommandControlToUser {
        /// The reason provided by the agent for transferring control.
        reason: String,
    },

    /// AI requested batched orchestration of one-or-more child agents that
    /// share run-wide configuration (model, harness, execution mode).
    /// The full per-child prompt is computed at dispatch time as
    /// `base_prompt + "\n\n" + agent_run_configs[i].prompt` (or just
    /// `base_prompt` when the per-agent `prompt` is empty).
    RunAgents(RunAgentsRequest),

    /// Synthesized from a server-emitted Message::ToolCall::WaitForEvents;
    /// dispatched by WaitForEventsExecutor.
    WaitForEvents {
        /// tool_call_id of the unresolved WaitForEvents call; used to
        /// match inbound resume signals.
        tool_call_id: String,
        /// 0 means "unset" (prost flat-scalar convention); the executor
        /// falls back to a default.
        idle_timeout_seconds: i32,
    },
}

/// Run-wide + per-agent configuration for a `RunAgents` tool call.
///
/// Mirrors the proto `RunAgents` message. Server-resolved fields
/// (`model_id`, `harness_type`, `execution_mode`) are
/// folded in by the server's final tool-call re-emission once the
/// payload is complete; the client renders the full layout from a
/// fully-resolved instance only.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RunAgentsRequest {
    pub summary: String,
    pub base_prompt: String,
    pub skills: Vec<SkillReference>,
    pub model_id: String,
    pub harness_type: String,
    pub execution_mode: RunAgentsExecutionMode,
    pub agent_run_configs: Vec<RunAgentsAgentRunConfig>,
    pub plan_id: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RunAgentsExecutionMode {
    Local,
    /// A server-resolved remote run from persisted conversation data. This build has no
    /// remote workers, so dispatching one fails; an edited request is always `Local`.
    Remote,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RunAgentsAgentRunConfig {
    pub name: String,
    pub prompt: String,
    pub title: String,
    /// Optional model override for this specific child agent. When non-empty,
    /// overrides the batch-level `model_id` for this child only. When empty,
    /// the child inherits the batch-level model.
    pub model_id: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum StartAgentExecutionMode {
    Local {
        /// `None` selects the legacy embedded local child-agent flow.
        /// `Some(...)` selects a third-party CLI harness to launch locally.
        harness_type: Option<String>,
        /// `None` inherits the parent agent's preferred LLM (legacy behavior).
        /// `Some(_)` overrides the child's preferred LLM with the supplied
        /// model id (used by the orchestrate confirmation card so the user's
        /// model selection is honored on local launches).
        model_id: Option<String>,
    },
}

impl AIAgentActionType {
    pub fn is_request_command_output(&self) -> bool {
        matches!(self, Self::RequestCommandOutput { .. })
    }

    pub fn is_read_files(&self) -> bool {
        matches!(self, Self::ReadFiles(..))
    }

    pub fn is_grep(&self) -> bool {
        matches!(self, Self::Grep { .. })
    }

    pub fn is_file_glob(&self) -> bool {
        matches!(self, Self::FileGlob { .. } | Self::FileGlobV2 { .. })
    }

    pub fn is_write_to_shell_command(&self) -> bool {
        matches!(self, Self::WriteToLongRunningShellCommand { .. })
    }

    pub fn cancelled_result(&self) -> AIAgentActionResultType {
        match self {
            Self::RequestCommandOutput { .. } => AIAgentActionResultType::RequestCommandOutput(
                RequestCommandOutputResult::CancelledBeforeExecution,
            ),
            Self::RequestFileEdits { .. } => {
                AIAgentActionResultType::RequestFileEdits(RequestFileEditsResult::Cancelled)
            }
            Self::ReadFiles(..) => AIAgentActionResultType::ReadFiles(ReadFilesResult::Cancelled),
            Self::Grep { .. } => AIAgentActionResultType::Grep(GrepResult::Cancelled),
            Self::FileGlob { .. } => AIAgentActionResultType::FileGlob(FileGlobResult::Cancelled),
            Self::FileGlobV2 { .. } => {
                AIAgentActionResultType::FileGlobV2(FileGlobV2Result::Cancelled)
            }
            Self::WriteToLongRunningShellCommand { .. } => {
                AIAgentActionResultType::WriteToLongRunningShellCommand(
                    WriteToLongRunningShellCommandResult::Cancelled,
                )
            }
            Self::CallMCPTool { .. } => {
                AIAgentActionResultType::CallMCPTool(CallMCPToolResult::Cancelled)
            }
            Self::ReadMCPResource { .. } => {
                AIAgentActionResultType::ReadMCPResource(ReadMCPResourceResult::Cancelled)
            }
            Self::SuggestNewConversation { .. } => AIAgentActionResultType::SuggestNewConversation(
                SuggestNewConversationResult::Cancelled,
            ),
            Self::SuggestPrompt { .. } => {
                AIAgentActionResultType::SuggestPrompt(SuggestPromptResult::Cancelled)
            }
            Self::OpenCodeReview => AIAgentActionResultType::OpenCodeReview,
            Self::InitProject => AIAgentActionResultType::InitProject,
            Self::ReadShellCommandOutput { .. } => AIAgentActionResultType::ReadShellCommandOutput(
                ReadShellCommandOutputResult::Cancelled,
            ),
            Self::InsertCodeReviewComments { .. } => {
                AIAgentActionResultType::InsertReviewComments(InsertReviewCommentsResult::Cancelled)
            }
            Self::FetchConversation { .. } => {
                AIAgentActionResultType::FetchConversation(FetchConversationResult::Cancelled)
            }
            Self::SendMessageToAgent { .. } => {
                AIAgentActionResultType::SendMessageToAgent(SendMessageToAgentResult::Cancelled)
            }
            Self::TransferShellCommandControlToUser { .. } => {
                AIAgentActionResultType::TransferShellCommandControlToUser(
                    TransferShellCommandControlToUserResult::Cancelled,
                )
            }
            Self::RunAgents(_) => AIAgentActionResultType::RunAgents(RunAgentsResult::Cancelled),
            Self::WaitForEvents { .. } => {
                AIAgentActionResultType::WaitForEvents(WaitForEventsResult::Cancelled)
            }
        }
    }

    pub fn user_friendly_name(&self) -> String {
        match self {
            Self::RequestCommandOutput { command, .. } => {
                format!("Run command: {command}")
            }
            Self::WriteToLongRunningShellCommand { .. } => {
                "Write to long running shell command".to_string()
            }
            Self::ReadFiles(_) => "Read files".to_string(),
            Self::RequestFileEdits { file_edits, .. } => {
                let file_names = file_edits.iter().filter_map(|edit| edit.file()).join(", ");
                format!("Edit {file_names}")
            }
            Self::Grep { .. } => "Grep".to_string(),
            Self::FileGlob { .. } | Self::FileGlobV2 { .. } => "File glob".to_string(),
            Self::ReadMCPResource { .. } => "Read mcp resource".to_string(),
            Self::CallMCPTool { .. } => "Call mcp tool".to_string(),
            Self::SuggestNewConversation { .. } => "Suggest new conversation".to_string(),
            Self::SuggestPrompt { .. } => "Suggest prompt".to_string(),
            Self::InitProject => "Init project".to_string(),
            Self::OpenCodeReview => "Open code review".to_string(),
            Self::ReadShellCommandOutput { .. } => "Read shell command output".to_string(),
            Self::InsertCodeReviewComments { comments, .. } => {
                format!("Insert {} code review comments", comments.len())
            }
            Self::FetchConversation { .. } => "Fetch conversation".to_string(),
            Self::SendMessageToAgent { subject, .. } => format!("Send message: {subject}"),
            Self::TransferShellCommandControlToUser { .. } => {
                "Transfer shell command control to user".to_string()
            }
            Self::RunAgents(req) => {
                format!("Orchestrate {} agent(s)", req.agent_run_configs.len())
            }
            Self::WaitForEvents { .. } => "Wait for events".to_string(),
        }
    }
}

impl Display for AIAgentActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AIAgentActionType::RequestCommandOutput {
                command,
                is_read_only,
                uses_pager,
                ..
            } => {
                write!(
                    f,
                    "RequestCommandOutput: {command} (read_only: {is_read_only:?}, pager: {uses_pager:?})"
                )
            }
            AIAgentActionType::WriteToLongRunningShellCommand {
                block_id,
                input,
                mode,
            } => {
                write!(
                    f,
                    "WriteToLongRunningShellCommand (block id: {block_id}): {input:?}, {mode:?}",
                )
            }
            AIAgentActionType::ReadFiles(request) => {
                write!(f, "{request}")
            }
            AIAgentActionType::RequestFileEdits { file_edits, title } => {
                let file_names = file_edits
                    .iter()
                    .filter_map(|edit| edit.file())
                    .collect::<Vec<_>>()
                    .join(", ");
                if let Some(title) = title {
                    write!(f, "RequestFileEdits '{title}': [{file_names}]")
                } else {
                    write!(f, "RequestFileEdits: [{file_names}]")
                }
            }
            AIAgentActionType::Grep { queries, path } => {
                write!(f, "Grep: [{}] in {}", queries.join(", "), path)
            }
            AIAgentActionType::FileGlob { patterns, path } => {
                let path_str = path.as_deref().unwrap_or(".");
                write!(f, "FileGlob: [{}] in {}", patterns.join(", "), path_str)
            }
            AIAgentActionType::FileGlobV2 {
                patterns,
                search_dir,
            } => {
                let path_str = search_dir.as_deref().unwrap_or(".");
                write!(f, "FileGlobV2: [{}] in {}", patterns.join(", "), path_str)
            }
            AIAgentActionType::ReadMCPResource {
                server_id: _,
                name,
                uri,
            } => {
                if let Some(uri) = uri {
                    write!(f, "ReadMCPResource: {name} ({uri})")
                } else {
                    write!(f, "ReadMCPResource: {name}")
                }
            }
            AIAgentActionType::CallMCPTool {
                server_id: _,
                name,
                input,
            } => {
                write!(f, "CallMCPTool: {name} with input {input:?}")
            }
            AIAgentActionType::SuggestNewConversation { message_id } => {
                write!(f, "SuggestNewConversation: {message_id}")
            }
            AIAgentActionType::SuggestPrompt(request) => {
                write!(f, "SuggestPrompt: {request:?}")
            }
            AIAgentActionType::InitProject => {
                write!(f, "InitProject")
            }
            AIAgentActionType::OpenCodeReview => {
                write!(f, "OpenCodeReview")
            }
            AIAgentActionType::ReadShellCommandOutput { delay, block_id } => {
                let delay = match delay {
                    Some(ShellCommandDelay::Duration(duration)) => {
                        format!("{} seconds", duration.as_secs())
                    }
                    Some(ShellCommandDelay::OnCompletion) => "on completion".to_string(),
                    None => "no".to_string(),
                };
                write!(
                    f,
                    "ReadShellCommandOutput (block id: {block_id}): with {delay} delay"
                )
            }
            AIAgentActionType::InsertCodeReviewComments { comments, .. } => {
                let file_paths = comments
                    .iter()
                    .filter_map(|c| {
                        c.comment_location
                            .as_ref()
                            .map(|loc| loc.relative_file_path.as_str())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "InsertCodeReviewComments: {} comments on [{}]",
                    comments.len(),
                    file_paths
                )
            }
            AIAgentActionType::FetchConversation { conversation_id } => {
                write!(f, "FetchConversation: {conversation_id}")
            }
            AIAgentActionType::SendMessageToAgent {
                addresses, subject, ..
            } => {
                write!(
                    f,
                    "SendMessageToAgent: to=[{}] subject={subject}",
                    addresses.join(", ")
                )
            }
            AIAgentActionType::TransferShellCommandControlToUser { reason } => {
                write!(f, "TransferShellCommandControlToUser: {reason}")
            }
            AIAgentActionType::RunAgents(req) => {
                let names = req
                    .agent_run_configs
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "Orchestrate: summary='{}' agents=[{names}]", req.summary,)
            }
            AIAgentActionType::WaitForEvents {
                tool_call_id,
                idle_timeout_seconds,
            } => {
                write!(
                    f,
                    "WaitForEvents: tool_call_id={tool_call_id} idle_timeout_seconds={idle_timeout_seconds}"
                )
            }
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ReadFilesRequest {
    pub locations: Vec<FileLocations>,
}

impl Display for ReadFilesRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file_names = self
            .locations
            .iter()
            .map(|loc| loc.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "ReadFiles: [{file_names}]")
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ShellCommandDelay {
    Duration(Duration),
    OnCompletion,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, EnumDiscriminants)]
pub enum AIAgentPtyWriteMode {
    #[default]
    Raw,
    Line,
    Block,
}

impl AIAgentPtyWriteMode {
    /// Decorates input bytes according to the write mode.
    pub fn decorate_bytes(
        self,
        bytes: impl Into<Vec<u8>>,
        is_bracketed_paste_enabled: bool,
    ) -> Vec<u8> {
        use warp_terminal::model::escape_sequences;

        let bytes = bytes.into();
        match self {
            AIAgentPtyWriteMode::Raw => bytes,
            AIAgentPtyWriteMode::Line => {
                // Move to beginning of line, write input, then submit (Enter).
                let mut v = Vec::with_capacity(bytes.len() + 2);
                // ^A (SOH) is "beginning of line" for readline/prompt-toolkit style editors.
                v.push(escape_sequences::C0::SOH);
                v.extend_from_slice(&bytes);
                cfg_if::cfg_if! {
                    if #[cfg(target_os = "windows")] {
                        // Use CR to submit on Windows hosts.
                        v.push(escape_sequences::C0::CR);
                    } else {
                        // Use LF to submit on POSIX.
                        v.push(escape_sequences::C0::LF);
                    }
                }
                v
            }
            AIAgentPtyWriteMode::Block => {
                if is_bracketed_paste_enabled {
                    escape_sequences::BRACKETED_PASTE_START
                        .iter()
                        .copied()
                        .chain(bytes)
                        .chain(escape_sequences::BRACKETED_PASTE_END.iter().copied())
                        .collect()
                } else {
                    bytes
                }
            }
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InsertReviewComment {
    pub comment_id: String,
    pub author: String,
    pub last_modified_timestamp: String,
    pub comment_body: String,
    pub parent_comment_id: Option<String>,
    /// The file and line range the comment is attached to.
    /// If None, the comment applies to the whole diff set.
    pub comment_location: Option<InsertedCommentLocation>,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InsertedCommentLocation {
    /// Repo-relative path of the file the comment is attached to.
    pub relative_file_path: String,
    /// The specific line range the comment is attached to.
    /// If None, the comment applies to the whole file.
    pub line: Option<InsertedCommentLine>,
}

/// The side of a diff that a comment is attached to.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CommentSide {
    /// The right side of the diff (new file / additions).
    Right,
    /// The left side of the diff (old file / deletions).
    Left,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InsertedCommentLine {
    pub comment_line_range: Range<usize>,
    /// The diff hunk line range overlaps with the comment line range
    /// but may not match it exactly. We need this in order to be able
    /// to find the full diff hunk this comment is attached to.
    pub diff_hunk_line_range: Range<usize>,
    /// The diff hunk text is needed to find where to attach comments
    /// when line numbers on the local and remote branches have diverged.
    pub diff_hunk_text: String,
    /// The side of the diff the comment is attached to.
    pub side: Option<CommentSide>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SuggestPromptRequest {
    UnitTestsSuggestion {
        query: String,
        title: String,
        description: String,
    },
    PromptSuggestion {
        prompt: String,
        label: Option<String>,
    },
}

/// A file-editing request from the agent.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FileEdit {
    /// Edit an existing file by applying a diff.
    Edit(ParsedDiff),
    /// Create a new file.
    Create {
        file: Option<String>,
        content: Option<String>,
    },
    /// Delete an existing file.
    Delete { file: Option<String> },
}

impl FileEdit {
    /// The path to the file this edit applies to.
    pub fn file(&self) -> Option<&str> {
        match self {
            Self::Edit(diff) => diff.file().map(|s| s.as_str()),
            Self::Create { file, .. } => file.as_deref(),
            Self::Delete { file } => file.as_deref(),
        }
    }
}
