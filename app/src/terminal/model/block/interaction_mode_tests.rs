use super::*;

fn agent_mode(
    requested_command_action_id: Option<AIAgentActionId>,
    long_running_control_state: Option<LongRunningCommandControlState>,
) -> InteractionMode {
    InteractionMode::new_agent(AgentInteractionMetadata::new(
        requested_command_action_id,
        AIConversationId::new(),
        None,
        long_running_control_state,
        false,
        false,
    ))
}

fn transfer() -> UserTakeOverReason {
    UserTakeOverReason::TransferFromAgent {
        reason: "Enter your password".to_owned(),
    }
}

#[test]
fn a_local_agent_command_can_be_handed_to_the_user_and_back() {
    let mut mode = agent_mode(Some(AIAgentActionId::from("call-1".to_owned())), None);

    mode.take_over_for_user(transfer())
        .expect("an agent-requested command can be handed over");
    assert!(
        mode.long_running_control_state()
            .is_some_and(LongRunningCommandControlState::is_user_in_control)
    );

    mode.handoff_to_agent()
        .expect("the user can hand control back");
    assert!(
        mode.long_running_control_state()
            .is_some_and(LongRunningCommandControlState::is_agent_in_control)
    );
}

#[test]
fn a_command_the_agent_did_not_request_cannot_be_taken_over() {
    let mut mode = agent_mode(None, None);
    assert!(mode.take_over_for_user(transfer()).is_err());
}

#[test]
fn a_command_the_user_already_controls_cannot_be_taken_over_again() {
    let mut mode = agent_mode(
        Some(AIAgentActionId::from("call-1".to_owned())),
        Some(LongRunningCommandControlState::User {
            reason: UserTakeOverReason::Manual,
        }),
    );
    assert!(mode.take_over_for_user(transfer()).is_err());
}
