use std::collections::HashMap;

use anyhow::Result;
use async_channel::Sender;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
// #[cfg(any(test, feature = "test-util"))]
// pub use cloud_object_client::MockObjectClient;
use cloud_object_client::{
    GetCloudObjectResponse, InitialLoadResponse, ObjectActionHistory, ObjectActionType,
    ObjectDeleteResult, ObjectMetadataUpdateResult, ObjectPermissionUpdateResult,
    ObjectPermissionsUpdateData, ObjectUpdateMessage,
};
pub use cloud_object_client::{GuestIdentifier, ObjectClient};
use warp_graphql::object_permissions::AccessLevel;

use crate::cloud_object::folders::FolderId;
use crate::cloud_object::model::generic_string_model::GenericStringObjectId;
use crate::cloud_object::{
    BulkCreateCloudObjectResult, BulkCreateGenericStringObjectsRequest, CreateCloudObjectResult,
    CreateObjectRequest, GenericStringObjectFormat, GenericStringObjectUniqueKey, ObjectType,
    ObjectsToUpdate, Owner, Revision, ServerFolder, ServerMetadata, ServerNotebook, ServerObject,
    ServerPermissions, ServerWorkflow, UpdateCloudObjectResult,
};
use crate::notebooks::NotebookId;
use crate::server::ids::ServerId;
use crate::server::server_api::ServerApi;
use crate::server::sync_queue::SerializedModel;
use crate::sharing::SharingAccessLevel;
use crate::workflows::WorkflowId;

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl ObjectClient for ServerApi {
    async fn create_workflow(
        &self,
        _request: CreateObjectRequest,
    ) -> Result<CreateCloudObjectResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_workflow(
        &self,
        _workflow_id: WorkflowId,
        _data: SerializedModel,
        _revision: Option<Revision>,
    ) -> Result<UpdateCloudObjectResult<ServerWorkflow>> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn bulk_create_generic_string_objects(
        &self,
        _owner: Owner,
        _objects: &[BulkCreateGenericStringObjectsRequest],
    ) -> Result<BulkCreateCloudObjectResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn create_generic_string_object(
        &self,
        _format: GenericStringObjectFormat,
        _uniqueness_key: Option<GenericStringObjectUniqueKey>,
        _request: CreateObjectRequest,
    ) -> Result<CreateCloudObjectResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn create_notebook(
        &self,
        _request: CreateObjectRequest,
    ) -> Result<CreateCloudObjectResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_notebook(
        &self,
        _notebook_id: NotebookId,
        _title: Option<String>,
        _data: Option<SerializedModel>,
        _revision: Option<Revision>,
    ) -> Result<UpdateCloudObjectResult<ServerNotebook>> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn create_folder(
        &self,
        _request: CreateObjectRequest,
    ) -> Result<CreateCloudObjectResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_folder(
        &self,
        _folder_id: FolderId,
        _name: SerializedModel,
    ) -> Result<UpdateCloudObjectResult<ServerFolder>> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_generic_string_object(
        &self,
        _object_id: GenericStringObjectId,
        _model: SerializedModel,
        _revision: Option<Revision>,
    ) -> Result<UpdateCloudObjectResult<Box<dyn ServerObject>>> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn grab_notebook_edit_access(&self, _notebook_id: NotebookId) -> Result<ServerMetadata> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn give_up_notebook_edit_access(
        &self,
        _notebook_id: NotebookId,
    ) -> Result<ServerMetadata> {
        Err(crate::server::server_api::local_only_error())
    }

    /// Starts a websocket connections against the corresponding GraphQL subscription.
    /// Messages received over the socket are sent over the `message_sender`.
    /// Once the websocket is live, a one-shot message is sent over `stream_ready_sender`
    /// to indicate so. This is because this method only returns once the websocket is closed.
    async fn get_warp_drive_updates(
        &self,
        _message_sender: Sender<ObjectUpdateMessage>,
        _stream_ready_sender: Sender<()>,
    ) -> Result<()> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn fetch_changed_objects(
        &self,
        _objects_to_update: ObjectsToUpdate,
        _force_refresh: bool,
    ) -> Result<InitialLoadResponse> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn fetch_single_cloud_object(&self, _id: ServerId) -> Result<GetCloudObjectResponse> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn transfer_notebook_owner(
        &self,
        _notebook_id: NotebookId,
        _owner: Owner,
    ) -> Result<bool> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn transfer_workflow_owner(
        &self,
        _workflow_id: WorkflowId,
        _owner: Owner,
    ) -> Result<bool> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn transfer_generic_string_object_owner(
        &self,
        _gso_id: GenericStringObjectId,
        _owner: Owner,
    ) -> Result<bool> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn trash_object(&self, _id: ServerId) -> Result<bool> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn untrash_object(&self, _id: ServerId) -> Result<ObjectMetadataUpdateResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn delete_object(&self, _id: ServerId) -> Result<ObjectDeleteResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn empty_trash(&self, _owner: Owner) -> Result<ObjectDeleteResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn move_object(
        &self,
        _id: ServerId,
        _folder_id: Option<FolderId>,
        _owner: Owner,
        _object_type: ObjectType,
    ) -> Result<bool> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn record_object_action(
        &self,
        _id: ServerId,
        _action_type: ObjectActionType,
        _timestamp: DateTime<Utc>,
        _data: Option<String>,
    ) -> Result<ObjectActionHistory> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn leave_object(&self, _id: ServerId) -> Result<ObjectDeleteResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn set_object_link_permissions(
        &self,
        _object_id: ServerId,
        _access_level: SharingAccessLevel,
    ) -> Result<ObjectPermissionUpdateResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn remove_object_link_permissions(
        &self,
        _object_id: ServerId,
    ) -> Result<ObjectPermissionUpdateResult> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn add_object_guests(
        &self,
        _object_id: ServerId,
        _guest_emails: Vec<String>,
        _access_level: AccessLevel,
    ) -> Result<ObjectPermissionsUpdateData> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn update_object_guests(
        &self,
        _object_id: ServerId,
        _guest_emails: Vec<String>,
        _access_level: AccessLevel,
    ) -> Result<ServerPermissions> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn remove_object_guest(
        &self,
        _object_id: ServerId,
        _guest: GuestIdentifier,
    ) -> Result<ServerPermissions> {
        Err(crate::server::server_api::local_only_error())
    }

    async fn fetch_environment_last_task_run_timestamps(
        &self,
    ) -> Result<HashMap<String, DateTime<Utc>>> {
        Err(crate::server::server_api::local_only_error())
    }
}
