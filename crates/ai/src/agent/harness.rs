#[derive(Clone, Debug, PartialEq)]
pub enum AgentHarness {
    Oz,
    ClaudeCode,
    Gemini,
    Codex,
    Other(String),
}
