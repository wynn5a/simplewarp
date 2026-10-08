use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use super::CloudNotebookModel;
use crate::ai::document::ai_document_model::AIDocumentId;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{CloudObject, Owner};
use crate::notebooks::CloudNotebook;
use crate::server::cloud_objects::update_manager::{
    ObjectOperation, OperationSuccessType, UpdateManager, UpdateManagerEvent,
};
use crate::server::ids::{ClientId, SyncId};

#[derive(Default, Clone)]
pub enum ActiveNotebook {
    #[default]
    None,
    // A notebook already stored in CloudModel, all relevant data should be queried
    // from CloudModel directly
    CommittedNotebook(SyncId),
    // A notebook that has been created and displayed in the view, but is not yet
    // committed to CloudModel
    NewNotebook(Box<CloudNotebook>),
}

#[derive(PartialEq, Eq, Default, Clone, Copy, Debug)]
pub enum Mode {
    #[default]
    Editing,
    View,
}

/// True if the object is currently being saved. We don't allow editing workflows
/// yet so this is only used for notebooks, but we will want it to apply for
/// workflows also.
#[derive(Default)]
pub enum SavingStatus {
    #[default]
    Saved,
    Saving,
}

/// Data displayed in the status bar that is also relevant for workflows and notebooks.
/// We share this data between views by making it a model.
#[derive(Default)]
pub struct ActiveNotebookData {
    /// Whether we're in editing, readonly or viewing mode.
    pub mode: Mode,
    pub saving_status: SavingStatus,
    pub active_notebook: ActiveNotebook,

    pub feature_not_available: bool,
}

impl ActiveNotebookData {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let update_manager = UpdateManager::handle(ctx);

        ctx.subscribe_to_model(&update_manager, |me, _, event, ctx| {
            me.handle_update_manager_event(event, ctx);
        });

        Self {
            ..Default::default()
        }
    }

    fn handle_update_manager_event(
        &mut self,
        event: &UpdateManagerEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let UpdateManagerEvent::ObjectOperationComplete { result } = event;

        if let (ObjectOperation::Trash | ObjectOperation::Untrash, OperationSuccessType::Success) =
            (&result.operation, &result.success_type)
            && self.id() == Some(result.id)
        {
            ctx.emit(ActiveNotebookDataEvent::TrashStatusChanged);
        }
    }

    pub fn reset(&mut self) {
        self.mode = Mode::View;
        self.saving_status = SavingStatus::default();
        self.active_notebook = ActiveNotebook::None;
        self.feature_not_available = false;
    }

    pub fn open_new(
        &mut self,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.reset();

        // create a new client id
        let new_id = ClientId::default();

        // Set the active notebook to be an uncommitted notebook
        self.active_notebook = ActiveNotebook::NewNotebook(Box::new(CloudNotebook::new_local(
            CloudNotebookModel::default(),
            owner,
            initial_folder_id,
            new_id,
        )));
        ctx.notify();
    }

    pub fn open_existing(&mut self, notebook_id: SyncId, ctx: &mut ModelContext<Self>) {
        self.reset();
        self.active_notebook = ActiveNotebook::CommittedNotebook(notebook_id);
        ctx.notify();
    }

    pub fn id(&self) -> Option<SyncId> {
        match &self.active_notebook {
            ActiveNotebook::None => None,
            ActiveNotebook::CommittedNotebook(id) => Some(*id),
            ActiveNotebook::NewNotebook(notebook) => Some(notebook.id),
        }
    }

    pub fn ai_document_id(&self, ctx: &AppContext) -> Option<AIDocumentId> {
        match &self.active_notebook {
            ActiveNotebook::None => None,
            ActiveNotebook::CommittedNotebook(id) => CloudModel::as_ref(ctx)
                .get_notebook(id)
                .and_then(|n| n.model().ai_document_id),
            ActiveNotebook::NewNotebook(notebook) => notebook.model().ai_document_id,
        }
    }

    pub fn active_notebook(&self) -> ActiveNotebook {
        self.active_notebook.clone()
    }

    /// Whether a notebook is open and still present (possibly trashed) in [`CloudModel`].
    pub fn exists(&self, app: &AppContext) -> bool {
        match &self.active_notebook {
            ActiveNotebook::None => false,
            ActiveNotebook::CommittedNotebook(id) => {
                CloudModel::as_ref(app).get_notebook(id).is_some()
            }
            ActiveNotebook::NewNotebook(_) => true,
        }
    }

    pub fn is_active_notebook(&self, notebook_id: SyncId) -> bool {
        self.id() == Some(notebook_id)
    }

    pub fn feature_not_available(&self) -> bool {
        self.feature_not_available
    }

    /// Checks if this notebook is trashed or deleted.
    pub fn trash_status(&self, ctx: &AppContext) -> TrashStatus {
        match &self.active_notebook {
            ActiveNotebook::None | ActiveNotebook::NewNotebook(_) => TrashStatus::Active,
            ActiveNotebook::CommittedNotebook(id) => {
                let cloud_model = CloudModel::as_ref(ctx);
                match cloud_model.get_notebook(id) {
                    Some(notebook) => {
                        if notebook.is_trashed(cloud_model) {
                            TrashStatus::Trashed
                        } else {
                            TrashStatus::Active
                        }
                    }
                    None => TrashStatus::Deleted,
                }
            }
        }
    }
}

pub enum ActiveNotebookDataEvent {
    /// This notebook was trashed or untrashed (used for refreshing pane overflow items)
    TrashStatusChanged,
}

/// Whether or not a notebook is trashed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrashStatus {
    Active,
    Trashed,
    Deleted,
}

impl TrashStatus {
    /// Whether or not the notebook can be edited in this state.
    pub fn is_editable(self) -> bool {
        match self {
            TrashStatus::Active => true,
            TrashStatus::Trashed | TrashStatus::Deleted => false,
        }
    }
}

impl Entity for ActiveNotebookData {
    type Event = ActiveNotebookDataEvent;
}
