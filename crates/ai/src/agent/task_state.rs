#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AgentTaskState {
    Blocked,
    Cancelled,
    Error,
    Failed,
}
