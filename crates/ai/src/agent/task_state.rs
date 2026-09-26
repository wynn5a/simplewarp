#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AgentTaskState {
    Blocked,
    Cancelled,
    Claimed,
    Error,
    InProgress,
    Succeeded,
    Failed,
}
