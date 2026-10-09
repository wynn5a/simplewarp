use std::collections::HashMap;
use std::sync::Arc;

use futures_util::StreamExt;
use warp_core::features::FeatureFlag;
use warp_multi_agent_api as api;

use super::convert_to::convert_input;
use super::{ConvertToAPITypeError, RequestParams, ResponseStream};
use crate::ai::agent::redaction;
use crate::server::server_api::AIApiError;
use crate::terminal::model::session::SessionType;

pub async fn generate_multi_agent_output(
    mut params: RequestParams,
    cancellation_rx: futures::channel::oneshot::Receiver<()>,
) -> Result<ResponseStream, ConvertToAPITypeError> {
    let supported_tools = params
        .supported_tools_override
        .take()
        .unwrap_or_else(|| get_supported_tools(&params));
    let supported_cli_agent_tools = get_supported_cli_agent_tools(&params);
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

    let api_keys = api_keys_with_warp_credit_fallback_setting(
        params.api_keys,
        params.allow_use_of_warp_credits,
    );

    let mut input = convert_input(params.input)?;
    add_global_rules(&mut input, &params.global_rules);

    let request = api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: params.tasks,
        }),
        input: Some(input),
        settings: Some(api::request::Settings {
            model_config: Some(api::request::settings::ModelConfig {
                base: params.model.into(),
                cli_agent: params.cli_agent_model.into(),
                base_model_context_window_limit: params.context_window_limit.unwrap_or(0),
                ..Default::default()
            }),
            rules_enabled: params.is_memory_enabled,
            // Warp Drive context fell with the drive surface; the wire field
            // belongs to the external API contract and can never be enabled.
            warp_drive_context_enabled: false,
            web_context_retrieval_enabled: true,
            supports_parallel_tool_calls: true,
            use_anthropic_text_editor_tools: false,
            planning_enabled: params.planning_enabled,
            supports_create_files: true,
            supported_tools: supported_tools.into_iter().map(Into::into).collect(),
            supports_long_running_commands: true,
            should_preserve_file_content_in_history: true,
            supports_todos_ui: true,
            supports_linked_code_blocks: FeatureFlag::LinkedCodeBlocks.is_enabled(),
            supports_started_child_task_message: true,
            supports_suggest_prompt: true,
            supports_read_image_files: FeatureFlag::ReadImageFiles.is_enabled(),
            supports_reasoning_message: true,
            api_keys,
            autonomy_level: params.autonomy_level.into(),
            isolation_level: params.isolation_level.into(),
            web_search_enabled: false,
            supported_cli_agent_tools: supported_cli_agent_tools
                .into_iter()
                .map(Into::into)
                .collect(),
            supports_v4a_file_diffs: FeatureFlag::V4AFileDiffs.is_enabled(),
            supports_summarization_via_message_replacement: false,
            supports_bundled_skills: FeatureFlag::BundledSkills.is_enabled(),
            supports_research_agent: params.research_agent_enabled,
            supports_orchestration_v2: false,
            supports_orchestration_runners: false,
            supports_background_computer_use: false,
            custom_model_providers: params.custom_model_providers,
            custom_model_routers: None,
        }),
        metadata: Some(api::request::Metadata {
            logging: logging_metadata,
            conversation_id: params
                .conversation_token
                .as_ref()
                .map(|token| token.as_str().to_string())
                .unwrap_or_default(),
            ambient_agent_task_id: params
                .ambient_agent_task_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
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
            parent_agent_id: params.parent_agent_id.unwrap_or_default(),
            agent_name: params.agent_name.unwrap_or_default(),
        }),
        existing_suggestions: params
            .existing_suggestions
            .map(|suggestions| suggestions.into()),
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
                        Err(error) => Err(convert_local_inference_error(error)),
                    }
                })
                .take_until(cancellation_rx);
            Ok(Box::pin(output_stream))
        }
        Err(error) => {
            let (tx, rx) = async_channel::unbounded();
            let _ = tx.send(Err(convert_local_inference_error(error))).await;
            Ok(Box::pin(rx))
        }
    }
}

