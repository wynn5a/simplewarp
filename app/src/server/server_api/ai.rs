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
use warp_graphql::queries::get_conversation_usage::ConversationUsage;
use warp_graphql::queries::get_scheduled_agent_history::ScheduledAgentHistory;
use warp_multi_agent_api::ConversationData;

use super::ServerApi;
use super::harness_support::{UploadField, UploadTarget};
use crate::ai::RequestUsageInfo;
pub use crate::ai::agent::UserQueryMode;
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::{
    AIAgentConversationFormat, AIAgentHarness, ServerAIConversationMetadata,
};
use crate::ai::ambient_agents::AmbientAgentTaskId;
// Re-export ambient agent types for backwards compatibility
pub use crate::ai::ambient_agents::{
    AgentConfigSnapshot, AgentSource, AmbientAgentTask, AmbientAgentTaskState, ExecutionLocation,
    TaskStatusMessage,
    task::{AttachmentInput, TaskAttachment},
};
use crate::ai::artifacts::Artifact;
use crate::ai::generate_code_review_content::api::{
    GenerateCodeReviewContentRequest, GenerateCodeReviewContentResponse,
};
use crate::ai::harness_availability::HarnessAvailability;
use crate::ai::llms::{
    AvailableLLMs, DisableReason, LLMContextWindow, LLMInfo, LLMModelHost, LLMSpec,
    LLMUsageMetadata, ModelsByFeature, RoutingHostConfig,
};
#[cfg(feature = "agent_mode_evals")]
use crate::ai::request_usage_model::RequestLimitInfo;
use crate::ai_assistant::execution_context::WarpAiExecutionContext;
use crate::ai_assistant::requests::GenerateDialogueResult;
use crate::ai_assistant::utils::TranscriptPart;
use crate::ai_assistant::{AIGeneratedCommand, GenerateCommandsFromNaturalLanguageError};
use crate::drive::workflows::ai_assist::{GeneratedCommandMetadata, GeneratedCommandMetadataError};
use crate::persistence::model::ConversationUsageMetadata;
use crate::terminal::model::block::SerializedBlock;

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
    /// References a batch of files previously uploaded to handoff/{token}/
    /// via `POST /agent/handoff/upload-snapshot`. The server stores the token on the new run's
    /// queued execution input and resolves the prefix in place at rehydration time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_snapshot_token: Option<InitialSnapshotToken>,
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

/// Server-minted token returned by `POST /agent/handoff/upload-snapshot` that scopes a batch
/// of presigned upload URLs to `handoff/{token}/`. The client passes it
/// back via `SpawnAgentRequest.initial_snapshot_token`; the server stores it on the new run's
/// queued execution input so rehydration discovery can read the same prefix.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InitialSnapshotToken(String);

impl InitialSnapshotToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Request body for `POST /agent/handoff/upload-snapshot`. Used by the local-to-cloud
/// handoff flow to allocate a token and presigned upload URLs scoped to
/// `handoff/{token}/` before any task exists.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UploadLocalHandoffSnapshotRequest {
    pub files: Vec<SnapshotUploadFileInfo>,
}

/// Describes a single file the client wants to upload as part of a handoff snapshot.
/// Wire-compatible with the server's `SnapshotUploadFileInfo` schema (also used by the
/// existing harness-side `/harness-support/upload-snapshot` endpoint).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SnapshotUploadFileInfo {
    pub filename: String,
    pub mime_type: String,
}

