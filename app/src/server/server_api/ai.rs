#[cfg(not(target_family = "wasm"))]
use std::path::Path;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
#[cfg(test)]
use mockall::automock;
use warp_graphql::ai::{AgentTaskState, PlatformErrorCode};

use super::ServerApi;
pub use crate::ai::agent::UserQueryMode;
use crate::ai::ambient_agents::AmbientAgentTaskId;
// Re-export ambient agent types for backwards compatibility
pub use crate::ai::ambient_agents::{
    AgentConfigSnapshot, AgentSource, AmbientAgentTask, AmbientAgentTaskState, ExecutionLocation,
    TaskStatusMessage, task::AttachmentInput,
};
use crate::ai::generate_code_review_content::api::{
    GenerateCodeReviewContentRequest, GenerateCodeReviewContentResponse,
};

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

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunFollowupRequest {
    pub message: String,
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

#[derive(serde::Deserialize)]
pub struct SpawnAgentResponse {
    pub task_id: AmbientAgentTaskId,
    pub run_id: String,
    #[serde(default)]
    pub at_capacity: bool,
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

#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait AIClient: 'static + Send + Sync {
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

    async fn cancel_ambient_agent_task(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn get_task_git_credentials(
        &self,
        task_id: String,
        workload_token: String,
    ) -> anyhow::Result<Vec<GitCredential>, anyhow::Error>;

    /// Generates AI copy for code-review flows: commit messages at dialog-open
    /// time and PR titles / bodies at confirm time. `output_type` in the
    /// request picks which of the three the server returns.
    async fn generate_code_review_content(
        &self,
        request: GenerateCodeReviewContentRequest,
    ) -> Result<GenerateCodeReviewContentResponse, anyhow::Error>;
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl AIClient for ServerApi {
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

    #[cfg(not(target_family = "wasm"))]
    async fn download_run_transcript_to_path(
        &self,
        _run_id: &AmbientAgentTaskId,
        _destination: &Path,
    ) -> anyhow::Result<(), anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn generate_code_review_content(
        &self,
        _request: GenerateCodeReviewContentRequest,
    ) -> Result<GenerateCodeReviewContentResponse, anyhow::Error> {
        Err(crate::server::server_api::local_only_error())
    }
}

#[cfg(test)]
#[path = "ai_tests.rs"]
mod tests;
