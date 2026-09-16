use serde::Serialize;
use serde_json::{Value, json};
use strum_macros::{EnumDiscriminants, EnumIter};
use warp_core::register_telemetry_event;
use warp_core::telemetry::{EnablementState, TelemetryEvent, TelemetryEventDesc};

#[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
#[derive(Clone, EnumDiscriminants)]
#[strum_discriminants(derive(EnumIter))]
pub enum AITelemetryEvent {
    ProviderCredentialChanged {
        provider: ProviderCredentialTelemetryProvider,
        credential_kind: ProviderCredentialTelemetryKind,
        action: ProviderCredentialTelemetryAction,
    },
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderCredentialTelemetryProvider {
    OpenAi,
    Anthropic,
    Google,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderCredentialTelemetryKind {
    PastedKey,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderCredentialTelemetryAction {
    Added,
    Removed,
}

impl TelemetryEvent for AITelemetryEvent {
    fn name(&self) -> &'static str {
        AITelemetryEventDiscriminants::from(self).name()
    }

    fn description(&self) -> &'static str {
        AITelemetryEventDiscriminants::from(self).description()
    }

    fn enablement_state(&self) -> EnablementState {
        AITelemetryEventDiscriminants::from(self).enablement_state()
    }

    fn payload(&self) -> Option<Value> {
        match self {
            Self::ProviderCredentialChanged {
                provider,
                credential_kind,
                action,
            } => Some(json!({
                "provider": provider,
                "credential_kind": credential_kind,
                "action": action,
            })),
        }
    }

    fn contains_ugc(&self) -> bool {
        match self {
            Self::ProviderCredentialChanged { .. } => false,
        }
    }

    fn event_descs() -> impl Iterator<Item = Box<dyn TelemetryEventDesc>> {
        warp_core::telemetry::enum_events::<Self>()
    }
}

impl TelemetryEventDesc for AITelemetryEventDiscriminants {
    fn name(&self) -> &'static str {
        match self {
            Self::ProviderCredentialChanged => "AI.ProviderCredential.Changed",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::ProviderCredentialChanged => {
                "A user added or removed a model-provider credential"
            }
        }
    }

    fn enablement_state(&self) -> EnablementState {
        match self {
            Self::ProviderCredentialChanged => EnablementState::Always,
        }
    }
}

register_telemetry_event!(AITelemetryEvent);

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;
