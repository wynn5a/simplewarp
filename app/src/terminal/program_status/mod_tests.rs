use super::*;

fn record(state: ProgramState, kind: Option<BlockedKind>) -> ProgramStatusRecord {
    ProgramStatusRecord {
        state,
        kind,
        progress: None,
        app: Some("any-agent".to_owned()),
        title: None,
        message: None,
    }
}

#[test]
fn working_and_idle_raise_nothing() {
    for state in [ProgramState::Working, ProgramState::Idle] {
        assert_eq!(
            ProgramAttention::newly_raised(None, &record(state, None)),
            None
        );
    }
}

#[test]
fn entering_an_attention_state_raises_it() {
    let working = record(ProgramState::Working, None);
    let cases = [
        (
            record(ProgramState::Blocked, Some(BlockedKind::Permission)),
            ProgramAttention::NeedsInput(Some(BlockedKind::Permission)),
        ),
        (
            record(ProgramState::Done, None),
            ProgramAttention::Finished { success: true },
        ),
        (
            record(ProgramState::Error, None),
            ProgramAttention::Finished { success: false },
        ),
    ];
    for (next, expected) in cases {
        assert_eq!(
            ProgramAttention::newly_raised(Some(&working), &next),
            Some(expected)
        );
        assert_eq!(ProgramAttention::newly_raised(None, &next), Some(expected));
    }
}

#[test]
fn repeating_the_same_attention_raises_nothing() {
    let done = record(ProgramState::Done, None);
    let mut done_with_message = done.clone();
    done_with_message.message = Some("updated".to_owned());
    assert_eq!(
        ProgramAttention::newly_raised(Some(&done), &done_with_message),
        None
    );
}

#[test]
fn changing_blocked_kind_raises_again() {
    let permission = record(ProgramState::Blocked, Some(BlockedKind::Permission));
    let question = record(ProgramState::Blocked, Some(BlockedKind::Question));
    assert_eq!(
        ProgramAttention::newly_raised(Some(&permission), &question),
        Some(ProgramAttention::NeedsInput(Some(BlockedKind::Question)))
    );
}

#[test]
fn blocked_then_done_raises_finished() {
    let blocked = record(ProgramState::Blocked, None);
    assert_eq!(
        ProgramAttention::newly_raised(Some(&blocked), &record(ProgramState::Done, None)),
        Some(ProgramAttention::Finished { success: true })
    );
}
