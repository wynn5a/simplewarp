use std::fmt;

use cloud_objects::cloud_object::{GenericStringObjectFormat, JsonObjectType};

use crate::cloud_object::{CloudObject, ObjectType};
use crate::notebooks::CloudNotebook;
use crate::ui_components::icons::Icon;
use crate::workflows::CloudWorkflow;

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

impl DriveObjectType {
    /// Returns the display type of a cloud object, or None for object kinds
    /// that have no display type (e.g. preferences or workflow enums).
    pub fn for_cloud_object(object: &dyn CloudObject) -> Option<Self> {
        match object.object_type() {
            ObjectType::Notebook => Some(Self::Notebook {
                is_ai_document: object
                    .as_any()
                    .downcast_ref::<CloudNotebook>()
                    .is_some_and(|notebook| notebook.model().ai_document_id.is_some()),
            }),
            ObjectType::Workflow => Some(
                object
                    .as_any()
                    .downcast_ref::<CloudWorkflow>()
                    .filter(|workflow| workflow.model().data.is_agent_mode_workflow())
                    .map(|_| Self::AgentModeWorkflow)
                    .unwrap_or(Self::Workflow),
            ),
            ObjectType::Folder => Some(Self::Folder),
            ObjectType::GenericStringObject(GenericStringObjectFormat::Json(object_type)) => {
                match object_type {
                    JsonObjectType::EnvVarCollection => Some(Self::EnvVarCollection),
                    JsonObjectType::AIFact => Some(Self::AIFact),
                    JsonObjectType::MCPServer => Some(Self::MCPServer),
                    JsonObjectType::WorkflowEnum
                    | JsonObjectType::AIExecutionProfile
                    | JsonObjectType::TemplatableMCPServer
                    | JsonObjectType::CloudEnvironment
                    | JsonObjectType::ScheduledAmbientAgent
                    | JsonObjectType::CloudAgentConfig => None,
                }
            }
        }
    }
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
