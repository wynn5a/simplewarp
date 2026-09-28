use warp_multi_agent_api as api;

/// Client-side representation of the orchestration config attached to a
/// conversation via `OrchestrationConfigSnapshot`.
///
/// Mirrors the proto `OrchestrationConfig` but uses Rust-native types
/// to keep view / model code free of proto imports. Children always run
/// locally, so the proto's execution mode is not carried: a persisted Remote
/// config reads as local.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OrchestrationConfig {
    pub model_id: String,
    pub harness_type: String,
}

/// User's approval state for orchestration on the active config.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub enum OrchestrationConfigStatus {
    /// No `OrchestrationConfigSnapshot` has been seen yet.
    #[default]
    None,
    Approved,
    Disapproved,
}

impl OrchestrationConfigStatus {
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }

    pub fn is_disapproved(&self) -> bool {
        matches!(self, Self::Disapproved)
    }
}

// ---------------------------------------------------------------------------
// Proto ↔ native conversions
// ---------------------------------------------------------------------------

impl OrchestrationConfig {
    /// Converts from the proto `OrchestrationConfig` message.
    pub fn from_proto(proto: &api::OrchestrationConfig) -> Self {
        Self {
            model_id: proto.model_id.clone(),
            harness_type: harness_proto_to_string(proto.harness.as_ref()).unwrap_or_default(),
        }
    }

    /// Converts to the proto `OrchestrationConfig` message.
    pub fn to_proto(&self) -> api::OrchestrationConfig {
        api::OrchestrationConfig {
            model_id: self.model_id.clone(),
            harness: harness_type_to_proto(&self.harness_type),
            execution_mode: Some(api::orchestration_config::ExecutionMode::Local(
                api::orchestration_config::Local {},
            )),
        }
    }
}

impl OrchestrationConfigStatus {
    /// Converts from the proto `OrchestrationStatus` message.
    pub fn from_proto(proto: Option<&api::OrchestrationStatus>) -> Self {
        let Some(status) = proto else {
            return Self::None;
        };
        match &status.status {
            Some(api::orchestration_status::Status::Approved(_)) => Self::Approved,
            Some(api::orchestration_status::Status::Disapproved(_)) => Self::Disapproved,
            None => Self::None,
        }
    }

    /// Converts to the proto `OrchestrationStatus` message.
    pub fn to_proto(&self) -> Option<api::OrchestrationStatus> {
        match self {
            Self::None => None,
            Self::Approved => Some(api::OrchestrationStatus {
                status: Some(api::orchestration_status::Status::Approved(
                    api::orchestration_status::Approved {},
                )),
            }),
            Self::Disapproved => Some(api::OrchestrationStatus {
                status: Some(api::orchestration_status::Status::Disapproved(
                    api::orchestration_status::Disapproved {},
                )),
            }),
        }
    }
}

/// Maps the proto `Harness` oneof to a client-side string identifier.
/// Returns `None` for an unset variant.
fn harness_proto_to_string(harness: Option<&api::Harness>) -> Option<String> {
    let variant = harness?.variant.as_ref()?;
    Some(
        match variant {
            api::harness::Variant::Oz(_) => "oz",
            api::harness::Variant::ClaudeCode(_) => "claude",
            api::harness::Variant::OpenCode(_) => "opencode",
            api::harness::Variant::Gemini(_) => "gemini",
            api::harness::Variant::Codex(_) => "codex",
        }
        .to_string(),
    )
}

/// Converts a client-side harness string identifier to the proto `Harness`
/// oneof variant. Returns `None` for empty or unknown strings.
fn harness_type_to_proto(harness_type: &str) -> Option<api::Harness> {
    let variant = match harness_type {
        "oz" => api::harness::Variant::Oz(api::harness::Oz {}),
        "claude" => api::harness::Variant::ClaudeCode(api::harness::ClaudeCode {}),
        "opencode" => api::harness::Variant::OpenCode(api::harness::OpenCode {}),
        "gemini" => api::harness::Variant::Gemini(api::harness::Gemini {}),
        "codex" => api::harness::Variant::Codex(api::harness::Codex {}),
        _ => return None,
    };
    Some(api::Harness {
        variant: Some(variant),
    })
}

#[cfg(test)]
#[path = "orchestration_config_tests.rs"]
mod tests;
