use crate::schema;

#[derive(cynic::Enum, Clone, Copy, Debug, PartialEq)]
pub enum AgentTaskState {
    #[cynic(rename = "BLOCKED")]
    Blocked,
    #[cynic(rename = "CANCELLED")]
    Cancelled,
    #[cynic(rename = "CLAIMED")]
    Claimed,
    #[cynic(rename = "ERROR")]
    Error,
    #[cynic(rename = "IN_PROGRESS")]
    InProgress,
    #[cynic(rename = "SUCCEEDED")]
    Succeeded,
    #[cynic(rename = "FAILED")]
    Failed,
}

#[derive(cynic::Enum, Clone, Debug, PartialEq)]
pub enum AgentHarness {
    Oz,
    ClaudeCode,
    Gemini,
    Codex,
    #[cynic(fallback)]
    Other(String),
}
