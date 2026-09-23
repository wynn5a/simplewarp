use super::{PricingPromotionState, PricingPromotionSurface};
#[test]
fn agent_and_terminal_dismissals_are_independent() {
    let mut state = PricingPromotionState {
        agent_dismissed: true,
        terminal_dismissed: false,
        displayed_surfaces: Default::default(),
    };
    assert!(state.is_dismissed(PricingPromotionSurface::AgentMessageBar));
    assert!(!state.is_dismissed(PricingPromotionSurface::TerminalMessageBar));

    state.terminal_dismissed = true;
    assert!(state.is_dismissed(PricingPromotionSurface::TerminalMessageBar));
    assert!(state.is_dismissed(PricingPromotionSurface::AgentMessageBar));
}
