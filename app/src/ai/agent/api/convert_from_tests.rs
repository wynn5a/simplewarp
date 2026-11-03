use warp_multi_agent_api as api;

use super::{ConversionParams, ConvertAPIMessageToClientOutputMessage, MaybeAIAgentOutputMessage};
use crate::ai::agent::task::TaskId;
use crate::ai::agent::{AIAgentActionType, AIAgentOutputMessageType, MemoryScope};

fn upload_artifact_tool_call_message(path: &str, description: &str) -> api::Message {
    api::Message {
        fetched_memories: vec![],
        id: "message-id".to_string(),
        task_id: "task-id".to_string(),
        server_message_data: String::new(),
        citations: vec![],
        message: Some(api::message::Message::ToolCall(api::message::ToolCall {
            tool_call_id: "tool-call-id".to_string(),
            tool: Some(api::message::tool_call::Tool::UploadFileArtifact(
                api::UploadFileArtifact {
                    file: Some(api::FilePathReference {
                        file_path: path.to_string(),
                    }),
                    description: description.to_string(),
                },
            )),
        })),
        request_id: "request-id".to_string(),
        timestamp: None,
    }
}

fn file_artifact_created_message(filepath: &str, description: &str) -> api::Message {
    api::Message {
        fetched_memories: vec![],
        id: "message-id".to_string(),
        task_id: "task-id".to_string(),
        server_message_data: String::new(),
        citations: vec![],
        message: Some(api::message::Message::ArtifactEvent(
            api::message::ArtifactEvent {
                event: Some(api::message::artifact_event::Event::Created(
                    api::message::artifact_event::ArtifactCreated {
                        artifact: Some(
                            api::message::artifact_event::artifact_created::Artifact::File(
                                api::message::artifact_event::FileArtifact {
                                    artifact_uid: "artifact-uid".to_string(),
                                    filepath: filepath.to_string(),
                                    mime_type: "text/plain".to_string(),
                                    size_bytes: 42,
                                    description: description.to_string(),
                                },
                            ),
                        ),
                    },
                )),
            },
        )),
        request_id: "request-id".to_string(),
        timestamp: None,
    }
}

fn extract_file_artifact_created(
    output: MaybeAIAgentOutputMessage,
) -> (String, String, Option<String>, i64) {
    let MaybeAIAgentOutputMessage::Message(output_message) = output else {
        panic!("expected output message");
    };
    let AIAgentOutputMessageType::ArtifactCreated(artifact) = output_message.message else {
        panic!("expected artifact created output message");
    };
    let crate::ai::agent::ArtifactCreatedData::File {
        filepath,
        filename,
        description,
        size_bytes,
        ..
    } = artifact
    else {
        panic!("expected file artifact created output message");
    };
    (filepath, filename, description, size_bytes)
}

#[test]
fn upload_artifact_tool_call_has_no_client_representation() {
    let task_id = TaskId::new("task-id".to_string());
    let message = upload_artifact_tool_call_message(
        "/tmp/build/output.log",
        "Build output for the latest run",
    );

    let output = message
        .to_client_output_message(ConversionParams {
            task_id: &task_id,
            active_code_review: None,
        })
        .expect("conversion should succeed");

    assert!(matches!(
        output,
        MaybeAIAgentOutputMessage::NoClientRepresentation
    ));
}

#[test]
fn converts_file_artifact_created_message_with_filename() {
    let task_id = TaskId::new("task-id".to_string());
    let message =
        file_artifact_created_message("outputs/report.txt", "Build output for the latest run");

    let output = message
        .to_client_output_message(ConversionParams {
            task_id: &task_id,
            active_code_review: None,
        })
        .expect("conversion should succeed");

    let (filepath, filename, description, size_bytes) = extract_file_artifact_created(output);

    assert_eq!(filepath, "outputs/report.txt");
    assert_eq!(filename, "report.txt");
    assert_eq!(
        description.as_deref(),
        Some("Build output for the latest run")
    );
    assert_eq!(size_bytes, 42);
}

