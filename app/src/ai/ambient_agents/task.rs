//! Ambient agent task types and utilities.

#[cfg(not(target_family = "wasm"))]
pub use cloud_object_models::HarnessModelConfig;
pub use cloud_object_models::{AgentConfigSnapshot, HarnessConfig};
use serde::{Deserialize, Serialize};
use warpui::{SingletonEntity, View, ViewContext};

use super::AmbientAgentTaskId;
use crate::view_components::DismissibleToast;
use crate::workspace::ToastStack;

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

/// Single attachment input captured on the client (e.g., a file upload).
#[derive(Clone, Debug, Serialize)]
pub struct AttachmentInput {
    pub file_name: String,
    pub mime_type: String,
    pub data: String,
}

/// Returns the trimmed orchestrator agent name, or `None` when empty / whitespace-only.
pub fn normalize_orchestrator_agent_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
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
