//! Common utilities for agent SDK commands.

use std::future::Future;
use std::sync::Arc;

use futures::TryFutureExt;
use warp_cli::agent::Harness;
use warpui::r#async::FutureExt;
use warpui::{AppContext, SingletonEntity as _};

use crate::ai::agent::conversation::ServerAIConversationMetadata;
use crate::ai::agent_sdk::driver::{AgentDriverError, WARP_DRIVE_SYNC_TIMEOUT};
use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::llms::{LLMId, LLMPreferences};
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::server_api::ai::AIClient;

pub fn validate_agent_mode_base_model_id(
    model_id: &str,
    ctx: &AppContext,
) -> anyhow::Result<LLMId> {
    let llm_prefs = LLMPreferences::as_ref(ctx);
    let valid_ids = llm_prefs
        .get_base_llm_choices_for_agent_mode(ctx)
        .map(|info| info.id.clone())
        .collect::<Vec<_>>();

    classify_agent_mode_base_model_id(
        model_id,
        &valid_ids,
        llm_prefs.agent_mode_models_unavailable(),
    )
}

/// Classifies a user-supplied agent-mode model id against the available model
/// list, distinguishing "the model list fetch failed (so the list is empty or
/// stale)" from "the id is genuinely not in a valid list".
fn classify_agent_mode_base_model_id(
    model_id: &str,
    valid_ids: &[LLMId],
    list_unavailable: bool,
) -> anyhow::Result<LLMId> {
    let llm_id: LLMId = model_id.into();
    if valid_ids.contains(&llm_id) {
        Ok(llm_id)
    } else if list_unavailable {
        Err(anyhow::anyhow!(
            "Could not retrieve the agent-mode model list from the server \
             (the request failed or returned no models). Try again later."
        ))
    } else {
        let suggestions = valid_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        Err(anyhow::anyhow!(
            "Unknown model id '{model_id}'. Try one of: {suggestions}"
        ))
    }
}

pub(super) fn parse_ambient_task_id(
    run_id: &str,
    error_prefix: &str,
) -> anyhow::Result<AmbientAgentTaskId> {
    run_id
        .parse()
        .map_err(|err| anyhow::anyhow!("{error_prefix} '{run_id}': {err}"))
}

/// Refresh Warp Drive before executing an operation.
pub fn refresh_warp_drive(
    ctx: &AppContext,
) -> impl Future<Output = anyhow::Result<()>> + Send + 'static + use<> {
    UpdateManager::as_ref(ctx)
        .initial_load_complete()
        .with_timeout(WARP_DRIVE_SYNC_TIMEOUT)
        .map_err(|_| anyhow::anyhow!("Timed out waiting for Warp Drive to sync"))
}

/// Fetch the conversation's server metadata and validate that its harness matches the caller's
/// `--harness` choice. Returns the metadata on success so the caller can reuse it (e.g. for the
/// server conversation token).
///
/// Called up-front before any task/config-build logic consumes `args.harness`, so a mismatch
/// error surfaces before side effects like task creation. We deliberately do NOT auto-upgrade
/// the harness: `Harness::Oz` default with a Claude conversation id is treated as a mismatch
/// and errors out.
pub(super) async fn fetch_and_validate_conversation_harness(
    ai_client: Arc<dyn AIClient>,
    conversation_id: &str,
    args_harness: Harness,
) -> Result<ServerAIConversationMetadata, AgentDriverError> {
    let metadata = ai_client
        .list_ai_conversation_metadata(Some(vec![conversation_id.to_string()]))
        .await
        .map_err(|e| AgentDriverError::ConversationLoadFailed(format!("{e:#}")))?
        .into_iter()
        .next()
        .ok_or_else(|| {
            AgentDriverError::ConversationLoadFailed(format!(
                "conversation {conversation_id} not found or not accessible"
            ))
        })?;

    if metadata.harness != args_harness {
        return Err(AgentDriverError::ConversationHarnessMismatch {
            conversation_id: conversation_id.to_string(),
            expected: Harness::from(metadata.harness).to_string(),
            got: args_harness.to_string(),
        });
    }

    Ok(metadata)
}

#[cfg(test)]
#[path = "common_tests.rs"]
mod tests;
