use std::sync::Arc;
use std::time::Duration;

use super::{AttemptFailure, RecoveryAction, recovery_action};
use crate::server::server_api::{AIApiError, ProviderFailure};

fn before_reply(retryable: bool) -> AttemptFailure {
    AttemptFailure {
        before_reply: true,
        retryable,
        retry_after: None,
        is_recoverable: retryable,
    }
}

fn after_reply(is_recoverable: bool) -> AttemptFailure {
    AttemptFailure {
        before_reply: false,
        retryable: is_recoverable,
        retry_after: None,
        is_recoverable,
    }
}

// Argument order after the failure: may_retry, retries_done, can_attempt_resume_on_error,
// is_online.

#[test]
fn a_transient_failure_before_the_reply_waits_and_retries() {
    assert_eq!(
        recovery_action(before_reply(true), true, 1, true, true),
        RecoveryAction::RetryAfter(Duration::from_secs(4))
    );
    // Resume eligibility is irrelevant before the reply.
    assert_eq!(
        recovery_action(before_reply(true), true, 0, false, true),
        RecoveryAction::RetryAfter(Duration::from_secs(2))
    );
    let mut asked_to_wait = before_reply(true);
    asked_to_wait.retry_after = Some(Duration::from_secs(7));
    assert_eq!(
        recovery_action(asked_to_wait, true, 0, true, true),
        RecoveryAction::RetryAfter(Duration::from_secs(7))
    );
}

#[test]
fn a_failure_before_the_reply_waits_for_connectivity_when_offline() {
    assert_eq!(
        recovery_action(before_reply(true), true, 0, true, false),
        RecoveryAction::RetryWhenOnline
    );
}

#[test]
fn giving_up_before_the_reply_fails_without_resuming() {
    // The retries are spent, retries are off, or the failure is not worth retrying.
    assert_eq!(
        recovery_action(before_reply(true), true, 3, true, true),
        RecoveryAction::Fail
    );
    assert_eq!(
        recovery_action(before_reply(true), false, 0, true, true),
        RecoveryAction::Fail
    );
    assert_eq!(
        recovery_action(before_reply(false), true, 0, true, true),
        RecoveryAction::Fail
    );
}

#[test]
fn a_failure_after_the_reply_started_resumes_as_before() {
    assert_eq!(
        recovery_action(after_reply(true), true, 0, true, true),
        RecoveryAction::Resume
    );
    // Offline doesn't change the decision; the resume spawn waits for connectivity.
    assert_eq!(
        recovery_action(after_reply(true), true, 0, true, false),
        RecoveryAction::Resume
    );
    // The retry budget is irrelevant once part of the reply has arrived.
    assert_eq!(
        recovery_action(after_reply(true), false, 3, true, true),
        RecoveryAction::Resume
    );
    // Resume requests themselves run with can_attempt_resume_on_error=false, bounding recovery to
    // a single resume.
    assert_eq!(
        recovery_action(after_reply(true), true, 0, false, true),
        RecoveryAction::Fail
    );
    assert_eq!(
        recovery_action(after_reply(false), true, 0, true, true),
        RecoveryAction::Fail
    );
}

#[test]
fn a_provider_failure_carries_its_own_retry_facts() {
    let provider = AIApiError::Provider {
        error: Arc::new(AIApiError::ErrorStatus(
            http::StatusCode::SERVICE_UNAVAILABLE,
            String::new(),
        )),
        failure: ProviderFailure {
            before_reply: true,
            retryable: true,
            retry_after: Some(Duration::from_secs(3)),
        },
    };
    // The opening events of a failure the client will not retry arrive before the error, so the
    // stream has seen client actions; the provider still knows the reply never started.
    assert_eq!(
        AttemptFailure::of(&provider, true),
        AttemptFailure {
            before_reply: true,
            retryable: true,
            retry_after: Some(Duration::from_secs(3)),
            is_recoverable: true,
        }
    );

    let eof = AIApiError::UnexpectedEof;
    assert!(AttemptFailure::of(&eof, false).before_reply);
    assert!(!AttemptFailure::of(&eof, true).before_reply);
    assert!(AttemptFailure::of(&eof, false).retryable);
}
