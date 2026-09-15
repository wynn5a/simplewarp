use std::collections::{HashMap, HashSet};
#[cfg(not(target_family = "wasm"))]
use std::path::Path;

use ai::index::full_source_code_embedding::store_client::{IntermediateNode, StoreClient};
use ai::index::full_source_code_embedding::{
    self, CodebaseContextConfig, ContentHash, EmbeddingConfig, NodeHash, RepoMetadata,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
#[cfg(test)]
use mockall::automock;
use warp_errors::report_error;
use warp_graphql::ai::{AgentTaskState, PlatformErrorCode};
use warp_multi_agent_api::ConversationData;

use super::ServerApi;
use super::presigned_upload::UploadField;
use crate::ai::RequestUsageInfo;
pub use crate::ai::agent::UserQueryMode;
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::{AIAgentHarness, ServerAIConversationMetadata};
use crate::ai::ambient_agents::AmbientAgentTaskId;
// Re-export ambient agent types for backwards compatibility
pub use crate::ai::ambient_agents::{
    AgentConfigSnapshot, AgentSource, AmbientAgentTask, AmbientAgentTaskState, ExecutionLocation,
    TaskStatusMessage, task::AttachmentInput,
};
use crate::ai::artifacts::Artifact;
use crate::ai::generate_code_review_content::api::{
    GenerateCodeReviewContentRequest, GenerateCodeReviewContentResponse,
};
use crate::ai::harness_availability::HarnessAvailability;
#[cfg(feature = "agent_mode_evals")]
use crate::ai::request_usage_model::RequestLimitInfo;
use crate::ai_assistant::execution_context::WarpAiExecutionContext;
use crate::ai_assistant::requests::GenerateDialogueResult;
use crate::ai_assistant::utils::TranscriptPart;
use crate::ai_assistant::{AIGeneratedCommand, GenerateCommandsFromNaturalLanguageError};
use crate::drive::workflows::ai_assist::{GeneratedCommandMetadata, GeneratedCommandMetadataError};
use crate::persistence::model::ConversationUsageMetadata;

/// A status update for a task, optionally including a platform error code.
pub struct TaskStatusUpdate {
    pub message: String,
    pub error_code: Option<PlatformErrorCode>,
}
fn public_api_user_query_mode(mode: UserQueryMode) -> &'static str {
    match mode {
        UserQueryMode::Normal => "normal",
        UserQueryMode::Plan => "plan",
        UserQueryMode::Orchestrate => "orchestrate",
    }
}

fn serialize_user_query_mode_for_public_api<S>(
    mode: &UserQueryMode,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(public_api_user_query_mode(*mode))
}

impl TaskStatusUpdate {
    /// Create a status update with just a message (no error code).
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_code: None,
        }
    }

    /// Create a status update with a message and error code.
    pub fn with_error_code(message: impl Into<String>, error_code: PlatformErrorCode) -> Self {
        Self {
            message: message.into(),
            error_code: Some(error_code),
        }
    }
}

/// JSON payload sent to the public `POST /agent/run` API.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpawnAgentRequest {
    /// None for skill-only or conversation-only invocations; omitted on the wire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// The public API accepts lowercase mode strings (`normal`, `plan`, or `orchestrate`).
    #[serde(serialize_with = "serialize_user_query_mode_for_public_api")]
    pub mode: UserQueryMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<AgentConfigSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<bool>,
    /// Agent identity UID to use as the execution principal for the run.
    #[serde(rename = "agent_identity_uid", skip_serializing_if = "Option::is_none")]
    pub agent_identity_uid: Option<String>,
    /// Use a Claude-compatible skill as the base prompt.
    /// Format: "repo:skill_name" or just "skill_name".
    /// The skill is resolved at runtime in the agent environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interactive: Option<bool>,
    /// Populated when a cloud agent spawns a child run via the public API.
    /// Not yet wired through the local start_agent flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    /// Base64-encoded `warp.multi_agent.v1.Skill` payloads to restore as runtime skills.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub runtime_skills: Vec<String>,
    /// Base64-encoded `warp.multi_agent.v1.Attachment` payloads to restore as referenced attachments.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub referenced_attachments: Vec<String>,
    /// Server-side conversation id to resume against (sets `task.AgentConversationID`).
    /// For local-to-cloud handoff this is the forked conversation id returned by
    /// `POST /agent/conversations/{conversation_id}/fork` at chip-click time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// When `Some(true)`, the cloud agent skips the end-of-run snapshot upload.
    /// Set by the client when cloud conversation storage is disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_disabled: Option<bool>,
    /// True when the source conversation was part of an orchestration tree at
    /// handoff time. Only set on local-to-cloud handoff spawns from an
    /// orchestrated source; absent otherwise. The server uses it to inject the
    /// universal hidden first-turn orchestration handoff message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration_handoff: Option<bool>,
}

