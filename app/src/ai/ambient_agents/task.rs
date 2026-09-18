//! Ambient agent task types and utilities.

use chrono::{DateTime, Duration as ChronoDuration, Utc};
#[cfg(not(target_family = "wasm"))]
pub use cloud_object_models::HarnessModelConfig;
pub use cloud_object_models::{AgentConfigSnapshot, HarnessConfig};
use iso8601_duration::Duration as Iso8601Duration;
use serde::{Deserialize, Serialize};
use session_sharing_protocol::common::SessionId;
use url::Url;
use warpui::{SingletonEntity, View, ViewContext};

use super::AmbientAgentTaskId;
use crate::ai::artifacts::{Artifact, deserialize_artifacts};
use crate::view_components::DismissibleToast;
use crate::workspace::ToastStack;

fn parse_session_id_from_link(session_link: &str) -> Option<SessionId> {
    Url::parse(session_link).ok().and_then(|url| {
        url.path_segments()
            .into_iter()
            .flatten()
            .last()
            .and_then(|segment| segment.parse().ok())
    })
}

fn parse_execution_session_id(execution: RunExecution<'_>) -> Option<SessionId> {
    execution
        .session_id
        .and_then(|id| id.parse().ok())
        .or_else(|| execution.session_link.and_then(parse_session_id_from_link))
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSource {
    Linear,
    AgentWebhook,
    Slack,
    Cli,
    ScheduledAgent,
    Interactive,
    WebApp,
    GitHubAction,
    GitHubWebhook,
    CloudMode,
    Orchestration,
    Jira,
    GitLabWebhook,
    RunScorer,
    Autofix,
    BenchmarkTrial,
}

impl AgentSource {
    pub fn as_str(&self) -> &str {
        match self {
            AgentSource::Linear => "LINEAR",
            AgentSource::AgentWebhook => "API",
            AgentSource::Slack => "SLACK",
            AgentSource::Cli => "CLI",
            AgentSource::ScheduledAgent => "SCHEDULED_AGENT",
            // The public API's run source for local interactive tasks is named
            // `LOCAL`.
            AgentSource::Interactive => "LOCAL",
            AgentSource::WebApp => "WEB_APP",
            AgentSource::GitHubAction => "GITHUB_ACTION",
            AgentSource::GitHubWebhook => "GITHUB_WEBHOOK",
            AgentSource::CloudMode => "CLOUD_MODE",
            AgentSource::Orchestration => "ORCHESTRATION",
            AgentSource::Jira => "JIRA",
            AgentSource::GitLabWebhook => "GITLAB_WEBHOOK",
            AgentSource::RunScorer => "RUN_SCORER",
            // The server surfaces the internal AUTOFIX task source under the public
            // name SELF_IMPROVEMENT (mirrors AgentWebhook/"API" above).
            AgentSource::Autofix => "SELF_IMPROVEMENT",
            AgentSource::BenchmarkTrial => "BENCHMARK_TRIAL",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            AgentSource::Linear => "Linear",
            AgentSource::AgentWebhook => "API",
            AgentSource::Slack => "Slack",
            AgentSource::Cli => "CLI",
            AgentSource::ScheduledAgent => "Scheduled",
            AgentSource::Interactive | AgentSource::CloudMode => "Warp App",
            AgentSource::WebApp => "Oz Web",
            AgentSource::GitHubAction => "GitHub Action",
            AgentSource::GitHubWebhook => "GitHub",
            AgentSource::Orchestration => "Orchestration",
            AgentSource::Jira => "Jira",
            AgentSource::GitLabWebhook => "GitLab",
            AgentSource::RunScorer => "Scorer",
            AgentSource::Autofix => "Self-improvement",
            AgentSource::BenchmarkTrial => "Benchmark",
        }
    }

    /// Returns true if this source represents a user-initiated conversation
    /// (as opposed to automated/programmatic sources like CLI or scheduled runs).
    pub fn is_user_initiated(&self) -> bool {
        match self {
            AgentSource::Linear
            | AgentSource::Slack
            | AgentSource::Interactive
            | AgentSource::WebApp
            | AgentSource::CloudMode
            | AgentSource::Jira => true,
            AgentSource::Cli
            | AgentSource::ScheduledAgent
            | AgentSource::AgentWebhook
            | AgentSource::GitHubAction
            | AgentSource::GitHubWebhook
            | AgentSource::Orchestration
            | AgentSource::GitLabWebhook
            | AgentSource::RunScorer
            | AgentSource::Autofix
            | AgentSource::BenchmarkTrial => false,
        }
    }
}

/// Where the server executed an agent run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ExecutionLocation {
    Local,
    Remote,
}

