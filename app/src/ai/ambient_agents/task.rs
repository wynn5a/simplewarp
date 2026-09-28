//! Ambient agent task types and utilities.

pub use cloud_object_models::{AgentConfigSnapshot, HarnessConfig, HarnessModelConfig};

/// Returns the trimmed orchestrator agent name, or `None` when empty / whitespace-only.
pub fn normalize_orchestrator_agent_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