/// Response body for `POST /agent/conversations/{conversation_id}/fork`. The returned id is sent
/// on the subsequent `POST /agent/runs` request under `conversation_id` (resume semantics).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ForkConversationResponse {
    pub forked_conversation_id: String,
}

/// Response body for `POST /agent/conversations/{conversation_id}/rename`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RenameConversationResponse {
    pub title: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunFollowupRequest {
    pub message: String,
}

// --- Orchestrations V2 messaging types ---

#[derive(Debug, Clone, serde::Serialize)]
pub struct SendAgentMessageRequest {
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub sender_run_id: String,
}

#[derive(Debug, Clone)]
pub struct ListAgentMessagesRequest {
    pub unread_only: bool,
    pub since: Option<String>,
    pub limit: i32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SendAgentMessageResponse {
    pub message_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentMessageHeader {
    pub message_id: String,
    pub sender_run_id: String,
    pub subject: String,
    pub sent_at: String,
    pub delivered_at: Option<String>,
    pub read_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentRunEvent {
    pub event_type: String,
    pub run_id: String,
    pub ref_id: Option<String>,
    pub execution_id: Option<String>,
    pub occurred_at: String,
    pub sequence: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReadAgentMessageResponse {
    pub message_id: String,
    pub sender_run_id: String,
    pub subject: String,
    pub body: String,
    pub sent_at: String,
    pub delivered_at: Option<String>,
    pub read_at: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct SpawnAgentResponse {
    pub task_id: AmbientAgentTaskId,
    pub run_id: String,
    #[serde(default)]
    pub at_capacity: bool,
}

/// Response from the artifact endpoint.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "artifact_type")]
pub enum ArtifactDownloadResponse {
    #[serde(rename = "SCREENSHOT")]
    Screenshot {
        #[serde(flatten)]
        common: ArtifactDownloadCommonFields,
        data: ScreenshotArtifactResponseData,
    },
    #[serde(rename = "FILE")]
    File {
        #[serde(flatten)]
        common: ArtifactDownloadCommonFields,
        data: FileArtifactResponseData,
    },
}

impl ArtifactDownloadResponse {
    fn common(&self) -> &ArtifactDownloadCommonFields {
        match self {
            ArtifactDownloadResponse::Screenshot { common, .. }
            | ArtifactDownloadResponse::File { common, .. } => common,
        }
    }

    pub fn artifact_uid(&self) -> &str {
        &self.common().artifact_uid
    }

    pub fn artifact_type(&self) -> &'static str {
        match self {
            ArtifactDownloadResponse::Screenshot { .. } => "SCREENSHOT",
            ArtifactDownloadResponse::File { .. } => "FILE",
        }
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.common().created_at
    }

    pub fn download_url(&self) -> &str {
        match self {
            ArtifactDownloadResponse::Screenshot { data, .. } => &data.download_url,
            ArtifactDownloadResponse::File { data, .. } => &data.download_url,
        }
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        match self {
            ArtifactDownloadResponse::Screenshot { data, .. } => data.expires_at,
            ArtifactDownloadResponse::File { data, .. } => data.expires_at,
        }
    }

    pub fn content_type(&self) -> &str {
        match self {
            ArtifactDownloadResponse::Screenshot { data, .. } => &data.content_type,
            ArtifactDownloadResponse::File { data, .. } => &data.content_type,
        }
    }

    pub fn filepath(&self) -> Option<&str> {
        match self {
            ArtifactDownloadResponse::Screenshot { .. } => None,
            ArtifactDownloadResponse::File { data, .. } => Some(&data.filepath),
        }
    }

    pub fn filename(&self) -> Option<&str> {
        match self {
            ArtifactDownloadResponse::Screenshot { .. } => None,
            ArtifactDownloadResponse::File { data, .. } => Some(&data.filename),
        }
    }

    pub fn description(&self) -> Option<&str> {
        match self {
            ArtifactDownloadResponse::Screenshot { data, .. } => data.description.as_deref(),
            ArtifactDownloadResponse::File { data, .. } => data.description.as_deref(),
        }
    }

    pub fn size_bytes(&self) -> Option<i64> {
        match self {
            ArtifactDownloadResponse::Screenshot { .. } => None,
            ArtifactDownloadResponse::File { data, .. } => data.size_bytes,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ArtifactDownloadCommonFields {
    pub artifact_uid: String,
    pub created_at: DateTime<Utc>,
}

/// Screenshot-specific data from the artifact endpoint.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ScreenshotArtifactResponseData {
    pub download_url: String,
    pub expires_at: DateTime<Utc>,
    pub content_type: String,
    pub description: Option<String>,
}

/// File-specific data from the artifact endpoint.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct FileArtifactResponseData {
    pub download_url: String,
    pub expires_at: DateTime<Utc>,
    pub content_type: String,
    pub filepath: String,
    pub filename: String,
    pub description: Option<String>,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct CreateFileArtifactUploadRequest {
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
    pub filepath: String,
    /// Short badge-visible title for the artifact (e.g. a recording title).
    pub title: Option<String>,
    pub description: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct FileArtifactRecord {
    pub artifact_uid: String,
    pub filepath: String,
    pub description: Option<String>,
    pub mime_type: String,
    pub size_bytes: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct FileArtifactUploadHeaderInfo {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct FileArtifactUploadTargetInfo {
    pub url: String,
    pub method: String,
    pub headers: Vec<FileArtifactUploadHeaderInfo>,
    /// Ordered multipart form fields for presigned POST uploads.
    pub fields: Vec<UploadField>,
}

#[derive(Debug, Clone)]
pub struct CreateFileArtifactUploadResponse {
    pub artifact: FileArtifactRecord,
    pub upload_target: FileArtifactUploadTargetInfo,
}

/// A single git credential entry returned by `taskGitCredentials`.
#[derive(Clone)]
pub struct GitCredential {
    /// The provider's OAuth or installation access token.
    pub token: String,
    /// The provider-specific git username, when available.
    pub username: Option<String>,
    /// The provider account's email, when available.
    pub email: Option<String>,
    /// The managed git host, such as `"github.com"` or `"gitlab.com"`.
    pub host: String,
}

/// Filter parameters for listing ambient agent tasks.
#[derive(Clone, Debug, Default)]
pub struct TaskListFilter {
    pub creator_uid: Option<String>,
    pub updated_after: Option<DateTime<Utc>>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub states: Option<Vec<AmbientAgentTaskState>>,
    pub source: Option<AgentSource>,
    pub execution_location: Option<ExecutionLocation>,
    pub environment_id: Option<String>,
    pub skill_spec: Option<String>,
    pub schedule_id: Option<String>,
    pub ancestor_run_id: Option<String>,
    pub config_name: Option<String>,
    pub model_id: Option<String>,
    pub artifact_type: Option<ArtifactType>,
    pub search_query: Option<String>,
    pub sort_by: Option<RunSortBy>,
    pub sort_order: Option<RunSortOrder>,
    pub cursor: Option<String>,
}

/// Artifact type filter values accepted by the public API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactType {
    Plan,
    PullRequest,
    Screenshot,
    File,
}

impl ArtifactType {
    pub fn as_query_param(&self) -> &'static str {
        match self {
            ArtifactType::Plan => "PLAN",
            ArtifactType::PullRequest => "PULL_REQUEST",
            ArtifactType::Screenshot => "SCREENSHOT",
            ArtifactType::File => "FILE",
        }
    }
}

/// Sort-by values accepted by the public API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunSortBy {
    UpdatedAt,
    CreatedAt,
    Title,
    Agent,
}

impl RunSortBy {
    pub fn as_query_param(&self) -> &'static str {
        match self {
            RunSortBy::UpdatedAt => "updated_at",
            RunSortBy::CreatedAt => "created_at",
            RunSortBy::Title => "title",
            RunSortBy::Agent => "agent",
        }
    }
}

/// Sort-order values accepted by the public API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunSortOrder {
    Asc,
    Desc,
}

impl RunSortOrder {
    pub fn as_query_param(&self) -> &'static str {
        match self {
            RunSortOrder::Asc => "asc",
            RunSortOrder::Desc => "desc",
        }
    }
}

#[derive(Clone, serde::Deserialize, Debug, PartialEq, Eq)]
pub struct ConnectedSelfHostedWorker {
    pub worker_host: String,
    pub connection_count: u32,
    pub connected_at: String,
    pub last_seen_at: String,
}

#[derive(Clone, serde::Deserialize, Debug, PartialEq, Eq)]
pub struct ListConnectedSelfHostedWorkersResponse {
    pub workers: Vec<ConnectedSelfHostedWorker>,
}

#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait AIClient: 'static + Send + Sync {
    async fn generate_commands_from_natural_language(
        &self,
        prompt: String,
        ai_execution_context: Option<WarpAiExecutionContext>,
    ) -> Result<Vec<AIGeneratedCommand>, GenerateCommandsFromNaturalLanguageError>;

    async fn generate_dialogue_answer(
        &self,
        transcript: Vec<TranscriptPart>,
        prompt: String,
        ai_execution_context: Option<WarpAiExecutionContext>,
    ) -> anyhow::Result<GenerateDialogueResult>;

    async fn generate_metadata_for_command(
        &self,
        command: String,
    ) -> Result<GeneratedCommandMetadata, GeneratedCommandMetadataError>;

    async fn get_request_limit_info(&self) -> Result<RequestUsageInfo, anyhow::Error>;

    async fn get_available_harnesses(&self) -> Result<Vec<HarnessAvailability>, anyhow::Error>;
    async fn list_connected_self_hosted_workers(
        &self,
    ) -> Result<ListConnectedSelfHostedWorkersResponse, anyhow::Error>;

    async fn provide_negative_feedback_response_for_ai_conversation(
        &self,
        conversation_id: String,
        request_ids: Vec<String>,
    ) -> anyhow::Result<i32, anyhow::Error>;

    async fn create_agent_task(
        &self,
        prompt: String,
        environment_uid: Option<String>,
        parent_run_id: Option<String>,
        config: Option<AgentConfigSnapshot>,
    ) -> anyhow::Result<AmbientAgentTaskId, anyhow::Error>;

    /// Updates a run's server-side record. Every argument is independently optional; omitted
    /// fields are left untouched rather than cleared.
    ///
    /// `session_debug_until` is the deadline of an open post-failure debug window. It is
    /// deliberately separate from `status_message` so a refresh can move the deadline without
    /// rewriting the failure text the run reported.
    async fn update_agent_task(
        &self,
        task_id: AmbientAgentTaskId,
        task_state: Option<AgentTaskState>,
        session_id: Option<session_sharing_protocol::common::SessionId>,
        conversation_id: Option<String>,
        status_message: Option<TaskStatusUpdate>,
        session_debug_until: Option<DateTime<Utc>>,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn spawn_agent(
        &self,
        request: SpawnAgentRequest,
    ) -> anyhow::Result<SpawnAgentResponse, anyhow::Error>;

    /// Materialize a server-side fork of a conversation.
    async fn fork_conversation(
        &self,
        conversation_id: String,
        title: Option<String>,
    ) -> anyhow::Result<ForkConversationResponse, anyhow::Error>;

    /// Rename a server-side conversation and return the normalized title.
    async fn rename_conversation(
        &self,
        conversation_id: String,
        title: String,
    ) -> anyhow::Result<RenameConversationResponse, anyhow::Error>;

    async fn list_ambient_agent_tasks(
        &self,
        limit: i32,
        filter: TaskListFilter,
    ) -> anyhow::Result<Vec<AmbientAgentTask>, anyhow::Error>;

    /// List agent runs and return the raw server JSON response.
    async fn list_agent_runs_raw(
        &self,
        limit: i32,
        filter: TaskListFilter,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error>;

    async fn get_ambient_agent_task(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<AmbientAgentTask, anyhow::Error>;

    /// Fetch a single agent run and return the raw server JSON response.
    async fn get_agent_run_raw(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error>;

    #[cfg(not(target_family = "wasm"))]
    async fn download_run_transcript_to_path(
        &self,
        run_id: &AmbientAgentTaskId,
        destination: &Path,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn submit_run_followup(
        &self,
        run_id: &AmbientAgentTaskId,
        request: RunFollowupRequest,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn get_ai_conversation(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<(ConversationData, ServerAIConversationMetadata), anyhow::Error>;

    async fn list_ai_conversation_metadata(
        &self,
        conversation_ids: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<ServerAIConversationMetadata>>;

    async fn cancel_ambient_agent_task(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn get_task_git_credentials(
        &self,
        task_id: String,
        workload_token: String,
    ) -> anyhow::Result<Vec<GitCredential>, anyhow::Error>;

    async fn create_file_artifact_upload_target(
        &self,
        request: CreateFileArtifactUploadRequest,
    ) -> anyhow::Result<CreateFileArtifactUploadResponse, anyhow::Error>;

    async fn confirm_file_artifact_upload(
        &self,
        artifact_uid: String,
        checksum: String,
    ) -> anyhow::Result<FileArtifactRecord, anyhow::Error>;

    async fn get_artifact_download(
        &self,
        artifact_uid: &str,
    ) -> anyhow::Result<ArtifactDownloadResponse, anyhow::Error>;

    // --- Orchestrations V2 messaging ---

    async fn send_agent_message(
        &self,
        request: SendAgentMessageRequest,
    ) -> anyhow::Result<SendAgentMessageResponse, anyhow::Error>;

    async fn list_agent_messages(
        &self,
        run_id: &str,
        request: ListAgentMessagesRequest,
    ) -> anyhow::Result<Vec<AgentMessageHeader>, anyhow::Error>;

    async fn mark_message_delivered(&self, message_id: &str) -> anyhow::Result<(), anyhow::Error>;

    async fn read_agent_message(
        &self,
        message_id: &str,
    ) -> anyhow::Result<ReadAgentMessageResponse, anyhow::Error>;

    /// Fetch a normalized conversation by conversation ID.
    async fn get_public_conversation(
        &self,
        conversation_id: &str,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error>;

    /// Fetch a normalized conversation by run ID.
    async fn get_run_conversation(
        &self,
        run_id: &str,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error>;

    /// Generates AI copy for code-review flows: commit messages at dialog-open
    /// time and PR titles / bodies at confirm time. `output_type` in the
    /// request picks which of the three the server returns.
    async fn generate_code_review_content(
        &self,
        request: GenerateCodeReviewContentRequest,
    ) -> Result<GenerateCodeReviewContentResponse, anyhow::Error>;
}

impl ServerApi {
    pub(crate) async fn send_agent_message_for_task(
        &self,
        task_id: &AmbientAgentTaskId,
        request: SendAgentMessageRequest,
    ) -> anyhow::Result<SendAgentMessageResponse, anyhow::Error> {
        let response = self
            .post_public_api_response_for_task(task_id, "agent/messages", &request)
            .await?;
        let response = response.json::<SendAgentMessageResponse>().await?;
        Ok(response)
    }

    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub(crate) async fn list_agent_messages_for_task(
        &self,
        task_id: &AmbientAgentTaskId,
        run_id: &str,
        request: ListAgentMessagesRequest,
    ) -> anyhow::Result<Vec<AgentMessageHeader>, anyhow::Error> {
        let mut params = vec![format!("limit={}", request.limit)];
        if request.unread_only {
            params.push("unread=true".to_string());
        }
        if let Some(since) = request.since {
            params.push(format!("since={}", urlencoding::encode(&since)));
        }

        let path = format!("agent/messages/{run_id}?{}", params.join("&"));
        let response = self
            .get_public_api_response_for_task(task_id, &path)
            .await?;
        let response = response.json::<Vec<AgentMessageHeader>>().await?;
        Ok(response)
    }

    pub(crate) async fn mark_message_delivered_for_task(
        &self,
        task_id: &AmbientAgentTaskId,
        message_id: &str,
    ) -> anyhow::Result<(), anyhow::Error> {
        self.post_public_api_response_for_task(
            task_id,
            &format!("agent/messages/{message_id}/delivered"),
            &(),
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn read_agent_message_for_task(
        &self,
        task_id: &AmbientAgentTaskId,
        message_id: &str,
    ) -> anyhow::Result<ReadAgentMessageResponse, anyhow::Error> {
        let response = self
            .post_public_api_response_for_task(
                task_id,
                &format!("agent/messages/{message_id}/read"),
                &(),
            )
            .await?;
        let response = response.json::<ReadAgentMessageResponse>().await?;
        Ok(response)
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl AIClient for ServerApi {
    async fn generate_commands_from_natural_language(
        &self,
        _prompt: String,
        // TODO: use relevant context from RequestContext and deprecate usage of ai_execution_context
        _ai_execution_context: Option<WarpAiExecutionContext>,
    ) -> Result<Vec<AIGeneratedCommand>, GenerateCommandsFromNaturalLanguageError> {
        Err(GenerateCommandsFromNaturalLanguageError::Other)
    }

    async fn generate_dialogue_answer(
        &self,
        _transcript: Vec<TranscriptPart>,
        _prompt: String,
        // TODO: use relevant context from RequestContext and deprecate usage of ai_execution_context
        _ai_execution_context: Option<WarpAiExecutionContext>,
    ) -> anyhow::Result<GenerateDialogueResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn generate_metadata_for_command(
        &self,
        _command: String,
    ) -> Result<GeneratedCommandMetadata, GeneratedCommandMetadataError> {
        Err(GeneratedCommandMetadataError::Other)
    }

    #[cfg(feature = "agent_mode_evals")]
    async fn get_request_limit_info(&self) -> Result<RequestUsageInfo, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    #[cfg(not(feature = "agent_mode_evals"))]
    async fn get_request_limit_info(&self) -> Result<RequestUsageInfo, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_available_harnesses(&self) -> Result<Vec<HarnessAvailability>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn provide_negative_feedback_response_for_ai_conversation(
        &self,
        _conversation_id: String,
        _request_ids: Vec<String>,
    ) -> anyhow::Result<i32, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    #[tracing::instrument(skip_all, err, fields(
        tags.cloud_agent = true,
        config.worker_host = tracing::field::Empty,
        config.harness = tracing::field::Empty
    ))]
    async fn create_agent_task(
        &self,
        _prompt: String,
        _environment_uid: Option<String>,
        _parent_run_id: Option<String>,
        _config: Option<AgentConfigSnapshot>,
    ) -> anyhow::Result<AmbientAgentTaskId, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true))]
    async fn update_agent_task(
        &self,
        _task_id: AmbientAgentTaskId,
        _task_state: Option<AgentTaskState>,
        _session_id: Option<session_sharing_protocol::common::SessionId>,
        _conversation_id: Option<String>,
        _status_message: Option<TaskStatusUpdate>,
        _session_debug_until: Option<DateTime<Utc>>,
    ) -> anyhow::Result<(), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn spawn_agent(
        &self,
        _request: SpawnAgentRequest,
    ) -> anyhow::Result<SpawnAgentResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_connected_self_hosted_workers(
        &self,
    ) -> anyhow::Result<ListConnectedSelfHostedWorkersResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn fork_conversation(
        &self,
        _conversation_id: String,
        _title: Option<String>,
    ) -> anyhow::Result<ForkConversationResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn rename_conversation(
        &self,
        _conversation_id: String,
        _title: String,
    ) -> anyhow::Result<RenameConversationResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_ambient_agent_tasks(
        &self,
        _limit: i32,
        _filter: TaskListFilter,
    ) -> anyhow::Result<Vec<AmbientAgentTask>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_agent_runs_raw(
        &self,
        _limit: i32,
        _filter: TaskListFilter,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true))]
    async fn get_ambient_agent_task(
        &self,
        _task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<AmbientAgentTask, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_agent_run_raw(
        &self,
        _task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn submit_run_followup(
        &self,
        _run_id: &AmbientAgentTaskId,
        _request: RunFollowupRequest,
    ) -> anyhow::Result<(), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true))]
    async fn get_ai_conversation(
        &self,
        _server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<(ConversationData, ServerAIConversationMetadata), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_ai_conversation_metadata(
        &self,
        _conversation_ids: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<ServerAIConversationMetadata>> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn cancel_ambient_agent_task(
        &self,
        _task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<(), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_task_git_credentials(
        &self,
        _task_id: String,
        _workload_token: String,
    ) -> anyhow::Result<Vec<GitCredential>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn create_file_artifact_upload_target(
        &self,
        _request: CreateFileArtifactUploadRequest,
    ) -> anyhow::Result<CreateFileArtifactUploadResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn confirm_file_artifact_upload(
        &self,
        _artifact_uid: String,
        _checksum: String,
    ) -> anyhow::Result<FileArtifactRecord, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_artifact_download(
        &self,
        _artifact_uid: &str,
    ) -> anyhow::Result<ArtifactDownloadResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    #[cfg(not(target_family = "wasm"))]
    async fn download_run_transcript_to_path(
        &self,
        _run_id: &AmbientAgentTaskId,
        _destination: &Path,
    ) -> anyhow::Result<(), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    // --- Orchestrations V2 messaging ---

    async fn send_agent_message(
        &self,
        _request: SendAgentMessageRequest,
    ) -> anyhow::Result<SendAgentMessageResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_agent_messages(
        &self,
        _run_id: &str,
        _request: ListAgentMessagesRequest,
    ) -> anyhow::Result<Vec<AgentMessageHeader>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn mark_message_delivered(&self, _message_id: &str) -> anyhow::Result<(), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn read_agent_message(
        &self,
        _message_id: &str,
    ) -> anyhow::Result<ReadAgentMessageResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_public_conversation(
        &self,
        _conversation_id: &str,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_run_conversation(
        &self,
        _run_id: &str,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn generate_code_review_content(
        &self,
        _request: GenerateCodeReviewContentRequest,
    ) -> Result<GenerateCodeReviewContentResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }
}

// Conversions for AIConversationMetadata from GraphQL types

fn convert_harness(harness: warp_graphql::ai::AgentHarness) -> AIAgentHarness {
    match harness {
        warp_graphql::ai::AgentHarness::Oz => AIAgentHarness::Oz,
        warp_graphql::ai::AgentHarness::ClaudeCode => AIAgentHarness::ClaudeCode,
        warp_graphql::ai::AgentHarness::Gemini => AIAgentHarness::Gemini,
        warp_graphql::ai::AgentHarness::Codex => AIAgentHarness::Codex,
        warp_graphql::ai::AgentHarness::Other(value) => {
            report_error!(
                "Invalid AgentHarness; update client GraphQL types",
                extra: { "harness" => %value },
                warp_errors::ReportErrorLogMode::OncePerRun
            );
            AIAgentHarness::Unknown
        }
    }
}

impl TryFrom<warp_graphql::ai::AIConversation> for ServerAIConversationMetadata {
    type Error = anyhow::Error;

    fn try_from(value: warp_graphql::ai::AIConversation) -> Result<Self, Self::Error> {
        // Full conversion including per-model token usage and tool usage
        // stats, so restored conversations render the same usage details
        // (e.g. the credits-expansion "Models" rows) as live ones.
        let usage: ConversationUsageMetadata = (&value.usage.usage_metadata).into();
        let metadata = value.metadata.try_into()?;
        let permissions = value.permissions.try_into()?;
        let ambient_agent_task_id = value
            .ambient_agent_task_id
            .map(|id| id.into_inner().parse())
            .transpose()?;
        let server_conversation_token =
            ServerConversationToken::new(value.conversation_id.into_inner());

        // If we fail to parse any artifacts, don't fail the entire conversion -- just don't include them in the list
        let artifacts = value
            .artifacts
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| Artifact::try_from(a).ok())
            .collect();

        Ok(Self {
            title: value.title,
            working_directory: value.working_directory,
            harness: convert_harness(value.harness),
            usage,
            metadata,
            creator: value.creator.map(Into::into),
            permissions,
            ambient_agent_task_id,
            server_conversation_token,
            artifacts,
        })
    }
}

impl TryFrom<warp_graphql::queries::list_ai_conversations::AIConversationMetadata>
    for ServerAIConversationMetadata
{
    type Error = anyhow::Error;

    fn try_from(
        value: warp_graphql::queries::list_ai_conversations::AIConversationMetadata,
    ) -> Result<Self, Self::Error> {
        let usage: ConversationUsageMetadata = (&value.usage.usage_metadata).into();
        let metadata = value.metadata.try_into()?;
        let permissions = value.permissions.try_into()?;
        let ambient_agent_task_id = value
            .ambient_agent_task_id
            .map(|id| id.into_inner().parse())
            .transpose()?;
        let server_conversation_token =
            ServerConversationToken::new(value.conversation_id.into_inner());

        let artifacts = value
            .artifacts
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| Artifact::try_from(a).ok())
            .collect();

        Ok(Self {
            title: value.title,
            working_directory: value.working_directory,
            harness: convert_harness(value.harness),
            usage,
            metadata,
            creator: value.creator.map(Into::into),
            permissions,
            ambient_agent_task_id,
            server_conversation_token,
            artifacts,
        })
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl StoreClient for ServerApi {
    async fn update_intermediate_nodes(
        &self,
        _embedding_config: EmbeddingConfig,
        _nodes: Vec<IntermediateNode>,
    ) -> Result<HashMap<NodeHash, bool>, full_source_code_embedding::Error> {
        Err(full_source_code_embedding::Error::Other(
            crate::server::server_api::local_only_error(),
        ))
    }

    async fn generate_embeddings(
        &self,
        _embedding_config: EmbeddingConfig,
        _fragments: Vec<full_source_code_embedding::Fragment>,
        _root_hash: NodeHash,
        _repo_metadata: RepoMetadata,
    ) -> Result<HashMap<ContentHash, bool>, full_source_code_embedding::Error> {
        Err(full_source_code_embedding::Error::Other(
            crate::server::server_api::local_only_error(),
        ))
    }

    async fn populate_merkle_tree_cache(
        &self,
        _embedding_config: EmbeddingConfig,
        _root_hash: NodeHash,
        _repo_metadata: RepoMetadata,
    ) -> Result<bool, full_source_code_embedding::Error> {
        Err(full_source_code_embedding::Error::Other(
            crate::server::server_api::local_only_error(),
        ))
    }

    async fn sync_merkle_tree(
        &self,
        _nodes: Vec<NodeHash>,
        _embedding_config: EmbeddingConfig,
    ) -> Result<HashSet<NodeHash>, full_source_code_embedding::Error> {
        Err(full_source_code_embedding::Error::Other(
            crate::server::server_api::local_only_error(),
        ))
    }

    async fn rerank_fragments(
        &self,
        _query: String,
        _fragments: Vec<full_source_code_embedding::Fragment>,
    ) -> Result<Vec<full_source_code_embedding::Fragment>, full_source_code_embedding::Error> {
        Err(full_source_code_embedding::Error::Other(
            crate::server::server_api::local_only_error(),
        ))
    }

    async fn get_relevant_fragments(
        &self,
        _embedding_config: EmbeddingConfig,
        _query: String,
        _root_hash: NodeHash,
        _repo_metadata: RepoMetadata,
    ) -> Result<Vec<ContentHash>, full_source_code_embedding::Error> {
        Err(full_source_code_embedding::Error::Other(
            crate::server::server_api::local_only_error(),
        ))
    }

    async fn codebase_context_config(
        &self,
    ) -> Result<CodebaseContextConfig, full_source_code_embedding::Error> {
        Err(full_source_code_embedding::Error::Other(
            crate::server::server_api::local_only_error(),
        ))
    }
}

#[cfg(test)]
#[path = "ai_tests.rs"]
mod tests;
