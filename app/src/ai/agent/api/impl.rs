use std::collections::HashMap;
use std::sync::Arc;

use agent_harness::budget::estimate_tokens;
use futures_util::StreamExt;
use warp_multi_agent_api as api;

use super::convert_to::convert_input;
use super::{ConvertToAPITypeError, RequestParams, ResponseStream};
use crate::ai::agent::{AIAgentInput, redaction};
use crate::ai::agent_memory::{RequestMemories, RuleText};
use crate::server::server_api::{AIApiError, ProviderFailure};
use crate::terminal::model::session::SessionType;

pub async fn generate_multi_agent_output(
    mut params: RequestParams,
    cancellation_rx: futures::channel::oneshot::Receiver<()>,
) -> Result<ResponseStream, ConvertToAPITypeError> {
    let supported_tools = params
        .supported_tools_override
        .take()
        .unwrap_or_else(|| get_supported_tools(&params));
    let mut logging_metadata = HashMap::new();
    if let Some(metadata) = params.metadata {
        logging_metadata.insert(
            "is_autodetected_user_query".to_owned(),
            prost_types::Value {
                kind: Some(prost_types::value::Kind::BoolValue(
                    metadata.is_autodetected_user_query,
                )),
            },
        );
        logging_metadata.insert(
            "entrypoint".to_owned(),
            prost_types::Value {
                kind: Some(prost_types::value::Kind::StringValue(
                    metadata.entrypoint.entrypoint(),
                )),
            },
        );
        logging_metadata.insert(
            "is_auto_resume_after_error".to_owned(),
            prost_types::Value {
                kind: Some(prost_types::value::Kind::BoolValue(
                    metadata.is_auto_resume_after_error,
                )),
            },
        );
    }

    if params.should_redact_secrets {
        redaction::redact_inputs(&mut params.input);
    }

    let mut input = convert_input(params.input)?;
    add_memories(&mut input, &params.memories);

    let request = api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: params.tasks,
        }),
        input: Some(input),
        settings: Some(api::request::Settings {
            model_config: Some(api::request::settings::ModelConfig {
                base: params.model.into(),
                cli_agent: params.cli_agent_model.into(),
                ..Default::default()
            }),
            rules_enabled: params.is_memory_enabled,
            // Warp Drive context fell with the drive surface; the wire field
            // belongs to the external API contract and can never be enabled.
            warp_drive_context_enabled: false,
            supports_parallel_tool_calls: true,
            supports_create_files: true,
            supported_tools: supported_tools.into_iter().map(Into::into).collect(),
            supports_long_running_commands: true,
            should_preserve_file_content_in_history: true,
            supports_linked_code_blocks: true,
            supports_read_image_files: true,
            supports_reasoning_message: true,
            api_keys: params.api_keys,
            autonomy_level: params.autonomy_level.into(),
            isolation_level: params.isolation_level.into(),
            supports_v4a_file_diffs: true,
            supports_bundled_skills: true,
            custom_model_providers: params.custom_model_providers,
            will_retry_failures_before_reply: params.will_retry_failures_before_reply,
            ..Default::default()
        }),
        metadata: Some(api::request::Metadata {
            logging: logging_metadata,
            conversation_id: params
                .conversation_token
                .as_ref()
                .map(|token| token.as_str().to_string())
                .unwrap_or_default(),
            ambient_agent_task_id: String::new(),
            forked_from_conversation_id: if params.conversation_token.is_none() {
                // We only include this param on our initial request to the server
                // (when the forked conversation has not been assigned a new id yet).
                params
                    .forked_from_conversation_token
                    .map(|token| token.as_str().to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            },
            parent_agent_id: String::new(),
            agent_name: String::new(),
        }),
        existing_suggestions: None,
        mcp_context: params.mcp_context.map(Into::into),
    };

    // SimpleWarp runs the model call on this machine, so the request never leaves for a Warp
    // server and the API keys inside it never do either. The event stream has the same shape,
    // so everything downstream is unchanged.
    match local_inference::generate_local_output(&request).await {
        Ok(stream) => {
            let output_stream = stream
                .then(|result| async {
                    match result {
                        Ok(event) => Ok(event),
                        Err(local_inference::Failure {
                            error,
                            before_reply,
                        }) => Err(convert_local_inference_failure(error, before_reply)),
                    }
                })
                .take_until(cancellation_rx);
            Ok(Box::pin(output_stream))
        }
        Err(error) => {
            let (tx, rx) = async_channel::unbounded();
            let _ = tx
                .send(Err(convert_local_inference_failure(
                    error, /*before_reply*/ true,
                )))
                .await;
            Ok(Box::pin(rx))
        }
    }
}

/// Estimated tokens that `inputs` add to the conversation the model sees: the question with its
/// attachments and each tool result, rendered as `local_inference` renders them for the model.
/// Inputs that cannot be converted count as nothing.
pub fn estimate_input_tokens(inputs: Vec<AIAgentInput>) -> u64 {
    if inputs.is_empty() {
        return 0;
    }
    convert_input(inputs)
        .map(|input| estimate_tokens(&local_inference::convert::input_text(&input)))
        .unwrap_or_default()
}

