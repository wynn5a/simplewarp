//! Conversions from MAA API types to application types.
use std::time::Duration;

use ai::agent::UnknownCitationTypeError;
use ai::agent::convert::ToolToAIAgentActionError;
use warp_core::channel::ChannelState;
use warp_multi_agent_api as api;

use crate::ai::agent::comment::CodeReview;
use crate::ai::agent::task::TaskId;
use crate::ai::agent::todos::AIAgentTodoList;
use crate::ai::agent::util::parse_markdown_into_text_and_code_sections;
use crate::ai::agent::{
    AIAgentAction, AIAgentActionType, AIAgentAttachment, AIAgentCitation, AIAgentOutputMessage,
    AIAgentText, AIAgentTodo, ArtifactCreatedData, MessageId, SubagentCall, SubagentType,
    SuggestedAgentModeWorkflow, SuggestedRule, Suggestions, SummarizationType, TodoOperation,
    UserQueryMode,
};
use crate::ai::artifact_download::sanitized_basename;

impl TryFrom<api::Attachment> for AIAgentAttachment {
    type Error = anyhow::Error;

    fn try_from(attachment: api::Attachment) -> Result<Self, Self::Error> {
        match attachment.value {
            Some(api::attachment::Value::FilePathReference(fpr)) => {
                Ok(AIAgentAttachment::FilePathReference {
                    file_id: String::new(),
                    file_name: fpr
                        .file_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&fpr.file_path)
                        .to_string(),
                    file_path: fpr.file_path,
                })
            }
            _ => anyhow::bail!("Unsupported attachment type for conversion"),
        }
    }
}

/// Converts proto UserQueryMode to the internal UserQueryMode type
pub(crate) fn convert_user_query_mode(mode: Option<&api::UserQueryMode>) -> UserQueryMode {
    let Some(mode) = mode else {
        return UserQueryMode::default();
    };

    match &mode.r#type {
        Some(api::user_query_mode::Type::Plan(_)) => UserQueryMode::Plan,
        Some(api::user_query_mode::Type::Orchestrate(_)) => UserQueryMode::Orchestrate,
        None => UserQueryMode::Normal,
    }
}

