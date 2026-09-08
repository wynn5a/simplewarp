// We don't directly run agent harnesses on WASM, so this code is unused.
#![cfg_attr(target_family = "wasm", expect(dead_code))]

use std::collections::HashMap;

use anyhow::{Context, Result};
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;

use super::ServerApi;
#[cfg(feature = "local_fs")]
pub use super::presigned_upload::FileUploadBody;
pub use super::presigned_upload::UploadBody;
use crate::ai::agent::conversation::AIConversationId;
#[cfg(not(target_family = "wasm"))]
use crate::ai::agent_sdk::retry::with_bounded_retry;
use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::artifacts::Artifact;

/// A presigned upload target returned by the server.
#[serde_with::serde_as]
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UploadTarget {
    pub url: String,
    pub method: String,
    #[serde(default)]
    #[serde_as(deserialize_as = "serde_with::DefaultOnNull")]
    pub headers: HashMap<String, String>,
    /// Ordered multipart form fields for POST uploads.
    #[serde(default)]
    #[serde_as(deserialize_as = "serde_with::DefaultOnNull")]
    pub fields: Vec<UploadField>,
}

/// A single multipart form field on a POST upload target.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UploadField {
    pub name: String,
    pub value: UploadFieldValue,
}

/// Descriptor for a field value when uploading to an [`UploadTarget`].
/// This is currently only used for `POST` requests, but may be supported
/// for HTTP headers in the future.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UploadFieldValue {
    /// Literal string value known at URL-generation time.
    Static { value: String },
    /// Client should compute CRC32C of the upload, base64-encode the 4-byte
    /// big-endian result, and send it as this field's value.
    // `snake_case` would derive `content_crc32_c`, which does not match the
    // `ContentCRC32CFieldValue` discriminator in warp-server's OpenAPI schema.
    #[serde(rename = "content_crc32c")]
    ContentCrc32C,
    /// Client should use the raw upload bytes as this field's value.
    ContentData,
}

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
    pub prompt: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Optional user-turn preamble for resumed third-party harness sessions. The harness
    /// decides how to surface this — Claude Code prepends it to the user-turn prompt fed
    /// into the CLI so the agent treats it as immediate intent rather than background
    /// system context. Empty when no resumption is in effect.
    #[serde(default)]
    pub resumption_prompt: Option<String>,
    /// Optional server-retrieved context relevant to the task prompt. Each harness
    /// decides how to inject this — typically by prepending it to the user-turn prompt
    /// after any resumption preamble.
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ReportArtifactResponse {
    pub artifact_uid: String,
}

/// Trait for API endpoints used to support third-party agent harnesses in Oz.
#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait HarnessSupportClient: 'static + Send + Sync {
    /// Create a new external conversation for a third-party harness.
    async fn create_external_conversation(&self, format: &str) -> Result<AIConversationId>;

    /// Get a presigned upload target for the conversation's raw transcript.
    async fn get_transcript_upload_target(
        &self,
        conversation_id: &AIConversationId,
    ) -> Result<UploadTarget>;

    /// Get a presigned upload target for the conversation's block snapshot.
    async fn get_block_snapshot_upload_target(
        &self,
        conversation_id: &AIConversationId,
    ) -> Result<UploadTarget>;

    /// Resolve the prompt for a third-party harness run for a task stored on the server.
    async fn resolve_prompt(&self, request: ResolvePromptRequest) -> Result<ResolvedHarnessPrompt>;

    /// Report an artifact created by a third-party harness back to the Oz platform.
    async fn report_artifact(&self, artifact: &Artifact) -> Result<ReportArtifactResponse>;

    /// Send a progress notification to the task's originating platform.
    async fn notify_user(&self, message: &str) -> Result<()>;

    /// Report task completion or failure. The server derives PR links/branches from
    /// artifacts already reported via `report_artifact`.
    async fn finish_task(&self, success: bool, summary: &str) -> Result<()>;

    /// Report a clean shutdown of the agent process.
    async fn report_clean_shutdown(&self) -> Result<()>;

    /// Report an error shutdown of the agent process.
    async fn report_error_shutdown(
        &self,
        error_category: String,
        error_message: String,
    ) -> Result<()>;

    /// Download the raw third-party harness transcript bytes for the current task's
    /// conversation.
    ///
    /// Hits `GET /harness-support/transcript`, which redirects to a signed GCS URL.
    /// The conversation is resolved from the task's `agent_conversation_id` server-side,
    /// so callers do not pass a conversation id. Each harness deserializes the returned
    /// bytes into its own envelope shape (e.g. Claude Code parses
    /// `ClaudeTranscriptEnvelope`). Transient failures retry with bounded exponential
    /// backoff; permanent 4xx (e.g. 404 "no transcript") fail fast so the caller can
    /// surface a resume-specific error.
    async fn fetch_transcript(&self) -> Result<bytes::Bytes>;

    /// Get an HTTP client to use with [`UploadTarget`]s for saving blobs.
    fn http_client(&self) -> &http_client::Client;
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

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl HarnessSupportClient for ServerApi {
    async fn create_external_conversation(&self, _format: &str) -> Result<AIConversationId> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_transcript_upload_target(
        &self,
        _conversation_id: &AIConversationId,
    ) -> Result<UploadTarget> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_block_snapshot_upload_target(
        &self,
        _conversation_id: &AIConversationId,
    ) -> Result<UploadTarget> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn resolve_prompt(
        &self,
        _request: ResolvePromptRequest,
    ) -> Result<ResolvedHarnessPrompt> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn report_artifact(&self, _artifact: &Artifact) -> Result<ReportArtifactResponse> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn notify_user(&self, _message: &str) -> Result<()> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn finish_task(&self, _success: bool, _summary: &str) -> Result<()> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn report_clean_shutdown(&self) -> Result<()> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn report_error_shutdown(
        &self,
        _error_category: String,
        _error_message: String,
    ) -> Result<()> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn fetch_transcript(&self) -> Result<bytes::Bytes> {
        Err(crate::server::server_api::local_only_error())
    }

    fn http_client(&self) -> &http_client::Client {
        self.base_client.http_client()
    }
}

/// Upload a blob to a presigned upload target.
pub async fn upload_to_target(
    http_client: &http_client::Client,
    target: &UploadTarget,
    body: impl UploadBody,
) -> Result<()> {
    super::presigned_upload::upload_to_target(http_client, target, body).await
}

#[cfg(test)]
#[path = "harness_support_tests.rs"]
mod tests;
