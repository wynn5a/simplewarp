use serde::Serialize;

use crate::tab_configs::session_config::SessionType;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingTabConfigOpenMode {
    Direct,
    ParamsModal,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NewWorktreeConfigOpenSource {
    Submenu,
    NewWorktreeModal,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeBranchNamingMode {
    Auto,
    Manual,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuidedModalSessionType {
    Terminal,
    Oz,
    CliAgent,
}

impl From<&SessionType> for GuidedModalSessionType {
    fn from(value: &SessionType) -> Self {
        match value {
            SessionType::Terminal => Self::Terminal,
            SessionType::Oz => Self::Oz,
            SessionType::CliAgent(_) => Self::CliAgent,
        }
    }
}
