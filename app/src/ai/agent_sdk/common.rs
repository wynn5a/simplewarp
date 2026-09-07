//! Common utilities for agent SDK commands.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use futures::TryFutureExt;
use warp_cli::agent::Harness;
use warpui::r#async::FutureExt;
use warpui::{AppContext, GetSingletonModelHandle, SingletonEntity as _, UpdateModel};

use crate::ai::agent::conversation::ServerAIConversationMetadata;
use crate::ai::agent_sdk::driver::{AgentDriverError, WARP_DRIVE_SYNC_TIMEOUT};
use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::llms::{LLMId, LLMPreferences};
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::server_api::ServerApiProvider;
use crate::server::server_api::ai::AIClient;
use crate::workspaces::update_manager::TeamUpdateManager;

/// How long to wait for workspace metadata to refresh.
pub const WORKSPACE_METADATA_REFRESH_TIMEOUT: Duration = Duration::from_secs(10);

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

pub(super) fn set_ambient_task_context_from_run_id(
    ctx: &AppContext,
    run_id: &str,
) -> anyhow::Result<AmbientAgentTaskId> {
    let task_id = parse_ambient_task_id(run_id, "Invalid run ID")?;
    ServerApiProvider::handle(ctx)
        .as_ref(ctx)
        .get()
        .set_ambient_agent_task_id(Some(task_id));
    Ok(task_id)
}

/// Refresh workspace metadata before executing an operation.
///
/// This ensures that team state is up-to-date before creating cloud objects or performing
/// other operations that depend on team membership.
pub fn refresh_workspace_metadata<C>(
    ctx: &mut C,
) -> impl Future<Output = anyhow::Result<()>> + Send + 'static + use<C>
where
    C: GetSingletonModelHandle + UpdateModel,
{
    let refresh_future = TeamUpdateManager::handle(ctx).update(ctx, |manager, ctx| {
        manager
            .refresh_workspace_metadata(ctx)
            .with_timeout(WORKSPACE_METADATA_REFRESH_TIMEOUT)
    });

    async move {
        let _ = refresh_future
            .await
            .map_err(|_| anyhow::anyhow!("Timed out refreshing team metadata"))?;
        Ok(())
    }
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
