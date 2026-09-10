// We don't directly run agent harnesses on WASM, so this code is unused.
#![cfg_attr(target_family = "wasm", expect(dead_code))]

use anyhow::{Context, Result};

use super::ServerApi;
#[cfg(feature = "local_fs")]
pub use super::presigned_upload::FileUploadBody;
#[cfg(not(target_family = "wasm"))]
use crate::ai::agent_sdk::retry::with_bounded_retry;
use crate::ai::ambient_agents::AmbientAgentTaskId;

/// Skill attached to a resolve-prompt request,
/// used when invoking a third-party harness with a skill
/// via the CLI.
#[derive(serde::Serialize)]
pub struct ResolvePromptAttachedSkill {
    pub name: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(serde::Serialize)]
pub struct ResolvePromptRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<ResolvePromptAttachedSkill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments_dir: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct ResolvedHarnessPrompt {
    /// Optional user-turn preamble for resumed third-party harness sessions. The harness
    /// decides how to surface this — Claude Code prepends it to the user-turn prompt fed
    /// into the CLI so the agent treats it as immediate intent rather than background
    /// system context. Empty when no resumption is in effect.
    #[serde(default)]
    pub resumption_prompt: Option<String>,
}

impl ServerApi {
    pub(crate) async fn get_public_api_response_for_task(
        &self,
        _task_id: &AmbientAgentTaskId,
        _path: &str,
    ) -> Result<http_client::Response> {
        Err(crate::server::server_api::local_only_error())
    }

    pub(crate) async fn post_public_api_response_for_task<B>(
        &self,
        _task_id: &AmbientAgentTaskId,
        _path: &str,
        _body: &B,
    ) -> Result<http_client::Response>
    where
        B: serde::Serialize,
    {
        Err(crate::server::server_api::local_only_error())
    }

    pub(crate) async fn resolve_prompt_for_task(
        &self,
        task_id: &AmbientAgentTaskId,
        request: ResolvePromptRequest,
    ) -> Result<ResolvedHarnessPrompt> {
        let response = self
            .post_public_api_response_for_task(task_id, "harness-support/resolve-prompt", &request)
            .await?;
        let url = response.url().clone();
        response
            .json::<ResolvedHarnessPrompt>()
            .await
            .with_context(|| format!("Failed to deserialize response from {url}"))
    }

    pub(crate) async fn fetch_transcript_for_task(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> Result<bytes::Bytes> {
        #[cfg(not(target_family = "wasm"))]
        {
            with_bounded_retry("fetch task-scoped harness-support transcript", || async {
                let response = self
                    .get_public_api_response_for_task(task_id, "harness-support/transcript")
                    .await?;
                response
                    .bytes()
                    .await
                    .context("Failed to read task-scoped harness-support transcript body")
            })
            .await
        }
        #[cfg(target_family = "wasm")]
        {
            let _ = task_id;
            unreachable!(
                "fetch_transcript_for_task is not supported on wasm; agent_sdk is not built on this target"
            );
        }
    }
}
