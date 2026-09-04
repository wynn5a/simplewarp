use serde::Serialize;
use serde_json::{Value, json};
use strum_macros::{EnumDiscriminants, EnumIter};
use warp_core::features::FeatureFlag;
use warp_core::telemetry::{EnablementState, TelemetryEvent, TelemetryEventDesc};

/// The entry point through which Cloud Mode was entered.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudModeEntryPoint {
    /// User clicked "New Cloud Agent Tab" or similar action to create a dedicated Cloud Mode tab.
    NewTab,
    /// User entered Cloud Mode from an existing local terminal session (e.g., via keyboard shortcut or command).
    LocalSession,
    /// User entered Cloud Mode through the Oz launch modal.
    OzLaunchModal,
    /// User re-entered Cloud Mode by clicking on an ambient agent entry block.
    EntryBlock,
}

/// The entry point through which a local-to-cloud handoff was initiated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffEntryPoint {
    /// User typed `&` in the input to enter handoff compose mode.
    #[default]
    Ampersand,
    /// User used the `/handoff` slash command.
    SlashCommand,
    /// User clicked the "Hand off to cloud" chip in the footer toolbar.
    FooterChip,
    /// The client automatically initiated handoff for an eligible local agent.
    Automatic,
}

/// Telemetry events for client interactions with cloud agents.
#[derive(Debug, EnumDiscriminants)]
#[strum_discriminants(derive(EnumIter))]
pub enum CloudAgentTelemetryEvent {
    /// User entered Cloud Mode.
    EnteredCloudMode { entry_point: CloudModeEntryPoint },
    /// Ambient agent failed to dispatch or encountered an error during subscription.
    DispatchFailed {
        /// Error message describing the failure.
        error: String,
    },
    /// User clicked "Enable" on the auto-handoff sleep prompt.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    SleepPromptEnabled,
    /// User clicked "Dismiss" on the auto-handoff sleep prompt.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    SleepPromptDismissed,
}

impl TelemetryEvent for CloudAgentTelemetryEvent {
    fn name(&self) -> &'static str {
        CloudAgentTelemetryEventDiscriminants::from(self).name()
    }

    fn payload(&self) -> Option<Value> {
        match self {
            CloudAgentTelemetryEvent::EnteredCloudMode { entry_point } => Some(json!({
                "entry_point": entry_point,
            })),
            CloudAgentTelemetryEvent::DispatchFailed { error } => Some(json!({
                "error": error,
            })),
            CloudAgentTelemetryEvent::SleepPromptEnabled
            | CloudAgentTelemetryEvent::SleepPromptDismissed => None,
        }
    }

    fn description(&self) -> &'static str {
        CloudAgentTelemetryEventDiscriminants::from(self).description()
    }

    fn enablement_state(&self) -> EnablementState {
        CloudAgentTelemetryEventDiscriminants::from(self).enablement_state()
    }

    fn contains_ugc(&self) -> bool {
        false
    }

    fn event_descs() -> impl Iterator<Item = Box<dyn TelemetryEventDesc>> {
        warp_core::telemetry::enum_events::<Self>()
    }
}

impl TelemetryEventDesc for CloudAgentTelemetryEventDiscriminants {
    fn name(&self) -> &'static str {
        match self {
            Self::EnteredCloudMode => "AmbientAgent.CloudMode.Entered",
            Self::DispatchFailed => "AmbientAgent.DispatchFailed",
            Self::SleepPromptEnabled => "AmbientAgent.Handoff.SleepPrompt.Enabled",
            Self::SleepPromptDismissed => "AmbientAgent.Handoff.SleepPrompt.Dismissed",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::EnteredCloudMode => "User entered cloud agent view",
            Self::DispatchFailed => "Ambient agent failed to dispatch or encountered an error",
            Self::SleepPromptEnabled => {
                "User enabled auto-handoff on sleep from the discoverability prompt"
            }
            Self::SleepPromptDismissed => {
                "User dismissed the auto-handoff sleep discoverability prompt"
            }
        }
    }

    fn enablement_state(&self) -> EnablementState {
        EnablementState::Flag(FeatureFlag::CloudMode)
    }
}

warp_core::register_telemetry_event!(CloudAgentTelemetryEvent);