fn deserialize_ambient_agent_source<'de, D>(
    deserializer: D,
) -> Result<Option<AgentSource>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = serde::Deserialize::deserialize(deserializer)?;
    Ok(match s {
        Some(s) => match s.as_str() {
            "LINEAR" => Some(AgentSource::Linear),
            "AGENT_WEBHOOK" | "API" => Some(AgentSource::AgentWebhook),
            "SLACK" => Some(AgentSource::Slack),
            "LOCAL" => Some(AgentSource::Interactive),
            "CLI" => Some(AgentSource::Cli),
            "SCHEDULED_AGENT" => Some(AgentSource::ScheduledAgent),
            "WEB_APP" => Some(AgentSource::WebApp),
            "GITHUB_ACTION" => Some(AgentSource::GitHubAction),
            "GITHUB_WEBHOOK" => Some(AgentSource::GitHubWebhook),
            "CLOUD_MODE" => Some(AgentSource::CloudMode),
            "ORCHESTRATION" => Some(AgentSource::Orchestration),
            "JIRA" => Some(AgentSource::Jira),
            "GITLAB_WEBHOOK" => Some(AgentSource::GitLabWebhook),
            "RUN_SCORER" => Some(AgentSource::RunScorer),
            // The server surfaces the internal AUTOFIX task source under the public
            // name SELF_IMPROVEMENT; accept both spellings.
            "AUTOFIX" | "SELF_IMPROVEMENT" => Some(AgentSource::Autofix),
            "BENCHMARK_TRIAL" => Some(AgentSource::BenchmarkTrial),
            _ => {
                log::warn!("Unknown AmbientAgentSource: {s}");
                None
            }
        },
        None => None,
    })
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct AmbientAgentTask {
    pub task_id: AmbientAgentTaskId,
    #[serde(default)]
    pub parent_run_id: Option<String>,
    pub title: String,
    pub state: AmbientAgentTaskState,
    pub prompt: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub run_time: Option<Iso8601Duration>,
    pub status_message: Option<TaskStatusMessage>,
    #[serde(default, deserialize_with = "deserialize_ambient_agent_source")]
    pub source: Option<AgentSource>,
    #[serde(default)]
    pub execution_location: Option<ExecutionLocation>,
    pub session_id: Option<String>,
    pub session_link: Option<String>,
    pub creator: Option<TaskPrincipalInfo>,
    #[serde(default)]
    pub executor: Option<TaskPrincipalInfo>,
    pub conversation_id: Option<String>,
    pub request_usage: Option<RequestUsage>,
    pub is_sandbox_running: bool,

    /// Snapshot of the agent config used to create the task.
    #[serde(default, alias = "agent_config")]
    pub agent_config_snapshot: Option<AgentConfigSnapshot>,
    #[serde(default, deserialize_with = "deserialize_artifacts")]
    pub artifacts: Vec<Artifact>,

    /// The last event sequence number recorded for this run by the server.
    /// Used by orchestration event delivery to resume from the correct
    /// cursor on restart. Populated by `GET /agent/runs/{run_id}` when the
    /// server supports it; `None` on older servers.
    #[serde(default)]
    pub last_event_sequence: Option<i64>,

    /// The server-recorded `run_id`s of direct children of this run. Used
    /// by orchestration event-delivery restore to discover children whose
    /// records may not exist locally (e.g. remote-worker children in the
    /// driver case). Empty on older servers.
    #[serde(default)]
    pub children: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunExecution<'a> {
    pub session_id: Option<&'a str>,
    pub session_link: Option<&'a str>,
    pub request_usage: Option<&'a RequestUsage>,
    pub is_sandbox_running: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmbientAgentLiveSessionState {
    /// The task does not currently have a running execution with a joinable session signal.
    Inactive,
    /// The task has a running execution, but this client does not have a parsed
    /// shared-session id it can attach to.
    ActiveUnattachable,
    /// The task has a running execution and this client can attach to its shared session.
    Attachable { session_id: SessionId },
}

impl RunExecution<'_> {
    pub fn has_joinable_session(&self) -> bool {
        self.session_id.is_some() || self.session_link.is_some()
    }

    pub fn is_active(&self) -> bool {
        self.is_sandbox_running && self.has_joinable_session()
    }
}

/// Represents a single attachment input from the client (e.g., file upload)
#[derive(Clone, Debug, Serialize)]
pub struct AttachmentInput {
    pub file_name: String,
    pub mime_type: String,
    pub data: String, // base64-encoded data
}

/// Returns the trimmed orchestrator agent name, or `None` when empty / whitespace-only.
pub fn normalize_orchestrator_agent_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

impl AmbientAgentTask {
    /// Returns the short label for this task: trimmed `agent_config_snapshot.name`,
    /// trimmed `title`, or `"Agent"`.
    pub fn display_name(&self) -> &str {
        if let Some(name) = self
            .agent_config_snapshot
            .as_ref()
            .and_then(|c| c.name.as_deref())
        {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
        let trimmed_title = self.title.trim();
        if !trimmed_title.is_empty() {
            return trimmed_title;
        }
        "Agent"
    }

    pub fn active_run_execution(&self) -> RunExecution<'_> {
        RunExecution {
            session_id: self.session_id.as_deref(),
            session_link: self.session_link.as_deref().filter(|link| !link.is_empty()),
            request_usage: self.request_usage.as_ref(),
            is_sandbox_running: self.is_sandbox_running,
        }
    }