#[test]
fn transfer_control_tool_call_converts_to_action_message() {
    let task_id = TaskId::new("task".to_string());
    let reason = "Please finish the interactive flow".to_string();
    let message = api::Message {
        fetched_memories: vec![],
        id: "message".to_string(),
        task_id: "task".to_string(),
        server_message_data: String::new(),
        citations: vec![],
        message: Some(api::message::Message::ToolCall(api::message::ToolCall {
            tool_call_id: "tool_call".to_string(),
            tool: Some(
                api::message::tool_call::Tool::TransferShellCommandControlToUser(
                    api::message::tool_call::TransferShellCommandControlToUser {
                        reason: reason.clone(),
                    },
                ),
            ),
        })),
        request_id: "req".to_string(),
        timestamp: None,
    };

    let converted = message
        .to_client_output_message(ConversionParams {
            task_id: &task_id,
            active_code_review: None,
        })
        .expect("transfer-control conversion should succeed");

    match converted {
        MaybeAIAgentOutputMessage::Message(output) => match output.message {
            AIAgentOutputMessageType::Action(action) => {
                assert_eq!(action.task_id, task_id);
                assert_eq!(
                    action.action,
                    AIAgentActionType::TransferShellCommandControlToUser { reason }
                );
                assert!(action.requires_result);
            }
            other => panic!("Expected action message, got {other:?}"),
        },
        MaybeAIAgentOutputMessage::NoClientRepresentation => {
            panic!("Expected transfer-control tool call to produce a client action")
        }
    }
}

#[test]
fn a_remember_tool_call_is_an_action() {
    let task_id = TaskId::new("task".to_string());
    let message = api::Message {
        id: "message".to_string(),
        task_id: "task".to_string(),
        message: Some(api::message::Message::ToolCall(api::message::ToolCall {
            tool_call_id: "tool_call".to_string(),
            tool: Some(api::message::tool_call::Tool::Remember(
                api::message::tool_call::Remember {
                    content: "Tests run with cargo nextest.".to_string(),
                    scope: api::message::tool_call::remember::Scope::Global as i32,
                },
            )),
        })),
        ..Default::default()
    };

    let converted = message
        .to_client_output_message(ConversionParams {
            task_id: &task_id,
            active_code_review: None,
        })
        .expect("remember conversion should succeed");

    let MaybeAIAgentOutputMessage::Message(output) = converted else {
        panic!("Expected a remember tool call to produce a client action");
    };
    let AIAgentOutputMessageType::Action(action) = output.message else {
        panic!("Expected an action message, got {:?}", output.message);
    };
    assert_eq!(
        action.action,
        AIAgentActionType::Remember {
            content: "Tests run with cargo nextest.".to_string(),
            scope: MemoryScope::Global,
        }
    );
    assert!(action.requires_result);
}

/// A tool result that `local_inference` stores in the task during a live session is not drawn a
/// second time: the exchange already shows the result the client produced.
#[test]
fn a_tool_call_result_has_no_client_representation() {
    let task_id = TaskId::new("task-id".to_string());
    let message = api::Message {
        id: "result".to_string(),
        task_id: "task-id".to_string(),
        message: Some(api::message::Message::ToolCallResult(
            api::message::ToolCallResult {
                tool_call_id: "call-1".to_string(),
                context: None,
                result: Some(api::message::tool_call_result::Result::Cancel(())),
            },
        )),
        ..Default::default()
    };

    let output = message
        .to_client_output_message(ConversionParams {
            task_id: &task_id,
            active_code_review: None,
        })
        .expect("conversion should succeed");

    assert!(matches!(
        output,
        MaybeAIAgentOutputMessage::NoClientRepresentation
    ));
}

fn conversation_summary_message(tokens_before: u64, tokens_after: u64) -> api::Message {
    api::Message {
        id: "summary-id".to_string(),
        task_id: "task-id".to_string(),
        message: Some(api::message::Message::Summarization(
            api::message::Summarization {
                finished_duration: Some(prost_types::Duration {
                    seconds: 2,
                    nanos: 0,
                }),
                summary_type: Some(
                    api::message::summarization::SummaryType::ConversationSummary(
                        api::message::summarization::ConversationSummary {
                            summary: "## Goal\nShip it.".to_string(),
                            tokens_before,
                            tokens_after,
                            ..Default::default()
                        },
                    ),
                ),
            },
        )),
        ..Default::default()
    }
}

fn compacted_tokens_of(message: api::Message) -> Option<crate::ai::agent::CompactedTokens> {
    let task_id = TaskId::new("task-id".to_string());
    let MaybeAIAgentOutputMessage::Message(output) = message
        .to_client_output_message(ConversionParams {
            task_id: &task_id,
            active_code_review: None,
        })
        .expect("conversion should succeed")
    else {
        panic!("expected an output message");
    };
    let AIAgentOutputMessageType::Summarization {
        compacted_tokens, ..
    } = output.message
    else {
        panic!("expected a summarization");
    };
    compacted_tokens
}

#[test]
fn a_compacted_summary_carries_its_token_change() {
    assert_eq!(
        compacted_tokens_of(conversation_summary_message(182_000, 24_000)),
        Some(crate::ai::agent::CompactedTokens {
            before: 182_000,
            after: 24_000,
        })
    );
    assert_eq!(
        compacted_tokens_of(conversation_summary_message(0, 0)),
        None
    );
    assert_eq!(
        compacted_tokens_of(conversation_summary_message(182_000, 0)),
        None
    );
}