/// Response body for `POST /agent/handoff/upload-snapshot`. The `uploads` array is aligned
/// by index with the request `files` array; the client matches each `UploadTarget` back
/// to the requested filename by index.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct UploadLocalHandoffSnapshotResponse {
    pub initial_snapshot_token: InitialSnapshotToken,
    pub expires_at: String,
    pub uploads: Vec<UploadTarget>,
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReportAgentEventRequest {
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReportAgentEventResponse {
    pub sequence: i64,
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentRunClientEventRequest {
    pub event_uuid: String,
    pub event_name: String,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<AgentRunClientEventPayload>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum AgentRunClientEventPayload {
    SetupMetric(AgentRunClientSetupMetricPayload),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentRunClientSetupMetricPayload {
    pub start_ts: DateTime<Utc>,
    pub finish_ts: DateTime<Utc>,
    pub latency_ms: i64,
    pub is_error: bool,
}

impl AgentRunClientEventRequest {
    pub fn timeline_event(event_name: impl Into<String>, timestamp: DateTime<Utc>) -> Self {
        Self {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            event_name: event_name.into(),
            timestamp,
            payload: None,
        }
    }

    pub fn setup_metric_event(
        event_name: impl Into<String>,
        start_timestamp: DateTime<Utc>,
        finish_timestamp: DateTime<Utc>,
        is_error: bool,
    ) -> Self {
        Self {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            event_name: event_name.into(),
            timestamp: finish_timestamp,
            payload: Some(AgentRunClientEventPayload::SetupMetric(
                AgentRunClientSetupMetricPayload {
                    start_ts: start_timestamp,
                    finish_ts: finish_timestamp,
                    latency_ms: finish_timestamp
                        .signed_duration_since(start_timestamp)
                        .num_milliseconds()
                        .max(0),
                    is_error,
                },
            )),
        }
    }
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct AttachmentFileInfo {
    pub filename: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AttachmentDownloadInfo {
    pub attachment_id: String,
    pub download_url: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DownloadAttachmentsResponse {
    pub attachments: Vec<AttachmentDownloadInfo>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AttachmentUploadInfo {
    pub attachment_id: String,
    /// Presigned URL form of [`Self::upload_target`], kept for compatibility.
    /// It only describes a plain `PUT`, so it cannot express the presigned POST
    /// form that self-hosted S3 storage requires.
    pub upload_url: String,
    /// Absent when the server predates the upload-target contract.
    #[serde(default)]
    pub upload_target: Option<UploadTarget>,
}

impl AttachmentUploadInfo {
    /// The target to upload this attachment to, synthesizing a presigned `PUT`
    /// from [`Self::upload_url`] when the server did not send an upload target.
    pub fn resolve_upload_target(&self, content_type: &str) -> UploadTarget {
        self.upload_target.clone().unwrap_or_else(|| UploadTarget {
            url: self.upload_url.clone(),
            method: "PUT".to_string(),
            headers: HashMap::from([("Content-Type".to_string(), content_type.to_string())]),
            fields: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PrepareAttachmentUploadsResponse {
    pub attachments: Vec<AttachmentUploadInfo>,
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

/// Source information for an agent skill.
#[derive(Clone, serde::Deserialize, Debug, PartialEq)]
pub struct AgentSkillSource {
    pub owner: String,
    pub name: String,
    pub skill_path: String,
}

/// Environment information for an agent skill.
#[derive(Clone, serde::Deserialize, Debug, PartialEq)]
pub struct AgentSkillEnvironment {
    pub uid: String,
    pub name: String,
}

/// A variant of an agent skill.
#[derive(Clone, serde::Deserialize, Debug, PartialEq)]
pub struct AgentSkillVariant {
    pub id: String,
    pub description: String,
    pub base_prompt: String,
    pub source: AgentSkillSource,
    pub environments: Vec<AgentSkillEnvironment>,
}

/// An agent skill item with its variants.
#[derive(Clone, serde::Deserialize, Debug, PartialEq)]
pub struct AgentSkillItem {
    pub name: String,
    pub variants: Vec<AgentSkillVariant>,
}

/// Reference to a managed secret by name.
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
pub struct SecretRef {
    pub name: String,
}

/// JSON payload sent to `POST /agent/identities`.
#[derive(Clone, serde::Serialize, Debug, PartialEq, Eq)]
pub struct CreateAgentRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional base prompt for this agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<SecretRef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
}

/// JSON payload sent to `PUT /agent/identities/{uid}`.
///
/// Each field uses the public API's PATCH semantics: `None` omits the field
/// (leave unchanged), while `Some(String::new())` sends an empty value to clear
/// it. See `CreateAgentRequest`/`UpdateAgentRequest` in
/// `warp-server/public_api/openapi.yaml`.
#[derive(Clone, Default, serde::Serialize, Debug, PartialEq, Eq)]
pub struct UpdateAgentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Replacement prompt. `None` leaves it unchanged; `Some(String::new())`
    /// clears it via the public API's PATCH clear-via-empty semantics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Vec<SecretRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
}

/// Public API representation of a named agent identity.
#[derive(Clone, serde::Deserialize, serde::Serialize, Debug, PartialEq, Eq)]
pub struct AgentResponse {
    pub uid: String,
    pub name: String,
    pub description: Option<String>,
    /// Optional base prompt for this agent.
    #[serde(default)]
    pub prompt: Option<String>,
    pub available: bool,
    pub created_at: DateTime<Utc>,
    pub secrets: Vec<SecretRef>,
    pub skills: Vec<String>,
    pub base_model: Option<String>,
    #[serde(default)]
    pub environment_id: Option<String>,
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

/// A memory store returned by the public API.
#[derive(Clone, serde::Deserialize, serde::Serialize, Debug, PartialEq)]
pub struct MemoryStoreItem {
    pub uid: String,
    pub owner_type: String,
    pub owner_uid: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A memory in a memory store returned by the public API.
#[derive(Clone, serde::Deserialize, serde::Serialize, Debug, PartialEq)]
pub struct MemoryItem {
    pub uid: String,
    pub content: String,
    pub version_id: String,
    pub source: String,
    pub source_id: Option<String>,
    pub source_run_id: Option<String>,
    pub is_tombstoned: bool,
    pub tombstoned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, serde::Serialize, Debug, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MemorySource {
    Manual,
}

#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub struct CreateMemoryRequest {
    pub content: String,
    pub version: Option<String>,
    pub source: MemorySource,
    pub source_id: Option<String>,
    pub reason: String,
}

#[derive(Clone, serde::Deserialize, serde::Serialize, Debug, PartialEq)]
pub struct CreateMemoryResponse {
    pub memory_id: String,
    pub version_id: String,
}

#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub struct UpdateMemoryStoreRequest {
    pub description: Option<String>,
}

#[derive(Clone, serde::Deserialize, serde::Serialize, Debug, PartialEq)]
pub struct MemoryVersionItem {
    pub uid: String,
    pub version: String,
    pub content: String,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, serde::Deserialize, serde::Serialize, Debug, PartialEq)]
pub struct AgentAttachmentItem {
    pub uid: String,
    pub name: String,
    pub access: String,
    pub instructions: String,
}

#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub struct UpdateMemoryRequest {
    pub content: String,
    pub version: Option<String>,
    pub reason: String,
}

#[derive(Clone, serde::Deserialize, serde::Serialize, Debug, PartialEq)]
pub struct UpdateMemoryResponse {
    pub memory_id: String,
    pub version_id: String,
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

    /// Returns conversation usage history for the current user over the requested number of days.
    ///
    /// If `last_updated_end_timestamp` is provided, only conversations updated before that timestamp are returned.
    async fn get_conversation_usage_history(
        &self,
        days: Option<i32>,
        limit: Option<i32>,
        last_updated_end_timestamp: Option<warp_graphql::scalars::Time>,
    ) -> Result<Vec<ConversationUsage>, anyhow::Error>;

    async fn get_feature_model_choices(&self) -> Result<ModelsByFeature, anyhow::Error>;

    async fn get_available_harnesses(&self) -> Result<Vec<HarnessAvailability>, anyhow::Error>;
    async fn list_connected_self_hosted_workers(
        &self,
    ) -> Result<ListConnectedSelfHostedWorkersResponse, anyhow::Error>;

    /// Fetches the free-tier available models without requiring authentication.
    /// Used during pre-login onboarding so logged-out users see an accurate model list
    /// instead of the hard-coded `ModelsByFeature::default()` fallback.
    async fn get_free_available_models(
        &self,
        referrer: Option<String>,
    ) -> Result<ModelsByFeature, anyhow::Error>;

    async fn update_merkle_tree(
        &self,
        embedding_config: EmbeddingConfig,
        nodes: Vec<IntermediateNode>,
    ) -> anyhow::Result<HashMap<NodeHash, bool>>;

    async fn generate_code_embeddings(
        &self,
        embedding_config: EmbeddingConfig,
        fragments: Vec<full_source_code_embedding::Fragment>,
        root_hash: NodeHash,
        repo_metadata: RepoMetadata,
    ) -> anyhow::Result<HashMap<ContentHash, bool>>;

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

    /// Allocate an initial snapshot token and presigned upload URLs for staging local-to-cloud
    /// handoff snapshot files before the corresponding cloud task exists.
    async fn upload_local_handoff_snapshot(
        &self,
        request: UploadLocalHandoffSnapshotRequest,
    ) -> anyhow::Result<UploadLocalHandoffSnapshotResponse, anyhow::Error>;

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

    async fn get_scheduled_agent_history(
        &self,
        schedule_id: &str,
    ) -> anyhow::Result<ScheduledAgentHistory, anyhow::Error>;

    async fn get_ai_conversation(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<(ConversationData, ServerAIConversationMetadata), anyhow::Error>;

    async fn list_ai_conversation_metadata(
        &self,
        conversation_ids: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<ServerAIConversationMetadata>>;

    async fn get_ai_conversation_format(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<AIAgentConversationFormat, anyhow::Error>;

    async fn get_block_snapshot(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<SerializedBlock, anyhow::Error>;

    async fn delete_ai_conversation(
        &self,
        server_conversation_token: String,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn list_skills(
        &self,
        repo: Option<String>,
    ) -> anyhow::Result<Vec<AgentSkillItem>, anyhow::Error>;

    async fn list_agents(&self) -> anyhow::Result<Vec<AgentResponse>, anyhow::Error>;

    async fn list_agents_raw(&self) -> anyhow::Result<serde_json::Value, anyhow::Error>;

    async fn get_agent(&self, uid: &str) -> anyhow::Result<AgentResponse, anyhow::Error>;

    async fn get_agent_raw(&self, uid: &str) -> anyhow::Result<serde_json::Value, anyhow::Error>;

    async fn create_agent(
        &self,
        request: CreateAgentRequest,
    ) -> anyhow::Result<AgentResponse, anyhow::Error>;

    async fn create_agent_raw(
        &self,
        request: CreateAgentRequest,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error>;

    async fn update_agent(
        &self,
        uid: &str,
        request: UpdateAgentRequest,
    ) -> anyhow::Result<AgentResponse, anyhow::Error>;

    async fn update_agent_raw(
        &self,
        uid: &str,
        request: UpdateAgentRequest,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error>;

    async fn delete_agent(&self, uid: &str) -> anyhow::Result<(), anyhow::Error>;

    async fn list_memory_stores(&self) -> anyhow::Result<Vec<MemoryStoreItem>, anyhow::Error>;

    async fn list_memory_store_memories(
        &self,
        store_uid: &str,
    ) -> anyhow::Result<Vec<MemoryItem>, anyhow::Error>;

    async fn create_memory_store_memory(
        &self,
        store_uid: &str,
        request: CreateMemoryRequest,
    ) -> anyhow::Result<CreateMemoryResponse, anyhow::Error>;

    async fn update_memory_store_memory(
        &self,
        store_uid: &str,
        memory_uid: &str,
        request: UpdateMemoryRequest,
    ) -> anyhow::Result<UpdateMemoryResponse, anyhow::Error>;

    async fn delete_memory_store_memory(
        &self,
        store_uid: &str,
        memory_uid: &str,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn get_memory_store(
        &self,
        store_uid: &str,
    ) -> anyhow::Result<MemoryStoreItem, anyhow::Error>;

    async fn update_memory_store(
        &self,
        store_uid: &str,
        request: UpdateMemoryStoreRequest,
    ) -> anyhow::Result<MemoryStoreItem, anyhow::Error>;

    async fn list_memory_store_agents(
        &self,
        store_uid: &str,
    ) -> anyhow::Result<Vec<AgentAttachmentItem>, anyhow::Error>;

    async fn list_memory_versions(
        &self,
        store_uid: &str,
        memory_uid: &str,
    ) -> anyhow::Result<Vec<MemoryVersionItem>, anyhow::Error>;

    async fn cancel_ambient_agent_task(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn get_task_git_credentials(
        &self,
        task_id: String,
        workload_token: String,
    ) -> anyhow::Result<Vec<GitCredential>, anyhow::Error>;

    async fn get_task_attachments(
        &self,
        task_id: String,
    ) -> anyhow::Result<Vec<TaskAttachment>, anyhow::Error>;

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

    async fn prepare_attachments_for_upload(
        &self,
        task_id: &AmbientAgentTaskId,
        files: &[AttachmentFileInfo],
    ) -> anyhow::Result<PrepareAttachmentUploadsResponse, anyhow::Error>;

    async fn download_task_attachments(
        &self,
        task_id: &AmbientAgentTaskId,
        attachment_ids: &[String],
    ) -> anyhow::Result<DownloadAttachmentsResponse, anyhow::Error>;

    async fn get_handoff_snapshot_attachments(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<Vec<TaskAttachment>, anyhow::Error>;

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

    /// Persists the latest observed event sequence number for a run on the
    /// server. Used to keep the server-side cursor in sync with the client so
    /// that driver/cloud restores can resume without replaying events the
    /// parent has already acted on.
    async fn update_event_sequence_on_server(
        &self,
        run_id: &str,
        sequence: i64,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn report_agent_event(
        &self,
        run_id: &str,
        request: ReportAgentEventRequest,
    ) -> anyhow::Result<ReportAgentEventResponse, anyhow::Error>;
    async fn post_agent_run_client_event(
        &self,
        run_id: &AmbientAgentTaskId,
        request: AgentRunClientEventRequest,
    ) -> anyhow::Result<(), anyhow::Error>;

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

    async fn get_conversation_usage_history(
        &self,
        _days: Option<i32>,
        _limit: Option<i32>,
        _last_updated_end_timestamp: Option<warp_graphql::scalars::Time>,
    ) -> Result<Vec<ConversationUsage>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_feature_model_choices(&self) -> Result<ModelsByFeature, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_available_harnesses(&self) -> Result<Vec<HarnessAvailability>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_free_available_models(
        &self,
        _referrer: Option<String>,
    ) -> Result<ModelsByFeature, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_merkle_tree(
        &self,
        _embedding_config: EmbeddingConfig,
        _nodes: Vec<IntermediateNode>,
    ) -> anyhow::Result<HashMap<NodeHash, bool>> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn generate_code_embeddings(
        &self,
        _embedding_config: EmbeddingConfig,
        _fragments: Vec<full_source_code_embedding::Fragment>,
        _root_hash: NodeHash,
        _repo_metadata: RepoMetadata,
    ) -> anyhow::Result<HashMap<ContentHash, bool>> {
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

    async fn upload_local_handoff_snapshot(
        &self,
        _request: UploadLocalHandoffSnapshotRequest,
    ) -> anyhow::Result<UploadLocalHandoffSnapshotResponse, anyhow::Error> {
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

    async fn get_scheduled_agent_history(
        &self,
        _schedule_id: &str,
    ) -> anyhow::Result<ScheduledAgentHistory, anyhow::Error> {
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

    async fn get_ai_conversation_format(
        &self,
        _server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<AIAgentConversationFormat, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_block_snapshot(
        &self,
        _server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<SerializedBlock, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn delete_ai_conversation(
        &self,
        _server_conversation_token: String,
    ) -> anyhow::Result<(), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_skills(
        &self,
        _repo: Option<String>,
    ) -> anyhow::Result<Vec<AgentSkillItem>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }
    async fn list_memory_stores(&self) -> anyhow::Result<Vec<MemoryStoreItem>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_memory_store_memories(
        &self,
        _store_uid: &str,
    ) -> anyhow::Result<Vec<MemoryItem>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn create_memory_store_memory(
        &self,
        _store_uid: &str,
        _request: CreateMemoryRequest,
    ) -> anyhow::Result<CreateMemoryResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_memory_store_memory(
        &self,
        _store_uid: &str,
        _memory_uid: &str,
        _request: UpdateMemoryRequest,
    ) -> anyhow::Result<UpdateMemoryResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn delete_memory_store_memory(
        &self,
        _store_uid: &str,
        _memory_uid: &str,
    ) -> anyhow::Result<(), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_memory_store(
        &self,
        _store_uid: &str,
    ) -> anyhow::Result<MemoryStoreItem, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_memory_store(
        &self,
        _store_uid: &str,
        _request: UpdateMemoryStoreRequest,
    ) -> anyhow::Result<MemoryStoreItem, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_memory_store_agents(
        &self,
        _store_uid: &str,
    ) -> anyhow::Result<Vec<AgentAttachmentItem>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_memory_versions(
        &self,
        _store_uid: &str,
        _memory_uid: &str,
    ) -> anyhow::Result<Vec<MemoryVersionItem>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_agents(&self) -> anyhow::Result<Vec<AgentResponse>, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn list_agents_raw(&self) -> anyhow::Result<serde_json::Value, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_agent(&self, _uid: &str) -> anyhow::Result<AgentResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn get_agent_raw(&self, _uid: &str) -> anyhow::Result<serde_json::Value, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn create_agent(
        &self,
        _request: CreateAgentRequest,
    ) -> anyhow::Result<AgentResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn create_agent_raw(
        &self,
        _request: CreateAgentRequest,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_agent(
        &self,
        _uid: &str,
        _request: UpdateAgentRequest,
    ) -> anyhow::Result<AgentResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_agent_raw(
        &self,
        _uid: &str,
        _request: UpdateAgentRequest,
    ) -> anyhow::Result<serde_json::Value, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn delete_agent(&self, _uid: &str) -> anyhow::Result<(), anyhow::Error> {
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

    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true))]
    async fn get_task_attachments(
        &self,
        _task_id: String,
    ) -> anyhow::Result<Vec<TaskAttachment>, anyhow::Error> {
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

    async fn prepare_attachments_for_upload(
        &self,
        _task_id: &AmbientAgentTaskId,
        _files: &[AttachmentFileInfo],
    ) -> anyhow::Result<PrepareAttachmentUploadsResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn download_task_attachments(
        &self,
        _task_id: &AmbientAgentTaskId,
        _attachment_ids: &[String],
    ) -> anyhow::Result<DownloadAttachmentsResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true))]
    async fn get_handoff_snapshot_attachments(
        &self,
        _task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<Vec<TaskAttachment>, anyhow::Error> {
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

    async fn update_event_sequence_on_server(
        &self,
        _run_id: &str,
        _sequence: i64,
    ) -> anyhow::Result<(), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn report_agent_event(
        &self,
        _run_id: &str,
        _request: ReportAgentEventRequest,
    ) -> anyhow::Result<ReportAgentEventResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }
    async fn post_agent_run_client_event(
        &self,
        _run_id: &AmbientAgentTaskId,
        _request: AgentRunClientEventRequest,
    ) -> anyhow::Result<(), anyhow::Error> {
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

impl TryFrom<warp_graphql::queries::get_feature_model_choices::FeatureModelChoice>
    for ModelsByFeature
{
    type Error = anyhow::Error;

    fn try_from(
        value: warp_graphql::queries::get_feature_model_choices::FeatureModelChoice,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            agent_mode: value.agent_mode.try_into()?,
            coding: value.coding.try_into()?,
            cli_agent: Some(value.cli_agent.try_into()?),
            computer_use: Some(value.computer_use_agent.try_into()?),
        })
    }
}

impl TryFrom<warp_graphql::workspace::FeatureModelChoice> for ModelsByFeature {
    type Error = anyhow::Error;

    fn try_from(value: warp_graphql::workspace::FeatureModelChoice) -> Result<Self, Self::Error> {
        Ok(Self {
            agent_mode: value.agent_mode.try_into()?,
            coding: value.coding.try_into()?,
            cli_agent: Some(value.cli_agent.try_into()?),
            computer_use: Some(value.computer_use_agent.try_into()?),
        })
    }
}

impl TryFrom<warp_graphql::queries::get_feature_model_choices::AvailableLlms> for AvailableLLMs {
    type Error = anyhow::Error;

    fn try_from(
        value: warp_graphql::queries::get_feature_model_choices::AvailableLlms,
    ) -> Result<Self, Self::Error> {
        Self::new(
            value.default_id.into(),
            value.choices.into_iter().map(LLMInfo::from),
            value.preferred_codex_model_id.map(Into::into),
        )
    }
}

impl TryFrom<warp_graphql::workspace::AvailableLlms> for AvailableLLMs {
    type Error = anyhow::Error;

    fn try_from(value: warp_graphql::workspace::AvailableLlms) -> Result<Self, Self::Error> {
        Self::new(
            value.default_id.into(),
            value.choices.into_iter().map(LLMInfo::from),
            value.preferred_codex_model_id.map(Into::into),
        )
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::LlmInfo> for LLMInfo {
    fn from(value: warp_graphql::queries::get_feature_model_choices::LlmInfo) -> Self {
        let host_configs = {
            let mut map = std::collections::HashMap::new();
            for config in value.host_configs {
                let config: RoutingHostConfig = config.into();
                let host = config.model_routing_host.clone();
                if map.insert(host.clone(), config).is_some() {
                    log::warn!(
                        "Duplicate LlmModelHost entry for {:?}, using latest value",
                        host
                    );
                }
            }
            map
        };
        Self {
            id: value.id.into(),
            display_name: value.display_name,
            base_model_name: value.base_model_name,
            reasoning_level: value.reasoning_level,
            usage_metadata: value.usage_metadata.into(),
            description: value.description,
            disable_reason: value.disable_reason.map(DisableReason::from),
            vision_supported: value.vision_supported,
            spec: value.spec.map(Into::into),
            provider: value.provider.into(),
            host_configs,
            discount_percentage: value.pricing.discount_percentage.map(|v| v as f32),
            context_window: LLMContextWindow {
                is_configurable: value.context_window.is_configurable,
                min: value.context_window.min.into(),
                max: value.context_window.max.into(),
                default_max: value.context_window.default.into(),
            },
        }
    }
}

impl From<warp_graphql::workspace::LlmInfo> for LLMInfo {
    fn from(value: warp_graphql::workspace::LlmInfo) -> Self {
        let host_configs = {
            let mut map = std::collections::HashMap::new();
            for config in value.host_configs {
                let config: RoutingHostConfig = config.into();
                let host = config.model_routing_host.clone();
                if map.insert(host.clone(), config).is_some() {
                    log::warn!(
                        "Duplicate LlmModelHost entry for {:?}, using latest value",
                        host
                    );
                }
            }
            map
        };
        Self {
            id: value.id.into(),
            display_name: value.display_name,
            base_model_name: value.base_model_name,
            reasoning_level: value.reasoning_level,
            usage_metadata: value.usage_metadata.into(),
            description: value.description,
            disable_reason: value.disable_reason.map(DisableReason::from),
            vision_supported: value.vision_supported,
            spec: value.spec.map(Into::into),
            provider: value.provider.into(),
            host_configs,
            discount_percentage: value.pricing.discount_percentage.map(|v| v as f32),
            context_window: LLMContextWindow {
                is_configurable: value.context_window.is_configurable,
                min: value.context_window.min.into(),
                max: value.context_window.max.into(),
                default_max: value.context_window.default.into(),
            },
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::RoutingHostConfig>
    for RoutingHostConfig
{
    fn from(value: warp_graphql::queries::get_feature_model_choices::RoutingHostConfig) -> Self {
        Self {
            enabled: value.enabled,
            model_routing_host: value.model_routing_host.into(),
        }
    }
}

impl From<warp_graphql::workspace::RoutingHostConfig> for RoutingHostConfig {
    fn from(value: warp_graphql::workspace::RoutingHostConfig) -> Self {
        Self {
            enabled: value.enabled,
            model_routing_host: value.model_routing_host.into(),
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::LlmModelHost> for LLMModelHost {
    fn from(value: warp_graphql::queries::get_feature_model_choices::LlmModelHost) -> Self {
        match value {
            warp_graphql::queries::get_feature_model_choices::LlmModelHost::DirectApi => {
                LLMModelHost::DirectApi
            }
            warp_graphql::queries::get_feature_model_choices::LlmModelHost::AwsBedrock => {
                LLMModelHost::AwsBedrock
            }
            warp_graphql::queries::get_feature_model_choices::LlmModelHost::CustomEndpoint => {
                LLMModelHost::CustomEndpoint
            }
            warp_graphql::queries::get_feature_model_choices::LlmModelHost::GeminiEnterprise => {
                LLMModelHost::GeminiEnterprise
            }
            warp_graphql::queries::get_feature_model_choices::LlmModelHost::Other(value) => {
                log::warn!(
                    "Unknown LlmModelHost '{value}'. Make sure to update client GraphQL types!"
                );
                LLMModelHost::Unknown
            }
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::LlmSpec> for LLMSpec {
    fn from(value: warp_graphql::queries::get_feature_model_choices::LlmSpec) -> Self {
        Self {
            cost: value.cost as f32,
            quality: value.quality as f32,
            speed: value.speed as f32,
        }
    }
}

impl From<warp_graphql::workspace::LlmSpec> for LLMSpec {
    fn from(value: warp_graphql::workspace::LlmSpec) -> Self {
        Self {
            cost: value.cost as f32,
            quality: value.quality as f32,
            speed: value.speed as f32,
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::LlmUsageMetadata> for LLMUsageMetadata {
    fn from(value: warp_graphql::queries::get_feature_model_choices::LlmUsageMetadata) -> Self {
        Self {
            request_multiplier: value.request_multiplier.max(1) as usize,
            credit_multiplier: value.credit_multiplier.map(|v| v as f32),
        }
    }
}

impl From<warp_graphql::workspace::LlmUsageMetadata> for LLMUsageMetadata {
    fn from(value: warp_graphql::workspace::LlmUsageMetadata) -> Self {
        Self {
            request_multiplier: value.request_multiplier.max(1) as usize,
            credit_multiplier: value.credit_multiplier.map(|v| v as f32),
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::DisableReason> for DisableReason {
    fn from(value: warp_graphql::queries::get_feature_model_choices::DisableReason) -> Self {
        match value {
            warp_graphql::queries::get_feature_model_choices::DisableReason::AdminDisabled => {
                DisableReason::AdminDisabled
            }
            warp_graphql::queries::get_feature_model_choices::DisableReason::OutOfRequests => {
                DisableReason::OutOfRequests
            }
            warp_graphql::queries::get_feature_model_choices::DisableReason::ProviderOutage => {
                DisableReason::ProviderOutage
            }
            warp_graphql::queries::get_feature_model_choices::DisableReason::RequiresUpgrade => {
                DisableReason::RequiresUpgrade
            }
            warp_graphql::queries::get_feature_model_choices::DisableReason::Other(_) => {
                DisableReason::Unavailable
            }
        }
    }
}

impl From<warp_graphql::workspace::DisableReason> for DisableReason {
    fn from(value: warp_graphql::workspace::DisableReason) -> Self {
        match value {
            warp_graphql::workspace::DisableReason::AdminDisabled => DisableReason::AdminDisabled,
            warp_graphql::workspace::DisableReason::OutOfRequests => DisableReason::OutOfRequests,
            warp_graphql::workspace::DisableReason::ProviderOutage => DisableReason::ProviderOutage,
            warp_graphql::workspace::DisableReason::RequiresUpgrade => {
                DisableReason::RequiresUpgrade
            }
            warp_graphql::workspace::DisableReason::Other(_) => DisableReason::Unavailable,
        }
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
