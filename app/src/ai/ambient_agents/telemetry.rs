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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffSurface {
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    Gui,
    #[allow(dead_code)]
    Tui,
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

/// Describes which synthetic-input path drives an empty-prompt handoff.
/// Captured at handoff initiation so telemetry reflects the intended path
/// regardless of whether the snapshot derivation later produces content.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandoffInjectionPath {
    /// The handoff carried a non-empty user prompt; no client-side injection.
    #[default]
    None,
    /// Empty prompt + in-progress source. The client substituted `"Continue"`
    /// on the wire so the cloud agent picks up where the local agent left off.
    Continue,
    /// Empty prompt + idle source. The client substituted
    /// `"Apply the workspace changes from my previous session."` on the wire
    /// alongside the snapshot token; the cloud agent's first user-role turn
    /// carries an intent for the rehydrated workspace state.
    SnapshotRehydration,
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
    /// User initiated a local-to-cloud handoff.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    HandoffInitiated {
        /// How the handoff was triggered.
        entry_point: HandoffEntryPoint,
        /// Frontend that initiated the handoff.
        surface: HandoffSurface,
        /// Whether the handoff forked an existing conversation.
        forked_existing_conversation: bool,
        /// Whether the user submitted with an empty prompt buffer.
        empty_prompt: bool,
        /// Which synthetic-input path drives this submission (relevant only
        /// when `empty_prompt` is true; always `None` otherwise). Captured at
        /// handoff initiation, before snapshot derivation has settled — the
        /// `HandoffSnapshotPrepared` event reports the actual snapshot result.
        injection_path: HandoffInjectionPath,
    },
    /// The async snapshot-upload pipeline that backs a handoff has settled.
    /// Fires once per handoff after `derive_touched_workspace` completes.
    /// Pair with `HandoffInitiated` on the same run to learn whether the
    /// `SnapshotRehydration` injection path actually carried snapshot content.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    HandoffSnapshotPrepared {
        /// True when the derived `TouchedWorkspace` had at least one repo or
        /// orphan file. Reports what the snapshot pipeline produced; the upload
        /// itself may still fail downstream, so the wire prompt is not implied.
        derived_workspace_had_content: bool,
    },
    /// The auto-handoff sleep discoverability prompt was surfaced on wake.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    SleepPromptShown,
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
            CloudAgentTelemetryEvent::HandoffInitiated {
                entry_point,
                surface,
                forked_existing_conversation,
                empty_prompt,
                injection_path,
            } => Some(json!({
                "entry_point": entry_point,
                "surface": surface,
                "forked_existing_conversation": forked_existing_conversation,
                "empty_prompt": empty_prompt,
                "injection_path": injection_path,
            })),
            CloudAgentTelemetryEvent::HandoffSnapshotPrepared {
                derived_workspace_had_content,
            } => Some(json!({
                "derived_workspace_had_content": derived_workspace_had_content,
            })),
            CloudAgentTelemetryEvent::SleepPromptShown
            | CloudAgentTelemetryEvent::SleepPromptEnabled
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
            Self::HandoffInitiated => "AmbientAgent.Handoff.Initiated",
            Self::HandoffSnapshotPrepared => "AmbientAgent.Handoff.SnapshotPrepared",
            Self::SleepPromptShown => "AmbientAgent.Handoff.SleepPrompt.Shown",
            Self::SleepPromptEnabled => "AmbientAgent.Handoff.SleepPrompt.Enabled",
            Self::SleepPromptDismissed => "AmbientAgent.Handoff.SleepPrompt.Dismissed",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::EnteredCloudMode => "User entered cloud agent view",
            Self::DispatchFailed => "Ambient agent failed to dispatch or encountered an error",
            Self::HandoffInitiated => "User initiated a local-to-cloud handoff",
            Self::HandoffSnapshotPrepared => {
                "Handoff snapshot upload settled; reports whether it carried content"
            }
            Self::SleepPromptShown => {
                "The auto-handoff sleep discoverability prompt was shown on wake"
            }
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
