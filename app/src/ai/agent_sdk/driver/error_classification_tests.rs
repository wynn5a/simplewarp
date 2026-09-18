use warp_graphql::ai::AgentTaskState;

use super::classify_driver_error;
use crate::ai::agent::RenderableAIError;
use crate::ai::agent_sdk::driver::AgentDriverError;
use crate::ai::agent_sdk::driver::terminal::{BootstrapError, ShareSessionError};

fn assert_state(error: AgentDriverError, expected_state: AgentTaskState) {
    assert_eq!(classify_driver_error(&error), expected_state);
}

// --- Infrastructure errors → ERROR ---

#[test]
fn bootstrap_failures_are_error() {
    assert_state(
        AgentDriverError::BootstrapFailed {
            error: BootstrapError::PtySpawnFailed {
                reason: Some("pty gone".to_string()),
            },
        },
        AgentTaskState::Error,
    );
    assert_state(
        AgentDriverError::BootstrapFailed {
            error: BootstrapError::TimedOut,
        },
        AgentTaskState::Error,
    );
}

#[test]
fn terminal_unavailable_is_error() {
    assert_state(AgentDriverError::TerminalUnavailable, AgentTaskState::Error);
}

#[test]
fn not_logged_in_is_error() {
    assert_state(AgentDriverError::NotLoggedIn, AgentTaskState::Error);
}

#[test]
fn share_session_failures_are_error() {
    assert_state(
        AgentDriverError::ShareSessionFailed {
            error: ShareSessionError::Disabled,
        },
        AgentTaskState::Error,
    );
    assert_state(
        AgentDriverError::ShareSessionFailed {
            error: ShareSessionError::Timeout,
        },
        AgentTaskState::Error,
    );
}

// --- User-side errors → FAILED ---

#[test]
fn environment_and_config_failures_are_failed() {
    assert_state(
        AgentDriverError::EnvironmentSetupFailed("pip install exploded".to_string()),
        AgentTaskState::Failed,
    );
    assert_state(
        AgentDriverError::SkillResolutionFailed("missing".to_string()),
        AgentTaskState::Failed,
    );
    assert_state(
        AgentDriverError::InvalidWorkingDirectory {
            path: "/nope".into(),
            source: std::io::ErrorKind::NotFound.into(),
        },
        AgentTaskState::Failed,
    );
}

// --- Conversation errors ---

#[test]
fn conversation_cancelled_is_cancelled() {
    assert_state(
        AgentDriverError::ConversationCancelled {
            reason: crate::ai::agent::CancellationReason::ManuallyCancelled,
        },
        AgentTaskState::Cancelled,
    );
}

#[test]
fn conversation_blocked_is_blocked() {
    assert_state(
        AgentDriverError::ConversationBlocked {
            blocked_action: "run tests".to_string(),
        },
        AgentTaskState::Blocked,
    );
}

#[test]
fn renderable_error_classification_splits_user_from_internal() {
    assert_eq!(
        super::classify_renderable_error(&RenderableAIError::QuotaLimit {
            user_display_message: None,
        }),
        AgentTaskState::Failed
    );
    assert_eq!(
        super::classify_renderable_error(&RenderableAIError::ServerOverloaded),
        AgentTaskState::Error
    );
    assert_eq!(
        super::classify_renderable_error(&RenderableAIError::Other {
            error_message: "bad input".to_string(),
            will_attempt_resume: false,
            waiting_for_network: false,
            is_user_error: true,
        }),
        AgentTaskState::Failed
    );
    assert_eq!(
        super::classify_renderable_error(&RenderableAIError::Other {
            error_message: "boom".to_string(),
            will_attempt_resume: false,
            waiting_for_network: false,
            is_user_error: false,
        }),
        AgentTaskState::Error
    );
}