/// Hands the user's own rules to the local adapter as a project-rules entry with no root path.
///
/// The request has no field for them, because the server used to read them from the user's
/// account. An entry with no root path is rendered as the user's rules for all their work.
fn add_global_rules(input: &mut api::request::Input, rules: &[(String, String)]) {
    if rules.is_empty() {
        return;
    }
    let context = input.context.get_or_insert_with(Default::default);
    context
        .project_rules
        .push(api::input_context::ProjectRules {
            root_path: String::new(),
            active_rule_files: rules
                .iter()
                .map(|(name, content)| api::FileContent {
                    file_path: if name.is_empty() {
                        "Rule".to_owned()
                    } else {
                        name.clone()
                    },
                    content: content.clone(),
                    line_range: None,
                })
                .collect(),
            additional_rule_file_paths: vec![],
        });
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
        LocalError::ProviderStatus { status, ref body } => {
            match http::StatusCode::from_u16(status) {
                Ok(status) => AIApiError::ErrorStatus(status, body.clone()),
                Err(_) => AIApiError::Other(anyhow::anyhow!("{error}")),
            }
        }
        LocalError::Http(error) => AIApiError::Transport(error),
        other => AIApiError::Other(anyhow::anyhow!("{other}")),
    };
    Arc::new(error)
}

fn api_keys_with_warp_credit_fallback_setting(
    api_keys: Option<api::request::settings::ApiKeys>,
    allow_use_of_warp_credits: bool,
) -> Option<api::request::settings::ApiKeys> {
    match api_keys {
        Some(mut api_keys) => {
            api_keys.allow_use_of_warp_credits = allow_use_of_warp_credits;
            Some(api_keys)
        }
        None if allow_use_of_warp_credits => Some(api::request::settings::ApiKeys {
            allow_use_of_warp_credits: true,
            ..Default::default()
        }),
        None => None,
    }
}

fn get_supported_tools(params: &RequestParams) -> Vec<api::ToolType> {
    let mut supported_tools = vec![
        api::ToolType::Grep,
        api::ToolType::FileGlob,
        api::ToolType::FileGlobV2,
        api::ToolType::ReadMcpResource,
        api::ToolType::CallMcpTool,
        api::ToolType::InitProject,
        api::ToolType::OpenCodeReview,
        api::ToolType::RunShellCommand,
        api::ToolType::SuggestNewConversation,
        api::ToolType::Subagent,
        api::ToolType::WriteToLongRunningShellCommand,
        api::ToolType::ReadShellCommandOutput,
        api::ToolType::ReadDocuments,
        api::ToolType::CreateDocuments,
        api::ToolType::EditDocuments,
        api::ToolType::SuggestPrompt,
    ];

    if FeatureFlag::ConversationsAsContext.is_enabled() {
        supported_tools.push(api::ToolType::FetchConversation);
    }

    match params.session_context.session_type() {
        None | Some(SessionType::Local) => {
            supported_tools.extend(&[api::ToolType::ReadFiles, api::ToolType::ApplyFileDiffs]);
        }
        Some(SessionType::WarpifiedRemote) => {}
    }

    supported_tools.push(api::ToolType::InsertReviewComments);

    supported_tools
}

fn get_supported_cli_agent_tools(params: &RequestParams) -> Vec<api::ToolType> {
    let mut supported_cli_agent_tools = vec![
        api::ToolType::WriteToLongRunningShellCommand,
        api::ToolType::ReadShellCommandOutput,
        api::ToolType::Grep,
        api::ToolType::FileGlob,
        api::ToolType::FileGlobV2,
    ];

    if FeatureFlag::TransferControlTool.is_enabled() {
        supported_cli_agent_tools.push(api::ToolType::TransferShellCommandControlToUser);
    }

    match params.session_context.session_type() {
        None | Some(SessionType::Local) => {
            supported_cli_agent_tools.push(api::ToolType::ReadFiles);
        }
        Some(SessionType::WarpifiedRemote) => {}
    }

    supported_cli_agent_tools
}

#[cfg(test)]
#[path = "impl_tests.rs"]
mod tests;
