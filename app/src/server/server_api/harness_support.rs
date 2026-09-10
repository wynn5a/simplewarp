// We don't directly run agent harnesses on WASM, so this code is unused.
#![cfg_attr(target_family = "wasm", expect(dead_code))]

use std::collections::HashMap;

use anyhow::{Context, Result};

use super::ServerApi;
#[cfg(feature = "local_fs")]
pub use super::presigned_upload::FileUploadBody;
#[cfg(not(target_family = "wasm"))]
use crate::ai::agent_sdk::retry::with_bounded_retry;
use crate::ai::ambient_agents::AmbientAgentTaskId;

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

#[cfg(test)]
#[path = "harness_support_tests.rs"]
mod tests;