/// Hands the user's own rules and the agent's memories to the local adapter as project-rules
/// entries.
///
/// The request has no field for them, because the server used to read them from the user's
/// account. Rules for all work go in an entry with no root path, which is rendered as the user's
/// rules for all their work. Rules for the current project join the entry of the project's rule
/// files when there is one, so that the project's rules stay together.
fn add_memories(input: &mut api::request::Input, memories: &RequestMemories) {
    if memories.global.is_empty() && memories.project.is_none() {
        return;
    }
    let context = input.context.get_or_insert_with(Default::default);
    if !memories.global.is_empty() {
        context
            .project_rules
            .push(api::input_context::ProjectRules {
                root_path: String::new(),
                active_rule_files: rule_files(&memories.global),
                additional_rule_file_paths: vec![],
            });
    }
    if let Some(project) = &memories.project {
        match context
            .project_rules
            .iter_mut()
            .find(|rules| rules.root_path == project.root_path)
        {
            Some(rules) => rules.active_rule_files.extend(rule_files(&project.rules)),
            None => context
                .project_rules
                .push(api::input_context::ProjectRules {
                    root_path: project.root_path.clone(),
                    active_rule_files: rule_files(&project.rules),
                    additional_rule_file_paths: vec![],
                }),
        }
    }
}

fn rule_files(rules: &[RuleText]) -> Vec<api::FileContent> {
    rules
        .iter()
        .map(|(title, content)| api::FileContent {
            file_path: title.clone(),
            content: content.clone(),
            line_range: None,
        })
        .collect()
}

/// Maps a failed local-inference request onto [`AIApiError::Provider`]: the error the AI UI
/// already renders, with the facts that decide whether the request is sent again.
fn convert_local_inference_failure(
    error: local_inference::Error,
    before_reply: bool,
) -> Arc<AIApiError> {
    let failure = ProviderFailure {
        before_reply,
        retryable: error.is_retryable(),
        retry_after: error.retry_after(),
    };
    Arc::new(AIApiError::Provider {
        error: convert_local_inference_error(error),
        failure,
    })
}

/// Maps a local-inference failure onto the error type that the AI UI already renders.
///
/// A missing key and a rejected key are the two failures a user can fix themselves, so they keep
/// their own messages rather than falling into the generic bucket.
fn convert_local_inference_error(error: local_inference::Error) -> Arc<AIApiError> {
    use local_inference::Error as LocalError;

    let error = match error {
        LocalError::NoApiKey { ref model } => AIApiError::Other(anyhow::anyhow!(
            "No API key is set for model `{model}`. Add one in Settings > AI."
        )),
        LocalError::ProviderStatus {
            status, ref body, ..
        } => match http::StatusCode::from_u16(status) {
            Ok(status) => AIApiError::ErrorStatus(status, body.clone()),
            Err(_) => AIApiError::Other(anyhow::anyhow!("{error}")),
        },
        // A rate limit reported inside the stream is the same limit as an HTTP 429, so it takes
        // the quota path. The body mirrors a provider error body so its message is shown.
        LocalError::ProviderStream {
            ref code,
            ref message,
        } if is_rate_limit_code(code) => AIApiError::ErrorStatus(
            http::StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({ "error": { "code": code, "message": message } }).to_string(),
        ),
        LocalError::Http(error) => AIApiError::Transport(error),
        other => AIApiError::Other(anyhow::anyhow!("{other}")),
    };
    Arc::new(error)
}

fn is_rate_limit_code(code: &str) -> bool {
    matches!(
        code,
        // OpenRouter reports a rate limit inside the stream with the numeric code 429.
        "429"
            | "rate_limit_exceeded"
            | "insufficient_quota"
            | "subscription_sharing_usage_limit_exceeded"
    )
}

fn get_supported_tools(params: &RequestParams) -> Vec<api::ToolType> {
    let mut supported_tools = vec![
        api::ToolType::Grep,
        api::ToolType::FileGlobV2,
        api::ToolType::ReadMcpResource,
        api::ToolType::CallMcpTool,
        api::ToolType::RunShellCommand,
        api::ToolType::WriteToLongRunningShellCommand,
        api::ToolType::ReadShellCommandOutput,
        api::ToolType::TransferShellCommandControlToUser,
    ];
    if params.is_memory_enabled {
        supported_tools.push(api::ToolType::Remember);
    }

    match params.session_context.session_type() {
        None | Some(SessionType::Local) => {
            supported_tools.extend(&[
                api::ToolType::ReadFiles,
                api::ToolType::ApplyFileDiffs,
                api::ToolType::InsertReviewComments,
            ]);
        }
        Some(SessionType::WarpifiedRemote) => {}
    }

    supported_tools
}

#[cfg(test)]
#[path = "impl_tests.rs"]
mod tests;
