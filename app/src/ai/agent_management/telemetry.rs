use serde::Serialize;
use serde_json::json;
use strum_macros::{EnumDiscriminants, EnumIter};
use warp_core::telemetry::{EnablementState, TelemetryEvent, TelemetryEventDesc};

/// Where the item was opened from
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenedFrom {
    ConversationList,
    DetailsPanel,
}

/// Telemetry events for the agent management view
#[derive(Serialize, Debug, EnumDiscriminants)]
#[strum_discriminants(derive(EnumIter))]
pub enum AgentManagementTelemetryEvent {
    /// User opened a conversation
    ConversationOpened {
        conversation_id: String,
        opened_from: OpenedFrom,
    },
    /// User copied a conversation link
    ConversationLinkCopied {
        conversation_id: String,
        copied_from: OpenedFrom,
    },
    /// User clicked "Continue locally" in the details panel
    #[cfg(not(target_family = "wasm"))]
    DetailsPanelContinueLocally,
    /// User clicked "Open in Warp" in the tombstone (wasm)
    #[cfg(target_family = "wasm")]
    TombstoneOpenInWarp,
    /// User cancelled a cloud run
    CloudRunCancelled { task_id: String },
    /// User forked a conversation
    ConversationForked { conversation_id: String },
}

impl TelemetryEvent for AgentManagementTelemetryEvent {
    fn name(&self) -> &'static str {
        AgentManagementTelemetryEventDiscriminants::from(self).name()
    }

    fn payload(&self) -> Option<serde_json::Value> {
        match self {
            AgentManagementTelemetryEvent::ConversationOpened {
                conversation_id,
                opened_from,
            } => Some(json!({
                "conversation_id": conversation_id,
                "opened_from": opened_from,
            })),
            AgentManagementTelemetryEvent::ConversationLinkCopied {
                conversation_id,
                copied_from,
            } => Some(json!({
                "conversation_id": conversation_id,
                "copied_from": copied_from,
            })),
            #[cfg(not(target_family = "wasm"))]
            AgentManagementTelemetryEvent::DetailsPanelContinueLocally => None,
            #[cfg(target_family = "wasm")]
            AgentManagementTelemetryEvent::TombstoneOpenInWarp => None,
            AgentManagementTelemetryEvent::CloudRunCancelled { task_id } => {
                Some(json!({ "task_id": task_id }))
            }
            AgentManagementTelemetryEvent::ConversationForked { conversation_id } => {
                Some(json!({ "conversation_id": conversation_id }))
            }
        }
    }

    fn description(&self) -> &'static str {
        AgentManagementTelemetryEventDiscriminants::from(self).description()
    }

    fn enablement_state(&self) -> EnablementState {
        AgentManagementTelemetryEventDiscriminants::from(self).enablement_state()
    }

    fn contains_ugc(&self) -> bool {
        false
    }

    fn event_descs() -> impl Iterator<Item = Box<dyn TelemetryEventDesc>> {
        warp_core::telemetry::enum_events::<Self>()
    }
}

impl TelemetryEventDesc for AgentManagementTelemetryEventDiscriminants {
    fn name(&self) -> &'static str {
        match self {
            Self::ConversationOpened => "AgentManagement.ConversationOpened",
            Self::ConversationLinkCopied => "AgentManagement.ConversationLinkCopied",
            #[cfg(not(target_family = "wasm"))]
            Self::DetailsPanelContinueLocally => "AgentManagement.DetailsPanelContinueLocally",
            #[cfg(target_family = "wasm")]
            Self::TombstoneOpenInWarp => "AgentManagement.TombstoneOpenInWarp",
            Self::CloudRunCancelled => "AgentManagement.CloudRunCancelled",
            Self::ConversationForked => "AgentManagement.ConversationForked",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::ConversationOpened => "User opened a conversation",
            Self::ConversationLinkCopied => "User copied a conversation link",
            #[cfg(not(target_family = "wasm"))]
            Self::DetailsPanelContinueLocally => {
                "User clicked Continue locally in the details panel"
            }
            #[cfg(target_family = "wasm")]
            Self::TombstoneOpenInWarp => "User clicked Open in Warp in the tombstone",
            Self::CloudRunCancelled => "User cancelled a cloud run",
            Self::ConversationForked => "User forked a conversation",
        }
    }

    fn enablement_state(&self) -> EnablementState {
        EnablementState::Always
    }
}

warp_core::register_telemetry_event!(AgentManagementTelemetryEvent);
