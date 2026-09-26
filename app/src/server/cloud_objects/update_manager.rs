use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use chrono::Utc;
use cloud_objects::time::ServerTimestamp;
use lazy_static::lazy_static;
use regex::Regex;
use warp_errors::report_error;
use warpui::{AppContext, Entity, ModelContext, RetryOption, SingletonEntity};

use crate::ai::execution_profiles::{AIExecutionProfile, CloudAIExecutionProfileModel};
use crate::ai::facts::{AIFact, CloudAIFactModel};
#[cfg(not(target_family = "wasm"))]
use crate::ai::mcp::templatable::{CloudTemplatableMCPServerModel, TemplatableMCPServer};
use crate::auth::AuthStateProvider;
use crate::auth::auth_manager::AuthManager;
use crate::cloud_object::folders::CloudFolderModel;
use crate::cloud_object::model::actions::{ObjectActionType, ObjectActions};
use crate::cloud_object::model::generic_string_model::{
    GenericStringModel, GenericStringObjectId, Serializer, StringModel,
};
use crate::cloud_object::model::persistence::{CloudModel, CloudModelEvent, UpdateSource};
use crate::cloud_object::{
    CloudModelType, CloudObject, CloudObjectLocation, GenericCloudObject,
    GenericStringObjectFormat, JsonObjectType, ObjectIdType, Owner, Space,
};
use crate::drive::CloudObjectTypeAndId;
use crate::drive::drive_helpers::{
    is_feature_gated_anonymous_user_past_env_var_limit,
    is_feature_gated_anonymous_user_past_notebook_limit,
    is_feature_gated_anonymous_user_past_workflow_limit,
};
use crate::env_vars::{CloudEnvVarCollectionModel, EnvVarCollection};
use crate::notebooks::{CloudNotebookModel, NotebookId};
use crate::persistence::ModelEvent;
use crate::server::ids::{ClientId, HashableId, ObjectUid, ServerId, SyncId, ToServerId};
use crate::workflows::workflow::Workflow;
use crate::workflows::workflow_enum::{CloudWorkflowEnumModel, WorkflowEnum};
use crate::workflows::{CloudWorkflowModel, WorkflowId};
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::workspaces::workspace::WorkspaceUid;

lazy_static! {
    /// For online-only operations, we want to quickly determine if the operation can succeed,
    /// so that if it can't, we can put the user back into the known good state.
    /// So we try 3 times to prevent any transient failures.
    static ref ONLINE_ONLY_OPERATION_RETRY_STRATEGY: RetryOption =
        RetryOption::exponential(Duration::from_millis(500) /* interval */, 2. /* exponential factor */, 3 /* max retry count */);

    static ref DUPLICATE_OBJECT_NAME_REGEX: Regex = Regex::new(r" \((\d+)\)$").expect("regex should not fail to compile");

}

#[derive(Debug, PartialEq)]
pub enum OperationSuccessType {
    Success,
    Rejection,
}

#[derive(Debug, PartialEq)]
pub enum ObjectOperation {
    Update,
    MoveToFolder,
    MoveToDrive,
    Trash,
    Untrash,
    Delete { initiated_by: InitiatedBy },
    EmptyTrash,
}

#[derive(Debug)]
pub struct ObjectOperationResult {
    pub success_type: OperationSuccessType,
    pub operation: ObjectOperation,
    pub client_id: Option<ClientId>,
    pub server_id: Option<ServerId>,
    pub num_objects: Option<i32>, // counts number of objects (including descendants) deleted for permadeletion
}

#[derive(Debug)]
pub enum UpdateManagerEvent {
    ObjectOperationComplete { result: ObjectOperationResult },
}

/// An enum that defines whether the action was initiated by the user or the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitiatedBy {
    User,
    System,
}

#[derive(Debug)]
pub struct GenericStringObjectInput<T, S>
where
    T: StringModel<
            CloudObjectType = GenericCloudObject<GenericStringObjectId, GenericStringModel<T, S>>,
        > + 'static,
    S: Serializer<T> + 'static,
{
    pub id: ClientId,
    pub model: GenericStringModel<T, S>,
    pub initial_folder_id: Option<SyncId>,
}