    /// Returns the canonical live-session state for this task from the client's perspective.
    ///
    /// This separates task liveness from attachability: an in-progress task can have an active
    /// execution without a usable shared-session id. FAILED/ERROR tasks may also remain live while
    /// their sandbox is retained for debugging. Callers should not treat either case as a completed
    /// transcript/follow-up state.
    pub fn active_live_session_state(&self) -> AmbientAgentLiveSessionState {
        let execution = self.active_run_execution();
        if !self.supports_live_session() || !execution.is_active() {
            return AmbientAgentLiveSessionState::Inactive;
        }

        match parse_execution_session_id(execution) {
            Some(session_id) => AmbientAgentLiveSessionState::Attachable { session_id },
            None => AmbientAgentLiveSessionState::ActiveUnattachable,
        }
    }

    pub fn has_active_execution(&self) -> bool {
        self.supports_live_session() && self.active_run_execution().is_active()
    }

    /// Server-reported run duration.
    pub fn run_time(&self) -> Option<ChronoDuration> {
        self.run_time.and_then(|run_time| run_time.to_chrono())
    }

    fn supports_live_session(&self) -> bool {
        matches!(
            self.state,
            AmbientAgentTaskState::InProgress
                | AmbientAgentTaskState::Failed
                | AmbientAgentTaskState::Error
        )
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum AmbientAgentTaskState {
    Queued,
    Pending,
    Claimed,
    #[serde(alias = "IN_PROGRESS")]
    InProgress,
    Succeeded,
    Failed,
    Error,
    Blocked,
    Cancelled,
    #[serde(other)]
    Unknown,
}

impl std::fmt::Display for AmbientAgentTaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AmbientAgentTaskState::Queued => write!(f, "Queued"),
            AmbientAgentTaskState::Pending => write!(f, "Pending"),
            AmbientAgentTaskState::Claimed => write!(f, "Claimed"),
            AmbientAgentTaskState::InProgress => write!(f, "In progress"),
            AmbientAgentTaskState::Succeeded => write!(f, "Done"),
            AmbientAgentTaskState::Failed => write!(f, "Failed"),
            AmbientAgentTaskState::Error => write!(f, "Error"),
            AmbientAgentTaskState::Blocked => write!(f, "Blocked"),
            AmbientAgentTaskState::Cancelled => write!(f, "Cancelled"),
            AmbientAgentTaskState::Unknown => write!(f, "Failed"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct TaskPrincipalInfo {
    #[serde(rename = "type")]
    pub creator_type: String,
    pub uid: String,
    pub display_name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct TaskStatusMessage {
    pub message: String,
    #[serde(default, alias = "errorCode")]
    pub error_code: Option<TaskStatusErrorCode>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatusErrorCode {
    #[serde(alias = "ENVIRONMENT_SETUP_FAILED")]
    EnvironmentSetupFailed,
    #[serde(other)]
    Unknown,
}

impl TaskStatusErrorCode {
    pub fn is_environment_setup_failure(&self) -> bool {
        matches!(self, TaskStatusErrorCode::EnvironmentSetupFailed)
    }
}

impl TaskStatusMessage {
    pub fn is_environment_setup_failure(&self) -> bool {
        self.error_code
            .as_ref()
            .is_some_and(TaskStatusErrorCode::is_environment_setup_failure)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct RequestUsage {
    pub inference_cost: Option<f64>,
    pub compute_cost: Option<f64>,
    pub platform_cost: Option<f64>,
}

/// Cloud task cancellation is gone: task ids in this build are local-only,
/// so there is no server task to cancel. Reports the same failure the wall
/// produced, without the spawn.
pub fn cancel_task_with_toast<V: View>(task_id: AmbientAgentTaskId, ctx: &mut ViewContext<V>) {
    let window_id = ctx.window_id();
    log::warn!(
        "Cannot cancel task {task_id}: cloud task cancellation is unavailable in this build"
    );
    ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
        let toast = DismissibleToast::default(
            "Failed to cancel task: cloud task cancellation is unavailable in this build"
                .to_string(),
        );
        toast_stack.add_ephemeral_toast(toast, window_id, ctx);
    });
}

/// Cloud task cancellation is gone; nothing to cancel server-side.
pub fn cancel_task_silently<V: View>(task_id: AmbientAgentTaskId, _ctx: &mut ViewContext<V>) {
    log::warn!(
        "Cannot cancel task {task_id}: cloud task cancellation is unavailable in this build"
    );
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
