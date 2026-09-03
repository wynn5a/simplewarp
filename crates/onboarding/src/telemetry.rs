use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use strum_macros::{EnumDiscriminants, EnumIter};
use warp_core::telemetry::{EnablementState, TelemetryEvent, TelemetryEventDesc};

/// Telemetry events for the onboarding flow.
#[derive(Clone, Debug, Serialize, Deserialize, EnumDiscriminants)]
#[strum_discriminants(derive(EnumIter))]
#[strum_discriminants(name(OnboardingEventDiscriminant))]
pub enum OnboardingEvent {
    /// A callout was displayed.
    CalloutDisplayed { callout: String },
    /// The user clicked next on a callout.
    CalloutNext,
    /// The user completed the callout flow.
    CalloutCompleted { completion_type: String },
}

impl TelemetryEvent for OnboardingEvent {
    fn name(&self) -> &'static str {
        match self {
            OnboardingEvent::CalloutDisplayed { .. } => "onboarding_callout_displayed",
            OnboardingEvent::CalloutNext => "onboarding_callout_next",
            OnboardingEvent::CalloutCompleted { .. } => "onboarding_callout_completed",
        }
    }

    fn payload(&self) -> Option<Value> {
        match self {
            OnboardingEvent::CalloutDisplayed { callout } => Some(json!({
                "callout": callout,
            })),
            OnboardingEvent::CalloutNext => None,
            OnboardingEvent::CalloutCompleted { completion_type } => Some(json!({
                "completion_type": completion_type,
            })),
        }
    }

    fn description(&self) -> &'static str {
        match self {
            OnboardingEvent::CalloutDisplayed { .. } => "A callout was displayed to the user",
            OnboardingEvent::CalloutNext => "User clicked next on a callout",
            OnboardingEvent::CalloutCompleted { .. } => "User completed the callout flow",
        }
    }

    fn enablement_state(&self) -> EnablementState {
        EnablementState::Always
    }

    fn contains_ugc(&self) -> bool {
        false
    }

    fn event_descs() -> impl Iterator<Item = Box<dyn TelemetryEventDesc>> {
        warp_core::telemetry::enum_events::<Self>()
    }
}

impl TelemetryEventDesc for OnboardingEventDiscriminant {
    fn name(&self) -> &'static str {
        match self {
            OnboardingEventDiscriminant::CalloutDisplayed => "onboarding_callout_displayed",
            OnboardingEventDiscriminant::CalloutNext => "onboarding_callout_next",
            OnboardingEventDiscriminant::CalloutCompleted => "onboarding_callout_completed",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            OnboardingEventDiscriminant::CalloutDisplayed => "A callout was displayed to the user",
            OnboardingEventDiscriminant::CalloutNext => "User clicked next on a callout",
            OnboardingEventDiscriminant::CalloutCompleted => "User completed the callout flow",
        }
    }

    fn enablement_state(&self) -> EnablementState {
        EnablementState::Always
    }
}

warp_core::register_telemetry_event!(OnboardingEvent);

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;