/// The UpdateManager is responsible for delegating work when there is an
/// update to an object (e.g. via a user interaction): it writes to SQLite and
/// updates the CloudModel, the in-memory state used by the object views.
pub struct UpdateManager {
    model_event_sender: Option<SyncSender<ModelEvent>>,
}

impl UpdateManager {
    pub fn new(model_event_sender: Option<SyncSender<ModelEvent>>) -> Self {
        Self { model_event_sender }
    }

    /// Constructs an UpdateManager for tests.
    #[cfg(test)]
    pub fn mock() -> Self {
        Self::new(None)
    }

    fn save_to_db(&self, events: impl IntoIterator<Item = ModelEvent>) {
        let model_event_sender = self.model_event_sender.clone();
        if let Some(model_event_sender) = &model_event_sender {
            for event in events {
                if let Err(e) = model_event_sender.send(event) {
                    report_error!(anyhow::Error::new(e).context("Error saving to database"));
                }
            }
        }
    }

    /// Persists the user's current-workspace selection to SQLite.
    pub fn persist_current_workspace(&self, workspace_uid: WorkspaceUid) {
        self.save_to_db([ModelEvent::SetCurrentWorkspace { workspace_uid }]);
    }

    fn save_in_memory_object_to_sqlite(&mut self, cloud_model: &CloudModel, uid: &ObjectUid) {
        if let Some(cloud_object) = cloud_model.get_by_uid(uid) {
            self.save_to_db([cloud_object.upsert_event()]);
        }
    }

    fn save_in_memory_object_metadata_to_sqlite(
        &mut self,
        cloud_model: &CloudModel,
        uid: &ObjectUid,
        hashed_sqlite_id: &str,
    ) {
        if let Some(cloud_object) = cloud_model.get_by_uid(uid) {
            let metadata = cloud_object.metadata().clone();
            let event = ModelEvent::UpdateObjectMetadata {
                id: hashed_sqlite_id.to_string(),
                metadata,
            };
            self.save_to_db([event]);
        }
    }

    pub fn update_ai_fact(
        &mut self,
        ai_fact: AIFact,
        ai_fact_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(CloudAIFactModel::new(ai_fact), ai_fact_id, ctx);
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn update_templatable_mcp_server(
        &mut self,
        templatable_mcp_server: TemplatableMCPServer,
        templatable_mcp_server_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudTemplatableMCPServerModel::new(templatable_mcp_server),
            templatable_mcp_server_id,
            ctx,
        );
    }

    pub fn update_workflow(
        &mut self,
        workflow: Workflow,
        workflow_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(CloudWorkflowModel::new(workflow), workflow_id, ctx);
    }

