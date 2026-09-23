use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SkillOpenOrigin {
    // 'Open skill' button on ReadSkill tool call result
    ReadSkill,
    // 'Open skill' button on ReadFiles tool call result
    ReadFiles,
    // 'Open skill' button on CodeDiffView
    EditFiles,
    // /open-skill command
    OpenSkillCommand,
}
