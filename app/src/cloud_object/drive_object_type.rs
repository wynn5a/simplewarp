use std::fmt;

use crate::ui_components::icons::Icon;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DriveObjectType {
    Workflow,
    AgentModeWorkflow,
    AIFact,
    AIFactCollection,
    Notebook {
        /// Whether the notebook was created as an AI Document (plan)
        is_ai_document: bool,
    },
    Folder,
    EnvVarCollection,
    MCPServer,
    MCPServerCollection,
}

impl From<DriveObjectType> for Icon {
    fn from(cloud_object_type: DriveObjectType) -> Icon {
        match cloud_object_type {
            DriveObjectType::Workflow => Icon::Workflow,
            DriveObjectType::AgentModeWorkflow => Icon::Prompt,
            DriveObjectType::AIFact => Icon::BookOpen,
            DriveObjectType::AIFactCollection => Icon::BookOpen,
            DriveObjectType::Notebook { is_ai_document } => {
                if is_ai_document {
                    Icon::Compass
                } else {
                    Icon::Notebook
                }
            }
            DriveObjectType::Folder => Icon::Folder,
            DriveObjectType::EnvVarCollection => Icon::EnvVarCollection,
            DriveObjectType::MCPServer => Icon::Dataflow,
            DriveObjectType::MCPServerCollection => Icon::Dataflow,
        }
    }
}

impl fmt::Display for DriveObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DriveObjectType::Notebook { .. } => write!(f, "notebook"),
            DriveObjectType::Workflow => write!(f, "workflow"),
            DriveObjectType::Folder => write!(f, "folder"),
            DriveObjectType::EnvVarCollection => write!(f, "env var collection"),
            DriveObjectType::AgentModeWorkflow => write!(f, "prompt"),
            DriveObjectType::AIFact => write!(f, "ai fact"),
            DriveObjectType::AIFactCollection => write!(f, "ai fact collection"),
            DriveObjectType::MCPServer => write!(f, "mcp server"),
            DriveObjectType::MCPServerCollection => write!(f, "mcp server collection"),
        }
    }
}
