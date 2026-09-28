//! Ambient agent task types and utilities.

pub use cloud_object_models::{AgentConfigSnapshot, HarnessConfig, HarnessModelConfig};
use serde::Serialize;

/// Single attachment input captured on the client (e.g., a file upload).
#[derive(Clone, Debug, Serialize)]
pub struct AttachmentInput {
    pub file_name: String,
    pub mime_type: String,
    pub data: String,
}

/// Returns the trimmed orchestrator agent name, or `None` when empty / whitespace-only.
pub fn normalize_orchestrator_agent_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
