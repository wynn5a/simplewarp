pub use cloud_object_models::{CloudWorkflow, CloudWorkflowModel, WorkflowId};
use serde::{Deserialize, Serialize};
use warp_core::context_flag::ContextFlag;
use warpui::AppContext;

pub mod categories;
use workflow::Workflow;

pub mod aliases;
pub mod command_parser;
pub mod export_workflow;
pub mod info_box;
pub mod local_workflows;
pub mod manager;
pub mod workflow;
pub mod workflow_enum;
pub mod workflow_view;

use async_trait::async_trait;
pub use categories::{CategoriesView, CategoriesViewEvent, WorkflowsViewAction};
use cloud_objects::cloud_object::SerializedModel;

use crate::appearance::Appearance;
use crate::cloud_object::{CloudModelType, CloudObjectUpsertParams, ObjectType, WarpDriveItem};
use crate::drive::CloudObjectTypeAndId;
use crate::drive::items::workflow::WarpDriveWorkflow;
use crate::notebooks::{NotebookId, NotebookLocation};
use crate::persistence::ModelEvent;
use crate::server::ids::{ServerId, SyncId};

pub fn init(app: &mut AppContext) {
    categories::init(app);
    self::workflow_view::init(app);
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
pub enum WorkflowSource {
    Global,
    Local,
    Project,
    Team {
        team_uid: ServerId,
    },
    PersonalCloud,
    WarpAI,
    Notebook {
        notebook_id: Option<NotebookId>,
        team_uid: Option<ServerId>,
        location: NotebookLocation,
    },

    /// A hardcoded workflow type that allows Warp to surface features as Workflows (e.g.
    /// a command to see our network log)
    App,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Hash, PartialOrd)]
pub enum WorkflowSelectionSource {
    WarpDrive,
    CommandPalette,
    UniversalSearch,
    Voltron,
    WarpAI,
    Notebook,
    SlashMenu,
    UpArrowHistory,
    WorkflowView,
    AgentMode,
    Undefined,
    Alias,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowViewMode {
    View,
    Edit,
    Create,
}

impl WorkflowViewMode {
    /// The editing mode supported for a workflow.
    ///
    /// Editing is disabled if the user does not have edit permissions.
    pub fn supported_edit_mode(_workflow_id: Option<SyncId>, _app: &AppContext) -> Self {
        Self::Edit
    }

    /// The viewing mode supported for this workflow.
    ///
    /// Viewing is disabled if the user is allowed to edit the workflow and in a context where
    /// running workflows is supported.
    pub fn supported_view_mode(_workflow_id: Option<SyncId>, _app: &AppContext) -> Self {
        if ContextFlag::RunWorkflow.is_enabled() {
            Self::Edit
        } else {
            Self::View
        }
    }

    fn is_editable(&self) -> bool {
        match self {
            Self::View => false,
            Self::Edit | Self::Create => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AIWorkflowOrigin {
    CommandSearch,
    AgentMode,
    LegacyWarpAI,
}

/// Wrapper type for a workflow that may be saved locally or using cloud sync.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkflowType {
    /// Saved workflows sourced from local, global, project, app collections, saved locally.
    Local(Workflow),
    /// Saved workflows from personal or team collections, saved using cloud-sync.
    Cloud(Box<CloudWorkflow>),
    /// Ephemeral/transient workflows created from Warp AI output
    AIGenerated {
        workflow: Workflow,
        origin: AIWorkflowOrigin,
    },
    /// A workflow that's part of a cloud notebook.
    Notebook(Workflow),
}

impl WorkflowType {
    pub fn as_workflow(&self) -> &Workflow {
        match self {
            WorkflowType::Local(workflow) => workflow,
            WorkflowType::AIGenerated { workflow, .. } => workflow,
            WorkflowType::Cloud(workflow) => &workflow.model().data,
            WorkflowType::Notebook(workflow) => workflow,
        }
    }

    /// Returns the contained [`Workflow`], consuming `self`.
    pub fn take_workflow(self) -> Workflow {
        match self {
            WorkflowType::Local(workflow) => workflow,
            WorkflowType::AIGenerated { workflow, .. } => workflow,
            WorkflowType::Cloud(workflow) => workflow.model().data.clone(),
            WorkflowType::Notebook(workflow) => workflow,
        }
    }

    /// The object type and ID for the cloud object containing this workflow, if there is
    /// one. This is currently only supported for cloud workflows, not workflows within notebooks.
    pub fn object_id(&self) -> Option<CloudObjectTypeAndId> {
        match self {
            WorkflowType::Cloud(workflow) => Some(CloudObjectTypeAndId::Workflow(workflow.id)),
            _ => None,
        }
    }

    pub fn sync_id(&self) -> Option<SyncId> {
        match self {
            WorkflowType::Cloud(workflow) => Some(workflow.id),
            _ => None,
        }
    }

    pub fn server_id(&self) -> Option<WorkflowId> {
        match self.object_id() {
            Some(CloudObjectTypeAndId::Workflow(id)) => id.into_server().map(Into::into),
            _ => None,
        }
    }

    /// We don't show env var selection for Agent Mode suggested commands.
    pub(super) fn should_show_env_var_selection(&self) -> bool {
        !matches!(self, WorkflowType::AIGenerated { .. },)
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl CloudModelType for CloudWorkflowModel {
    type CloudObjectType = CloudWorkflow;
    type IdType = WorkflowId;

    fn serialized(&self) -> SerializedModel {
        SerializedModel::new(
            serde_json::to_string(&self.data).expect("failed to serialize workflow"),
        )
    }

    fn model_type_name(&self) -> &'static str {
        if self.data.is_agent_mode_workflow() {
            "Prompt"
        } else {
            "Workflow"
        }
    }

    fn object_type(&self) -> ObjectType {
        ObjectType::Workflow
    }

    fn cloud_object_type_and_id(&self, id: SyncId) -> CloudObjectTypeAndId {
        CloudObjectTypeAndId::Workflow(id)
    }

    fn display_name(&self) -> String {
        self.data.name().to_string()
    }

    fn set_display_name(&mut self, name: &str) {
        self.data.set_name(name);
    }

    fn upsert_event(params: CloudObjectUpsertParams<Self>) -> ModelEvent {
        ModelEvent::UpsertWorkflow {
            workflow: CloudWorkflow::from(params),
        }
    }

    fn bulk_upsert_event(objects: Vec<CloudObjectUpsertParams<Self>>) -> ModelEvent {
        ModelEvent::UpsertWorkflows(objects.into_iter().map(CloudWorkflow::from).collect())
    }

    fn should_update_after_server_conflict(&self) -> bool {
        true
    }
    fn renders_in_warp_drive(&self) -> bool {
        true
    }

    fn to_warp_drive_item(
        &self,
        id: SyncId,
        _appearance: &Appearance,
        workflow: &CloudWorkflow,
    ) -> Option<Box<dyn WarpDriveItem>> {
        Some(Box::new(WarpDriveWorkflow::new(
            self.cloud_object_type_and_id(id),
            workflow.clone(),
        )))
    }

    fn can_export(&self) -> bool {
        true
    }
}
