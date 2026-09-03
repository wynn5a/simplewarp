use serde_json::json;
use warp_core::telemetry::TelemetryEvent;

use super::OnboardingEvent;

#[test]
fn callout_event_payloads() {
    assert_eq!(
        OnboardingEvent::CalloutDisplayed {
            callout: "final_reminder".to_string(),
        }
        .payload(),
        Some(json!({ "callout": "final_reminder" }))
    );
    assert_eq!(OnboardingEvent::CalloutNext.payload(), None);
    assert_eq!(
        OnboardingEvent::CalloutCompleted {
            completion_type: "dismissed".to_string(),
        }
        .payload(),
        Some(json!({ "completion_type": "dismissed" }))
    );
}
