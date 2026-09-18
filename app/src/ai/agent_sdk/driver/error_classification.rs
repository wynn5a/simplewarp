use warp_graphql::ai::AgentTaskState;

use super::AgentDriverError;
use crate::ai::agent::RenderableAIError;

/// Classify an `AgentDriverError` into the task state its report would carry.
///
/// Consumed by `ErrorExt::is_actionable` to split user-actionable failures
/// (`Failed`) from internal ones (`Error`).
pub fn classify_driver_error(error: &AgentDriverError) -> AgentTaskState {
    match error {
        // --- Warp-side errors (task → ERROR) ---
        AgentDriverError::TerminalUnavailable | AgentDriverError::InvalidRuntimeState => {
            AgentTaskState::Error
        }
        AgentDriverError::BootstrapFailed { .. } => AgentTaskState::Error,
        AgentDriverError::ShareSessionFailed { .. } => AgentTaskState::Error,
        AgentDriverError::NotLoggedIn => AgentTaskState::Error,
        // --- User-side errors (task → FAILED) ---
        AgentDriverError::MCPServerNotFound(_)
        | AgentDriverError::ManagedMcpResolutionFailed { .. }
        | AgentDriverError::MCPStartupFailed { .. }
        | AgentDriverError::MCPJsonParseError(_)
        | AgentDriverError::MCPMissingVariables
        | AgentDriverError::ProfileError(_)
        | AgentDriverError::AIWorkflowNotFound(_)
        | AgentDriverError::EnvironmentNotFound(_)
        | AgentDriverError::EnvironmentSetupFailed(_)
        | AgentDriverError::SetupCommandExitedShell { .. }
        | AgentDriverError::InvalidWorkingDirectory { .. } => AgentTaskState::Failed,
        // --- Conversation errors ---
        AgentDriverError::ConversationError { error } => classify_renderable_error(error),
        // --- Cancellation / Blocked ---
        AgentDriverError::ConversationCancelled { .. } => AgentTaskState::Cancelled,
        AgentDriverError::ConversationBlocked { .. } => AgentTaskState::Blocked,
        // --- Setup errors ---
        AgentDriverError::SkillResolutionFailed(_)
        | AgentDriverError::ConfigBuildFailed(_)
        | AgentDriverError::HarnessCommandFailed { .. }
        | AgentDriverError::HarnessSetupFailed { .. }
        | AgentDriverError::HarnessConfigSetupFailed { .. }
        | AgentDriverError::HarnessAuthCheckFailed { .. }
        | AgentDriverError::HarnessRuntimeFailureDetected { .. } => AgentTaskState::Failed,
    }
}

/// Classify a conversation-level error into the task state its report would
/// carry: `Error` for warp-side faults, `Failed` for user-actionable ones.
pub(crate) fn classify_renderable_error(error: &RenderableAIError) -> AgentTaskState {
    match error {
        RenderableAIError::QuotaLimit { .. }
        | RenderableAIError::ContextWindowExceeded(_)
        | RenderableAIError::InvalidApiKey { .. }
        | RenderableAIError::AwsBedrockCredentialsExpiredOrInvalid { .. }
        | RenderableAIError::GeminiEnterpriseCredentialsExpiredOrInvalid
        | RenderableAIError::AgentExitedShell { .. } => AgentTaskState::Failed,
        RenderableAIError::ServerOverloaded
        | RenderableAIError::InternalWarpError
        | RenderableAIError::TransientNetworkError { .. }
        | RenderableAIError::CloudStartupFailed(_) => AgentTaskState::Error,
        RenderableAIError::Other { is_user_error, .. } => {
            if *is_user_error {
                AgentTaskState::Failed
            } else {
                AgentTaskState::Error
            }
        }
    }
}

#[cfg(test)]
#[path = "error_classification_tests.rs"]
mod tests;
