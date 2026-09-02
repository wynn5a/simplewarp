use crate::ai::agent::conversation::ConversationStatus;

#[test]
fn should_trigger_notification_returns_true_for_success() {
    assert!(ConversationStatus::Success.should_trigger_notification());
}

#[test]
fn should_trigger_notification_returns_true_for_blocked() {
    assert!(
        ConversationStatus::Blocked {
            blocked_action: "approve diff".to_owned(),
        }
        .should_trigger_notification()
    );
}

#[test]
fn should_trigger_notification_returns_true_for_error() {
    assert!(ConversationStatus::Error.should_trigger_notification());
}

#[test]
fn should_trigger_notification_returns_false_for_in_progress() {
    assert!(!ConversationStatus::InProgress.should_trigger_notification());
}

#[test]
fn should_trigger_notification_returns_false_for_waiting_for_events() {
    assert!(!ConversationStatus::WaitingForEvents.should_trigger_notification());
}

#[test]
fn should_trigger_notification_returns_false_for_cancelled() {
    assert!(!ConversationStatus::Cancelled.should_trigger_notification());
}