/// Unexpected errors when trying to convert an [`api::Message`] to an [`AIAgentOutputMessage`].
#[derive(Debug, thiserror::Error)]
pub enum MessageToAIAgentOutputMessageError {
    #[error("Missing expected message")]
    MissingMessage,
    #[error("Error converting tool to action: {0:?}")]
    ToolError(#[from] ToolToAIAgentActionError),
    #[error("Error converting citation: {0:?}")]
    CitationError(#[from] UnknownCitationTypeError),
}

/// Successful result when trying to convert an [`api::message::ToolCall`] to an [`AIAgentAction`].
#[allow(clippy::large_enum_variant)]
pub enum MaybeAIAgentOutputMessage {
    /// There is a mapping to a client output message.
    Message(AIAgentOutputMessage),
    /// We tried to parse a message that we don't care about.
    NoClientRepresentation,
}

/// Successful result when trying to convert an [`api::message::ToolCall`] to an [`AIAgentAction`].
#[allow(clippy::large_enum_variant)]
enum MaybeAIAgentAction {
    /// There is a mapping to a client action.
    Action(AIAgentAction),
    Subagent(SubagentCall),
    /// We tried to parse a tool call that we don't care about.
    NoClientRepresentation,
}

pub struct ConversionParams<'a> {
    pub task_id: &'a TaskId,
    pub current_todo_list: Option<&'a AIAgentTodoList>,
    pub active_code_review: Option<&'a CodeReview>,
}

/// Trait for converting an [`api::Message`] to an [`AIAgentOutputMessage`].
pub trait ConvertAPIMessageToClientOutputMessage {
    fn to_client_output_message(
        self,
        params: ConversionParams,
    ) -> Result<MaybeAIAgentOutputMessage, MessageToAIAgentOutputMessageError>;
}

impl ConvertAPIMessageToClientOutputMessage for api::Message {
    fn to_client_output_message(
        self,
        params: ConversionParams,
    ) -> Result<MaybeAIAgentOutputMessage, MessageToAIAgentOutputMessageError> {
        let Some(message) = self.message else {
            // In shared-session streams we can receive skeleton placeholder task messages without payloads.
            // Treat them as having no client representation rather than erroring and aborting ingestion entirely.
            return Ok(MaybeAIAgentOutputMessage::NoClientRepresentation);
        };

        let citations = self
            .citations
            .iter()
            .filter_map(|citation| AIAgentCitation::try_from((*citation).clone()).ok())
            .collect::<Vec<AIAgentCitation>>();

        match message {
            api::message::Message::AgentOutput(output) => Ok(MaybeAIAgentOutputMessage::Message(
                AIAgentOutputMessage::text(MessageId::new(self.id), output.into())
                    .with_citations(citations),
            )),
            api::message::Message::AgentReasoning(reasoning) => {
                let duration = reasoning
                    .finished_duration
                    .map(|d| Duration::from_secs(d.seconds as u64));
                Ok(MaybeAIAgentOutputMessage::Message(
                    AIAgentOutputMessage::reasoning(
                        MessageId::new(self.id),
                        reasoning.into(),
                        duration,
                    ),
                ))
            }
            api::message::Message::ToolCall(tool_call) => match tool_call.to_action(params)? {
                MaybeAIAgentAction::Action(action) => Ok(MaybeAIAgentOutputMessage::Message(
                    AIAgentOutputMessage::action(MessageId::new(self.id), action)
                        .with_citations(citations),
                )),
                MaybeAIAgentAction::Subagent(subagent) => Ok(MaybeAIAgentOutputMessage::Message(
                    AIAgentOutputMessage::subagent(MessageId::new(self.id), subagent)
                        .with_citations(citations),
                )),
                MaybeAIAgentAction::NoClientRepresentation => {
                    Ok(MaybeAIAgentOutputMessage::NoClientRepresentation)
                }
            },
            // Web search and fetch ran on the server; there is nothing to draw for saved ones.
            api::message::Message::WebSearch(_) | api::message::Message::WebFetch(_) => {
                Ok(MaybeAIAgentOutputMessage::NoClientRepresentation)
            }
            api::message::Message::ModelUsed(_) => {
                Ok(MaybeAIAgentOutputMessage::NoClientRepresentation)
            }
            api::message::Message::UpdateTodos(update_todos) => {
                if let Some(operation) = update_todos.operation {
                    match operation {
                        api::message::update_todos::Operation::CreateTodoList(create_todo_list) => {
                            Ok(MaybeAIAgentOutputMessage::Message(
                                AIAgentOutputMessage::todo_operation(
                                    MessageId::new(self.id),
                                    TodoOperation::UpdateTodos {
                                        todos: create_todo_list
                                            .initial_todos
                                            .into_iter()
                                            .map(Into::into)
                                            .collect(),
                                    },
                                )
                                .with_citations(citations),
                            ))
                        }
                        api::message::update_todos::Operation::UpdatePendingTodos(
                            update_pending_todos,
                        ) => Ok(MaybeAIAgentOutputMessage::Message(
                            AIAgentOutputMessage::todo_operation(
                                MessageId::new(self.id),
                                TodoOperation::UpdateTodos {
                                    todos: params
                                        .current_todo_list
                                        .iter()
                                        .flat_map(|list| list.completed_items().iter().cloned())
                                        .chain(
                                            update_pending_todos
                                                .updated_pending_todos
                                                .into_iter()
                                                .map(Into::into),
                                        )
                                        .collect(),
                                },
                            )
                            .with_citations(citations),
                        )),
                        api::message::update_todos::Operation::MarkTodosCompleted(
                            mark_todos_completed,
                        ) => {
                            if mark_todos_completed.todo_ids.is_empty() {
                                Ok(MaybeAIAgentOutputMessage::NoClientRepresentation)
                            } else {
                                // This is a mark as completed operation
                                Ok(MaybeAIAgentOutputMessage::Message(
                                    AIAgentOutputMessage::todo_operation(
                                        MessageId::new(self.id),
                                        TodoOperation::MarkAsCompleted {
                                            completed_todos: mark_todos_completed
                                                .todo_ids
                                                .into_iter()
                                                .filter_map(|todo_id| {
                                                    params.current_todo_list.and_then(|todo_list| {
                                                        todo_list
                                                            .completed_items()
                                                            .iter()
                                                            .find(|item| {
                                                                item.id.as_ref() == todo_id.as_str()
                                                            })
                                                            .cloned()
                                                    })
                                                })
                                                .collect(),
                                        },
                                    )
                                    .with_citations(citations),
                                ))
                            }
                        }
                    }
                } else {
                    Ok(MaybeAIAgentOutputMessage::NoClientRepresentation)
                }
            }
            api::message::Message::Summarization(summarization) => {
                let duration = summarization
                    .finished_duration
                    .map(|d| Duration::from_secs(d.seconds as u64));
                let (text, summarization_type, token_count) = match summarization.summary_type {
                    Some(api::message::summarization::SummaryType::ConversationSummary(
                        conv_summary,
                    )) => {
                        let token_count = if conv_summary.token_count > 0 {
                            Some(conv_summary.token_count as u32)
                        } else {
                            None
                        };
                        let text = if !conv_summary.summary.is_empty() {
                            AIAgentText {
                                sections: parse_markdown_into_text_and_code_sections(
                                    &conv_summary.summary,
                                ),
                            }
                        } else {
                            AIAgentText { sections: vec![] }
                        };
                        (text, SummarizationType::ConversationSummary, token_count)
                    }
                    Some(api::message::summarization::SummaryType::ToolCallResultSummary(_)) => (
                        AIAgentText { sections: vec![] },
                        SummarizationType::ToolCallResultSummary,
                        None,
                    ),
                    None => {
                        // Default to ConversationSummary if not specified
                        (
                            AIAgentText { sections: vec![] },
                            SummarizationType::ConversationSummary,
                            None,
                        )
                    }
                };
                Ok(MaybeAIAgentOutputMessage::Message(
                    AIAgentOutputMessage::summarization(
                        MessageId::new(self.id),
                        text,
                        duration,
                        summarization_type,
                        token_count,
                    ),
                ))
            }
            api::message::Message::UpdateReviewComments(update_comments) => {
                if let Some(operation) = update_comments.operation {
                    match operation {
                        api::message::update_review_comments::Operation::AddressReviewComments(
                            address_comments,
                        ) => {
                            if let Some(current_comments) = params.active_code_review {
                                let addressed_comments = current_comments
                                    .addressed_comments
                                    .iter()
                                    .filter(|comment| {
                                        address_comments
                                            .comment_ids
                                            .iter()
                                            .any(|id| id == &comment.id.to_string())
                                    })
                                    .cloned()
                                    .collect();
                                Ok(MaybeAIAgentOutputMessage::Message(
                                    AIAgentOutputMessage::comments_addressed(
                                        MessageId::new(self.id),
                                        addressed_comments,
                                    )
                                    .with_citations(citations),
                                ))
                            } else {
                                Ok(MaybeAIAgentOutputMessage::NoClientRepresentation)
                            }
                        }
                    }
                } else {
                    Ok(MaybeAIAgentOutputMessage::NoClientRepresentation)
                }
            }
            api::message::Message::DebugOutput(debug_output) => {
                if ChannelState::enable_debug_features() {
                    Ok(MaybeAIAgentOutputMessage::Message(
                        AIAgentOutputMessage::debug_output(
                            MessageId::new(self.id),
                            debug_output.text,
                        ),
                    ))
                } else {
                    Ok(MaybeAIAgentOutputMessage::NoClientRepresentation)
                }
            }
            api::message::Message::ArtifactEvent(artifact_event) => match artifact_event.event {
                Some(api::message::artifact_event::Event::Created(artifact_created)) => {
                    match artifact_created.artifact {
                        Some(
                            api::message::artifact_event::artifact_created::Artifact::PullRequest(
                                pr,
                            ),
                        ) => Ok(MaybeAIAgentOutputMessage::Message(
                            AIAgentOutputMessage::artifact_created(
                                MessageId::new(self.id),
                                ArtifactCreatedData::PullRequest {
                                    url: pr.url,
                                    branch: pr.branch,
                                },
                            )
                            .with_citations(citations),
                        )),
                        Some(
                            api::message::artifact_event::artifact_created::Artifact::Screenshot(
                                screenshot,
                            ),
                        ) => Ok(MaybeAIAgentOutputMessage::Message(
                            AIAgentOutputMessage::artifact_created(
                                MessageId::new(self.id),
                                ArtifactCreatedData::Screenshot {
                                    artifact_uid: screenshot.artifact_uid,
                                    mime_type: screenshot.mime_type,
                                    description: if screenshot.description.is_empty() {
                                        None
                                    } else {
                                        Some(screenshot.description)
                                    },
                                },
                            )
                            .with_citations(citations),
                        )),
                        Some(api::message::artifact_event::artifact_created::Artifact::File(
                            file,
                        )) => Ok(MaybeAIAgentOutputMessage::Message(
                            AIAgentOutputMessage::artifact_created(
                                MessageId::new(self.id),
                                ArtifactCreatedData::File {
                                    artifact_uid: file.artifact_uid,
                                    filename: sanitized_basename(&file.filepath)
                                        .unwrap_or_else(|| file.filepath.clone()),
                                    filepath: file.filepath,
                                    mime_type: file.mime_type,
                                    description: if file.description.is_empty() {
                                        None
                                    } else {
                                        Some(file.description)
                                    },
                                    size_bytes: file.size_bytes,
                                },
                            )
                            .with_citations(citations),
                        )),
                        None => Ok(MaybeAIAgentOutputMessage::NoClientRepresentation),
                    }
                }
                Some(api::message::artifact_event::Event::ForkArtifacts(_)) | None => {
                    Ok(MaybeAIAgentOutputMessage::NoClientRepresentation)
                }
            },
            // These messages don't indicate an error but they don't translate to a client-side output message.
            api::message::Message::UserQuery(_)
            | api::message::Message::SystemQuery(_)
            | api::message::Message::ToolCallResult(_)
            | api::message::Message::CodeReview(_)
            | api::message::Message::ServerEvent(_)
            | api::message::Message::InvokeSkill(_)
            | api::message::Message::PassiveSuggestionResult(_)
            // Orchestration messages: the local agent has no sub-agents, so saved ones
            // are not shown.
            | api::message::Message::MessagesReceivedFromAgents(_)
            | api::message::Message::EventsFromAgents(_)
            // Stage 2 plan-card config snapshot: hydrated separately by the
            // plan card's `AIDocumentModel` subscription, not via the
            // exchange/output stream. No client output message representation.
            | api::message::Message::OrchestrationConfigSnapshot(_) => {
                Ok(MaybeAIAgentOutputMessage::NoClientRepresentation)
            }
        }
    }
}

impl From<api::message::AgentOutput> for AIAgentText {
    fn from(value: api::message::AgentOutput) -> Self {
        AIAgentText {
            sections: parse_markdown_into_text_and_code_sections(value.text.as_str()),
        }
    }
}

impl From<api::message::AgentReasoning> for AIAgentText {
    fn from(value: api::message::AgentReasoning) -> Self {
        AIAgentText {
            sections: parse_markdown_into_text_and_code_sections(value.reasoning.as_str()),
        }
    }
}

/// Trait for converting an [`api::Message`] to an [`AIAgentOutputMessage`].
trait ConvertAPIToolCallToAIAgentAction {
    fn to_action(
        self,
        params: ConversionParams,
    ) -> Result<MaybeAIAgentAction, ToolToAIAgentActionError>;
}

/// Tries to convert an [`api::message::ToolCall`] to an [`AIAgentAction`].
///
/// A [`Result::Error`] indicates an unexpected problem, while [`Ok(None)`]
/// indicates a tool call that we aren't expected to parse.
impl ConvertAPIToolCallToAIAgentAction for api::message::ToolCall {
    fn to_action(
        self,
        params: ConversionParams,
    ) -> Result<MaybeAIAgentAction, ToolToAIAgentActionError> {
        let Some(tool) = self.tool else {
            return Err(ToolToAIAgentActionError::MissingTool);
        };

        let create_standard_action = |action: AIAgentActionType| {
            Ok(MaybeAIAgentAction::Action(AIAgentAction {
                id: self.tool_call_id.clone().into(),
                task_id: params.task_id.clone(),
                action,
                requires_result: true,
            }))
        };

        match tool {
            api::message::tool_call::Tool::RunShellCommand(run_shell_command) => {
                create_standard_action(run_shell_command.into())
            }
            api::message::tool_call::Tool::WriteToLongRunningShellCommand(
                write_to_long_running_shell_command,
            ) => create_standard_action(write_to_long_running_shell_command.into()),
            api::message::tool_call::Tool::ReadFiles(read_files) => {
                create_standard_action(read_files.into())
            }
            // Server-offered upload to server artifact storage; persisted calls are not shown.
            api::message::tool_call::Tool::UploadFileArtifact(_) => {
                Ok(MaybeAIAgentAction::NoClientRepresentation)
            }
            api::message::tool_call::Tool::Grep(grep) => create_standard_action(grep.into()),
            #[allow(deprecated)]
            api::message::tool_call::Tool::FileGlob(glob) => create_standard_action(glob.into()),
            api::message::tool_call::Tool::FileGlobV2(glob) => create_standard_action(glob.into()),
            api::message::tool_call::Tool::ApplyFileDiffs(apply_file_diffs) => {
                create_standard_action(apply_file_diffs.into())
            }
            api::message::tool_call::Tool::ReadMcpResource(read_mcp_resource) => {
                create_standard_action(read_mcp_resource.into())
            }
            api::message::tool_call::Tool::CallMcpTool(call_mcp_tool) => {
                match call_mcp_tool.try_into() {
                    Ok(call_mcp_tool_action) => create_standard_action(call_mcp_tool_action),
                    Err(error) => Err(error),
                }
            }
            api::message::tool_call::Tool::SuggestNewConversation(suggest_new_conversation) => {
                create_standard_action(suggest_new_conversation.into())
            }
            api::message::tool_call::Tool::SuggestPrompt(suggest_prompt) => {
                match suggest_prompt.try_into() {
                    Ok(suggest_prompt_action) => create_standard_action(suggest_prompt_action),
                    Err(_) => Ok(MaybeAIAgentAction::NoClientRepresentation),
                }
            }
            api::message::tool_call::Tool::OpenCodeReview(_) => {
                create_standard_action(AIAgentActionType::OpenCodeReview)
            }
            api::message::tool_call::Tool::InitProject(_) => {
                create_standard_action(AIAgentActionType::InitProject)
            }
            api::message::tool_call::Tool::ReadShellCommandOutput(read_shell_command_output) => {
                create_standard_action(read_shell_command_output.into())
            }
            api::message::tool_call::Tool::TransferShellCommandControlToUser(
                transfer_shell_command_control_to_user,
            ) => create_standard_action(transfer_shell_command_control_to_user.into()),
            // These tools are gone; saved calls have nothing to draw.
            api::message::tool_call::Tool::ReadDocuments(_)
            | api::message::tool_call::Tool::EditDocuments(_)
            | api::message::tool_call::Tool::CreateDocuments(_)
            | api::message::tool_call::Tool::SearchCodebase(_)
            | api::message::tool_call::Tool::ReadSkill(_)
            | api::message::tool_call::Tool::AskUserQuestion(_)
            | api::message::tool_call::Tool::UseComputer(_)
            | api::message::tool_call::Tool::RequestComputerUse(_)
            | api::message::tool_call::Tool::StartRecording(_)
            | api::message::tool_call::Tool::StopRecording(_)
            | api::message::tool_call::Tool::RunAgents(_)
            | api::message::tool_call::Tool::SendMessageToAgent(_)
            | api::message::tool_call::Tool::FetchConversation(_)
            | api::message::tool_call::Tool::WaitForEvents(_) => {
                Ok(MaybeAIAgentAction::NoClientRepresentation)
            }
            api::message::tool_call::Tool::Subagent(subagent) => {
                use api::message::tool_call::subagent::Metadata;
                use api::message::tool_call::subagent::conversation_search_metadata::Target;
                let subagent_type = match subagent.metadata {
                    Some(Metadata::Cli(_)) => SubagentType::Cli,
                    Some(Metadata::Research(_)) => SubagentType::Research,
                    Some(Metadata::Advice(_)) => SubagentType::Advice,
                    Some(Metadata::ComputerUse(_)) => SubagentType::Unknown,
                    Some(Metadata::Summarization(_)) => SubagentType::Summarization,
                    Some(Metadata::ConversationSearch(cs_meta)) => {
                        let query = if cs_meta.query.is_empty() {
                            None
                        } else {
                            Some(cs_meta.query)
                        };
                        let (conversation_id, agent_run_id) = match cs_meta.target {
                            Some(Target::ConversationId(conversation_id))
                                if !conversation_id.is_empty() =>
                            {
                                (Some(conversation_id), None)
                            }
                            Some(Target::AgentRunId(agent_run_id)) if !agent_run_id.is_empty() => {
                                (None, Some(agent_run_id))
                            }
                            Some(Target::ConversationId(_))
                            | Some(Target::AgentRunId(_))
                            | None => (None, None),
                        };
                        SubagentType::ConversationSearch {
                            query,
                            conversation_id,
                            agent_run_id,
                        }
                    }
                    Some(Metadata::WarpDocumentationSearch(_)) => {
                        SubagentType::WarpDocumentationSearch
                    }
                    None => SubagentType::Unknown,
                };
                Ok(MaybeAIAgentAction::Subagent(SubagentCall {
                    task_id: subagent.task_id,
                    subagent_type,
                }))
            }
            api::message::tool_call::Tool::InsertReviewComments(insert_review_comments) => {
                create_standard_action(insert_review_comments.into())
            }
            // Clients do not need to know how to parse server tool-calls but receiving
            // them is not an error.
            api::message::tool_call::Tool::Server(_) => {
                Ok(MaybeAIAgentAction::NoClientRepresentation)
            }
            _ => Err(ToolToAIAgentActionError::UnexpectedTool),
        }
    }
}

impl From<api::Suggestions> for Suggestions {
    fn from(api_suggestions: api::Suggestions) -> Self {
        Self {
            rules: api_suggestions
                .rules
                .into_iter()
                .map(|rule| SuggestedRule {
                    name: rule.name,
                    content: rule.content,
                    logging_id: rule.logging_id.into(),
                })
                .collect(),
            agent_mode_workflows: api_suggestions
                .workflows
                .into_iter()
                .map(|workflow| SuggestedAgentModeWorkflow {
                    name: workflow.name,
                    prompt: workflow.prompt,
                    logging_id: workflow.logging_id.into(),
                })
                .collect(),
        }
    }
}

impl From<api::TodoItem> for AIAgentTodo {
    fn from(value: api::TodoItem) -> Self {
        AIAgentTodo {
            id: value.id.into(),
            title: value.title,
            description: value.description,
        }
    }
}

#[cfg(test)]
#[path = "convert_from_tests.rs"]
mod tests;