    pub fn update_workflow_enum(
        &mut self,
        workflow_enum: WorkflowEnum,
        workflow_enum_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudWorkflowEnumModel::new(workflow_enum),
            workflow_enum_id,
            ctx,
        );
    }

    pub fn update_env_var_collection(
        &mut self,
        env_var_collection: EnvVarCollection,
        env_var_collection_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudEnvVarCollectionModel::new(env_var_collection),
            env_var_collection_id,
            ctx,
        );
    }

    pub fn update_notebook_data(
        &mut self,
        data: Arc<String>,
        notebook_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        let cloud_model = CloudModel::as_ref(ctx);
        if let Some(notebook) = cloud_model.get_notebook(&notebook_id) {
            let new_notebook = CloudNotebookModel {
                title: notebook.model().title.to_owned(),
                data: data.to_string(),
                ai_document_id: notebook.model().ai_document_id,
                conversation_id: notebook.model().conversation_id.clone(),
            };
            self.update_object(new_notebook, notebook_id, ctx);
        } else {
            log::warn!("Expected notebook to be in model with id {notebook_id:?}");
        }
    }

    pub fn update_notebook_title(
        &mut self,
        title: Arc<String>,
        notebook_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        let cloud_model = CloudModel::as_ref(ctx);
        if let Some(notebook) = cloud_model.get_notebook(&notebook_id) {
            let new_notebook = CloudNotebookModel {
                title: title.to_string(),
                data: notebook.model().data.to_owned(),
                ai_document_id: notebook.model().ai_document_id,
                conversation_id: notebook.model().conversation_id.clone(),
            };
            self.update_object(new_notebook, notebook_id, ctx);
        } else {
            log::warn!("Expected notebook to be in model with id {notebook_id:?}");
        }
    }

    pub fn move_object_to_location(
        &mut self,
        object_id: CloudObjectTypeAndId,
        new_location: CloudObjectLocation,
        ctx: &mut ModelContext<Self>,
    ) {
        let uid = object_id.uid();
        let Some(object_current_owner) = CloudModel::handle(ctx).read(ctx, |model, _| {
            model
                .get_by_uid(&uid)
                .map(|object| object.permissions().owner)
        }) else {
            return;
        };

        // Apply the move to the in-memory model, persist it, and report success.
        // `update_object_location` emits `CloudModelEvent::ObjectMoved`.
        let operation;
        match new_location {
            // Moving into the trash is really a trash operation.
            CloudObjectLocation::Trash => return self.trash_object(object_id, ctx),
            CloudObjectLocation::Space(destination_space) => {
                match UserWorkspaces::as_ref(ctx).space_to_owner(destination_space, ctx) {
                    Some(destination_owner) if destination_owner != object_current_owner => {
                        CloudModel::handle(ctx).update(ctx, |model, ctx| {
                            model.update_object_location(&uid, Some(destination_owner), None, ctx);
                        });
                        operation = ObjectOperation::MoveToDrive;
                    }
                    Some(_) => {
                        // The space is staying the same, so this is a move to its root.
                        CloudModel::handle(ctx).update(ctx, |model, ctx| {
                            model.update_object_location(&uid, None, None, ctx);
                        });
                        operation = ObjectOperation::MoveToFolder;
                    }
                    None => {
                        // We couldn't map the space to a valid owner (most likely, it's the
                        // "shared" space).
                        return;
                    }
                }
            }
            CloudObjectLocation::Folder(folder_id) => {
                CloudModel::handle(ctx).update(ctx, |model, ctx| {
                    model.update_object_location(&uid, None, Some(folder_id), ctx);
                });
                operation = ObjectOperation::MoveToFolder;
            }
        }

        // Persist changes in sqlite.
        CloudModel::handle(ctx).update(ctx, |cloud_model, _| {
            self.save_in_memory_object_to_sqlite(cloud_model, &uid);
        });

        ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
            result: ObjectOperationResult {
                success_type: OperationSuccessType::Success,
                operation,
                client_id: None,
                server_id: None,
                num_objects: None,
            },
        });
        ctx.notify();
    }

    pub fn duplicate_object(
        &mut self,
        cloud_object_type_and_id: &CloudObjectTypeAndId,
        ctx: &mut ModelContext<Self>,
    ) {
        match cloud_object_type_and_id {
            CloudObjectTypeAndId::Notebook(notebook_id) => {
                self.duplicate_object_internal::<NotebookId, CloudNotebookModel>(notebook_id, ctx);
            }
            CloudObjectTypeAndId::Workflow(workflow_id) => {
                self.duplicate_object_internal::<WorkflowId, CloudWorkflowModel>(workflow_id, ctx);
            }
            CloudObjectTypeAndId::GenericStringObject { object_type, id } => {
                if let GenericStringObjectFormat::Json(JsonObjectType::EnvVarCollection) =
                    object_type
                {
                    self.duplicate_object_internal::<GenericStringObjectId, CloudEnvVarCollectionModel>(
                        id, ctx,
                    );
                } else {
                    report_error!("Tried to duplicate an unsupported type: json object");
                    debug_assert!(false, "Tried to duplicate an unsupported type: json object");
                }
            }
            CloudObjectTypeAndId::Folder(_) => {
                // Duplicating folders not currently supported.
                report_error!("Tried to duplicate an unsupported type: folder");
                debug_assert!(false, "Tried to duplicate an unsupported type: folder");
            }
        }
    }

    fn duplicate_object_internal<K, M>(&mut self, id: &SyncId, ctx: &mut ModelContext<Self>)
    where
        K: HashableId
            + ToServerId
            + std::fmt::Debug
            + Into<String>
            + Clone
            + Copy
            + Send
            + Sync
            + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        let (duplicate_model, client_id, owner, initial_folder_id) = {
            let cloud_model = CloudModel::as_ref(ctx);
            let object: GenericCloudObject<K, M> = cloud_model
                .get_object_of_type(id)
                .expect("object should exist in order to be duplicated")
                .clone();
            let client_id = ClientId::new();
            let owner = object.permissions.owner;
            let initial_folder_id = object.metadata.folder_id;
            let mut duplicate_model = object.model().clone();
            let duplicate_name =
                self.get_next_duplicate_object_name(&object as &dyn CloudObject, cloud_model, ctx);
            duplicate_model.set_display_name(&duplicate_name);
            (duplicate_model, client_id, owner, initial_folder_id)
        };
        self.create_object(
            duplicate_model,
            owner,
            client_id,
            true,
            initial_folder_id,
            ctx,
        );
    }

    pub fn create_ai_fact(
        &mut self,
        ai_fact: AIFact,
        client_id: ClientId,
        owner: Owner,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            CloudAIFactModel::new(ai_fact),
            owner,
            client_id,
            false,
            None,
            ctx,
        );
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn create_templatable_mcp_server(
        &mut self,
        templatable_mcp_server: TemplatableMCPServer,
        client_id: ClientId,
        owner: Owner,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            CloudTemplatableMCPServerModel::new(templatable_mcp_server),
            owner,
            client_id,
            false,
            None,
            ctx,
        );
    }

    #[allow(dead_code)]
    pub fn update_ai_execution_profile(
        &mut self,
        ai_execution_profile: AIExecutionProfile,
        ai_execution_profile_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudAIExecutionProfileModel::new(ai_execution_profile),
            ai_execution_profile_id,
            ctx,
        );
    }

    pub fn delete_ai_execution_profile(
        &mut self,
        ai_execution_profile_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.delete_object_by_user(
            CloudObjectTypeAndId::GenericStringObject {
                object_type: GenericStringObjectFormat::Json(JsonObjectType::AIExecutionProfile),
                id: ai_execution_profile_id,
            },
            ctx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_notebook(
        &mut self,
        client_id: ClientId,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        model: CloudNotebookModel,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let count = CloudModel::handle(ctx).read(ctx, |model, ctx| {
            model
                .active_non_welcome_notebooks_in_space(Space::Personal, ctx)
                .count()
        });
        if AuthStateProvider::handle(ctx).read(ctx, |auth_state_provider, _ctx| {
            is_feature_gated_anonymous_user_past_notebook_limit(
                auth_state_provider.get(),
                count + 1,
            )
        }) {
            AuthManager::handle(ctx).update(ctx, |auth_manager: &mut AuthManager, ctx| {
                auth_manager.anonymous_user_hit_drive_object_limit(ctx);
            });
            return;
        };

        self.create_object(
            model,
            owner,
            client_id,
            force_expand,
            initial_folder_id,
            ctx,
        );
    }

    fn get_next_duplicate_object_name(
        &self,
        original_cloud_object: &dyn CloudObject,
        cloud_model: &CloudModel,
        app: &AppContext,
    ) -> String {
        let original_name = original_cloud_object.display_name();

        // Iterate through items in the same folder as the original object that are of the
        // same type, and populate a hashset with those names.
        let same_type_and_folder_names = cloud_model
            .active_cloud_objects_in_location_without_descendents(
                original_cloud_object.location(cloud_model, app),
                app,
            )
            .filter(|&object| object.object_type() == original_cloud_object.object_type())
            .map(|object| object.display_name())
            .collect::<HashSet<String>>();

        // Start with "{original_object_name} ({original_object_name's count + 1})".
        // Keep incrementing by one if there already exists an object of the same type in
        // the same folder (using the hashset generated above).
        let mut duplicate_name = get_duplicate_object_name(&original_name);
        while same_type_and_folder_names.contains(&duplicate_name) {
            duplicate_name = get_duplicate_object_name(&duplicate_name);
        }
        duplicate_name
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_workflow(
        &mut self,
        workflow: Workflow,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        client_id: ClientId,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let count = CloudModel::handle(ctx).read(ctx, |model, ctx| {
            model
                .active_non_welcome_workflows_in_space(Space::Personal, ctx)
                .count()
        });
        if AuthStateProvider::handle(ctx).read(ctx, |auth_state_provider, _ctx| {
            is_feature_gated_anonymous_user_past_workflow_limit(
                auth_state_provider.get(),
                count + 1,
            )
        }) {
            AuthManager::handle(ctx).update(ctx, |auth_manager: &mut AuthManager, ctx| {
                auth_manager.anonymous_user_hit_drive_object_limit(ctx);
            });
            return;
        };

        self.create_object(
            CloudWorkflowModel::new(workflow),
            owner,
            client_id,
            force_expand,
            initial_folder_id,
            ctx,
        );
    }

    pub fn create_env_var_collection(
        &mut self,
        client_id: ClientId,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        model: CloudEnvVarCollectionModel,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let count = CloudModel::handle(ctx).read(ctx, |model, ctx| {
            model
                .active_non_welcome_env_var_collections_in_space(Space::Personal, ctx)
                .count()
        });
        if AuthStateProvider::handle(ctx).read(ctx, |auth_state_provider, _ctx| {
            is_feature_gated_anonymous_user_past_env_var_limit(auth_state_provider.get(), count + 1)
        }) {
            AuthManager::handle(ctx).update(ctx, |auth_manager: &mut AuthManager, ctx| {
                auth_manager.anonymous_user_hit_drive_object_limit(ctx);
            });
            return;
        };

        self.create_object(
            model,
            owner,
            client_id,
            force_expand,
            initial_folder_id,
            ctx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_folder(
        &mut self,
        name: String,
        owner: Owner,
        client_id: ClientId,
        initial_folder_id: Option<SyncId>,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            // TODO(INT-789): support creating folders as warp packs
            CloudFolderModel::new(&name, false),
            owner,
            client_id,
            force_expand,
            initial_folder_id,
            ctx,
        );
    }

    /// Bulk creates a list of generic string objects, all in a single
    /// sqllite write and server api call.  More efficient than calling
    /// create_object for each object.
    ///
    /// Note that if the bulk creation request fails, the client will end up retrying
    /// object creation one write and request at a time.
    pub fn create_object<K, M>(
        &mut self,
        model: M,
        owner: Owner,
        client_id: ClientId,
        force_expand: bool,
        initial_folder_id: Option<SyncId>,
        ctx: &mut ModelContext<Self>,
    ) where
        K: HashableId
            + ToServerId
            + std::fmt::Debug
            + Into<String>
            + Clone
            + Copy
            + Send
            + Sync
            + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        let object_id = SyncId::ClientId(client_id);
        let auth_state = AuthStateProvider::as_ref(ctx).get();
        let initial_editor = auth_state.user_id();

        // Update in-memory model.
        CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            let mut object = GenericCloudObject::<K, M>::new_local(
                model.clone(),
                owner,
                initial_folder_id,
                client_id,
            );
            object.metadata.current_editor_uid = initial_editor.map(|uid| uid.as_string());
            cloud_model.create_object(object_id, object, ctx);

            if force_expand {
                cloud_model.force_expand_object_and_ancestors(object_id, ctx);
            }
        });

        // Update sqlite.
        let cloud_model = CloudModel::as_ref(ctx);
        if let Some(object) = cloud_model.get_object_of_type::<K, M>(&object_id) {
            self.save_to_db([object.upsert_event()]);
        }
    }

    pub fn update_object<K, M>(&mut self, model: M, object_id: SyncId, ctx: &mut ModelContext<Self>)
    where
        K: HashableId
            + ToServerId
            + std::fmt::Debug
            + Into<String>
            + Clone
            + Copy
            + Send
            + Sync
            + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        // Update in-memory model.
        CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            cloud_model.update_object_from_edit(model.clone(), object_id, ctx);
            ctx.notify();
        });

        // Update sqlite.
        let cloud_model = CloudModel::as_ref(ctx);
        if let Some(object) = cloud_model.get_object_of_type::<K, M>(&object_id) {
            self.save_to_db([object.upsert_event()]);
        };
    }

    // Takes a generic SyncId and records the action.
    pub fn record_object_action(
        &mut self,
        id_and_type: CloudObjectTypeAndId,
        action_type: ObjectActionType,
        data: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) {
        // Take the action timestamp from the client.
        let action_timestamp = Utc::now();

        // Update in-memory model.
        let object_action = ObjectActions::handle(ctx).update(ctx, |object_actions_model, ctx| {
            object_actions_model.insert_action(
                id_and_type.uid(),
                id_and_type.sqlite_uid_hash(),
                action_type.clone(),
                data.clone(),
                action_timestamp,
                ctx,
            )
        });

        // Update sqlite.
        self.save_to_db([ModelEvent::InsertObjectAction { object_action }]);
    }

    fn mark_object_trashed_and_return_timestamps(
        &self,
        uid: &ObjectUid,
        ctx: &mut ModelContext<Self>,
    ) -> (Option<ServerTimestamp>, Option<ServerTimestamp>) {
        let timestamp = ServerTimestamp::new(Utc::now());
        CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            if let Some(object) = cloud_model.get_mut_by_uid(uid) {
                // Here, we write a timestamp to the trashed_ts field. The client will eventually update to
                // the canonical version of the timestamp once it receives an rtc message from the server.

                object.metadata_mut().trashed_ts = Some(timestamp);
                object
                    .metadata_mut()
                    .pending_changes_statuses
                    .has_pending_metadata_change = true;
                ctx.emit(CloudModelEvent::ObjectTrashed {
                    type_and_id: object.cloud_object_type_and_id(),
                    source: UpdateSource::Local,
                });
                ctx.notify();
                (
                    object.metadata().metadata_last_updated_ts,
                    object.metadata().trashed_ts,
                )
            } else {
                (None, None)
            }
        })
    }

    pub fn trash_object(&mut self, id: CloudObjectTypeAndId, ctx: &mut ModelContext<Self>) {
        // If the object isn't known to the server yet, we can't trash it.
        let Some(server_id) = id.server_id() else {
            return;
        };

        let hashed_id = id.uid();
        // If there's a pending online-only operation for this object, don't trash it.
        let Some(has_pending_online_only_operation) =
            CloudModel::handle(ctx).read(ctx, |model, _| {
                model
                    .get_by_uid(&hashed_id)
                    .map(|object| object.metadata().has_pending_online_only_change())
            })
        else {
            return;
        };

        if has_pending_online_only_operation {
            return;
        }

        self.mark_object_trashed_and_return_timestamps(&hashed_id, ctx);

        // Persist the metadata change in sqlite.
        CloudModel::handle(ctx).update(ctx, |cloud_model, _| {
            if let Some(object) = cloud_model.get_mut_by_uid(&hashed_id) {
                object
                    .metadata_mut()
                    .pending_changes_statuses
                    .has_pending_metadata_change = false;
            }

            let hashed_sqlite_id = server_id.sqlite_type_and_uid_hash(id.object_id_type());
            self.save_in_memory_object_metadata_to_sqlite(
                cloud_model,
                &hashed_id,
                &hashed_sqlite_id,
            );
        });

        ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
            result: ObjectOperationResult {
                success_type: OperationSuccessType::Success,
                operation: ObjectOperation::Trash,
                client_id: None,
                server_id: Some(ServerId::from_string_lossy(&hashed_id)),
                num_objects: None,
            },
        });
        ctx.notify();
    }

    pub fn untrash_object(&mut self, id: CloudObjectTypeAndId, ctx: &mut ModelContext<Self>) {
        // If the object isn't known to the server yet, we can't untrash it.
        let Some(server_id) = id.server_id() else {
            return;
        };

        let hashed_id = id.uid();
        // If there's a pending online-only operation for this object, don't untrash it.
        let Some(has_pending_online_only_operation) =
            CloudModel::handle(ctx).read(ctx, |model, _| {
                model
                    .get_by_uid(&hashed_id)
                    .map(|object| object.metadata().has_pending_online_only_change())
            })
        else {
            return;
        };

        if has_pending_online_only_operation {
            return;
        }

        // Clear the trash timestamp and persist the metadata change in sqlite.
        CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            if let Some(object) = cloud_model.get_mut_by_uid(&hashed_id) {
                object.metadata_mut().trashed_ts = None;
                object
                    .metadata_mut()
                    .pending_changes_statuses
                    .pending_untrash = false;

                let hashed_sqlite_id = server_id.sqlite_type_and_uid_hash(id.object_id_type());
                let type_and_id = object.cloud_object_type_and_id();
                self.save_in_memory_object_metadata_to_sqlite(
                    cloud_model,
                    &hashed_id,
                    &hashed_sqlite_id,
                );

                ctx.emit(CloudModelEvent::ObjectUntrashed {
                    type_and_id,
                    source: UpdateSource::Local,
                });
                ctx.notify();
            }
        });

        ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
            result: ObjectOperationResult {
                success_type: OperationSuccessType::Success,
                operation: ObjectOperation::Untrash,
                client_id: None,
                server_id: Some(ServerId::from_string_lossy(&hashed_id)),
                num_objects: None,
            },
        });
        ctx.notify();
    }

    pub fn delete_object_by_user(
        &mut self,
        id: CloudObjectTypeAndId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.delete_object_with_initiated_by(id, InitiatedBy::User, ctx);
    }

    pub fn delete_object_with_initiated_by(
        &mut self,
        id: CloudObjectTypeAndId,
        initiated_by: InitiatedBy,
        ctx: &mut ModelContext<Self>,
    ) {
        // If the object isn't known to the server yet, we can't delete it.
        let Some(server_id) = id.server_id() else {
            return;
        };

        let uid = id.uid();
        // If there's a pending online-only operation or delete for this object, don't delete it.
        let Some((has_pending_online_only_operation, has_pending_delete)) = CloudModel::handle(ctx)
            .read(ctx, |model, _| {
                model.get_by_uid(&uid).map(|object| {
                    (
                        object.metadata().has_pending_online_only_change(),
                        object.metadata().pending_changes_statuses.pending_delete,
                    )
                })
            })
        else {
            return;
        };

        if has_pending_online_only_operation || has_pending_delete {
            return;
        }

        let num_deleted_objects =
            self.on_object_delete_success(vec![SyncId::ServerId(server_id)], ctx);
        ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
            result: ObjectOperationResult {
                success_type: OperationSuccessType::Success,
                operation: ObjectOperation::Delete { initiated_by },
                client_id: None,
                server_id: Some(ServerId::from_string_lossy(&uid)),
                num_objects: Some(num_deleted_objects),
            },
        });
        ctx.notify();
    }

    pub fn empty_trash(&mut self, space: Space, ctx: &mut ModelContext<Self>) {
        let Some(owner) = UserWorkspaces::as_ref(ctx).space_to_owner(space, ctx) else {
            // TODO: For the Shared space, this should delete every object that's shared with the user
            // and trashed.
            log::warn!("Tried to empty trash in unsupported space {space:?}");
            return;
        };

        let trashed_ids: Vec<SyncId> = CloudModel::handle(ctx).read(ctx, |model, _| {
            model
                .get_all_exportable_object_ids()
                .into_iter()
                .filter_map(|type_and_id| {
                    let object = model.get_by_uid(&type_and_id.uid())?;
                    let _is_trashed_in_space = object.metadata().trashed_ts.is_some()
                        && object.permissions().owner == owner;
                    type_and_id
                        .server_id()
                        .filter(|_| {
                            object.metadata().trashed_ts.is_some()
                                && object.permissions().owner == owner
                        })
                        .map(SyncId::ServerId)
                })
                .collect()
        });

        let num_deleted_objects = self.on_object_delete_success(trashed_ids, ctx);
        let (success_type, num_objects) = if num_deleted_objects == 0 {
            // Rejection toast: there are no objects in the Trash.
            (OperationSuccessType::Rejection, Some(0))
        } else {
            (OperationSuccessType::Success, Some(num_deleted_objects))
        };
        ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
            result: ObjectOperationResult {
                success_type,
                operation: ObjectOperation::EmptyTrash,
                client_id: None,
                server_id: None,
                num_objects,
            },
        });
        ctx.notify();
    }

    pub fn on_object_delete_success(
        &mut self,
        deleted_ids: Vec<SyncId>,
        ctx: &mut ModelContext<'_, UpdateManager>,
    ) -> i32 {
        let cloud_model_handle = CloudModel::handle(ctx);
        let all_object_uids: Vec<ObjectUid> = deleted_ids.iter().map(|&id| id.uid()).collect();

        // This variable counts the number of objects deleted client-side in each Empty Trash action,
        // because the server returns everything in the db, including objects that have already been marked for deletion
        let mut num_deleted_objects = 0;
        let mut sync_ids_and_types: Vec<(SyncId, ObjectIdType)> = Vec::new();
        cloud_model_handle.update(ctx, |cloud_model, ctx| {
            (sync_ids_and_types, num_deleted_objects) =
                cloud_model.delete_objects_by_id(all_object_uids.clone(), ctx);
        });

        // Deleted the actions associated with these objects too.
        ObjectActions::handle(ctx).update(ctx, |object_actions, ctx| {
            for uid in all_object_uids.clone() {
                object_actions.delete_actions_for_object(&uid, ctx);
            }
        });

        // Return early if empty
        if num_deleted_objects == 0 {
            return num_deleted_objects;
        }

        // Delete objects from sqlite. This will also delete their actions.
        self.save_to_db([ModelEvent::DeleteObjects {
            ids: sync_ids_and_types,
        }]);

        num_deleted_objects
    }

    pub fn rename_folder(
        &mut self,
        folder_id: SyncId,
        new_name: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let cloud_model = CloudModel::as_ref(ctx);
        if let Some(folder) = cloud_model.get_folder(&folder_id) {
            let new_folder = CloudFolderModel {
                name: new_name,
                is_open: folder.model().is_open,
                is_warp_pack: folder.model().is_warp_pack,
            };
            self.update_object(new_folder, folder_id, ctx);
        } else {
            log::warn!("Attempted to rename folder that doesn't exist with id: {folder_id:?}");
        }
    }
}

/// Return the newly duplicated object's name based on the original object's name. E.g.:
/// - "my object name" -> "my object name (1)"
pub fn get_duplicate_object_name(original_name: &str) -> String {
    match DUPLICATE_OBJECT_NAME_REGEX
        .captures(original_name)
        .and_then(|caps| caps.get(1))
        .and_then(|num| num.as_str().parse::<usize>().ok())
    {
        Some(num) => {
            let new_num = num.saturating_add(1);

            // edge case check for when the duplicate number is usize::MAX
            if new_num == usize::MAX {
                format!("{original_name} (1)")
            } else {
                DUPLICATE_OBJECT_NAME_REGEX
                    .replace(original_name, format!(" ({new_num})"))
                    .to_string()
            }
        }
        None => format!("{original_name} (1)"),
    }
}

impl Entity for UpdateManager {
    type Event = UpdateManagerEvent;
}

impl SingletonEntity for UpdateManager {}
