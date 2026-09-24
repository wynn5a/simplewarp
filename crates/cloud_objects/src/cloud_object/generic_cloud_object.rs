use std::marker::PhantomData;
use std::sync::Arc;

use super::{
    CloudObjectMetadata, CloudObjectPermissions, CloudObjectStatuses, CloudObjectSyncStatus,
    NumInFlightRequests, ObjectType, Owner,
};
use crate::ids::{ClientId, SyncId};

#[derive(Clone, Debug, Default)]
pub enum ConflictStatus<T> {
    #[default]
    NoConflicts,
    ConflictingChanges {
        object: Arc<T>,
    },
}

impl<T> ConflictStatus<T> {
    /// Utility function that allows for a more ergonomic way of figuring out whether there is a
    /// conflict (for cases where we don't care about the conflict details).
    pub fn has_conflicts(&self) -> bool {
        matches!(self, ConflictStatus::ConflictingChanges { .. })
    }
}

/// A portable payload for persisting or otherwise upserting a cloud object without app-local event types.
#[derive(Clone, Debug)]
pub struct CloudObjectUpsertParams<M> {
    pub id: SyncId,
    pub object_type: ObjectType,
    pub metadata: CloudObjectMetadata,
    pub permissions: CloudObjectPermissions,
    pub model: M,
}

/// A generic implementation of cloud objects that can be used for any model and id types.
///
/// For instance, rather than directly implementing the CloudObject trait, CloudObjects can
/// implement GenericCloudObject<K, M> where K is their id type and M is their model type.
///
/// For example, CloudNotebook becomes:
///
///   pub type CloudNotebook = GenericCloudObject<NotebookId, CloudNotebookModel>
///
/// The advantage of using the generic model is you get common implementations
/// of CloudObject methods like ```versions``` for free.
///
/// See the comments for CloudObject to understand the relationship between
/// this trait, CloudObject and CloudModelType.  They are tightly coupled.
#[derive(Clone, Debug)]
pub struct GenericCloudObject<K, M> {
    pub id: SyncId,
    pub metadata: CloudObjectMetadata,
    pub permissions: CloudObjectPermissions,
    /// Tracks whether this object has a conflict with the server version.
    /// This is runtime state (not persisted) - conflicts are always NoConflicts when loaded from SQLite.
    pub conflict_status: ConflictStatus<Self>,

    // Intentionally not public to prevent users of this class from holding
    // onto references to the model outside of this struct.
    //
    // This is an Arc in order to support clone-on-write semantics for the model.
    // By wrapping the model in an Arc, clones become cheap, and we can avoid
    // doing deep clones of the model whenever the containing object is cloned.
    //
    // Callers who want to update the model need to call set_model to update the
    // entire model atomically.
    model: Arc<M>,
    /// Keeps `K` well-formed now that the id type only appears (via `Self`) in the
    /// conflict snapshot, which is only ever held behind an `Arc`.
    _marker: PhantomData<fn() -> K>,
}

impl<K, M> PartialEq for GenericCloudObject<K, M>
where
    M: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.model() == other.model()
    }
}

impl<K, M> GenericCloudObject<K, M> {
    /// Gets a reference to the model held by the object.
    pub fn model(&self) -> &M {
        &self.model
    }

    /// Returns a shared handle to the model.
    pub fn shared_model(&self) -> Arc<M> {
        self.model.clone()
    }

    /// Sets a new version of the model on the object, replacing the old version.
    pub fn set_model(&mut self, model: M) {
        self.model = model.into();
    }

    /// Constructs a new instance of this model with the given id, model, metadata and permissions.
    pub fn new(
        id: SyncId,
        model: M,
        metadata: CloudObjectMetadata,
        permissions: CloudObjectPermissions,
    ) -> Self {
        Self {
            id,
            model: model.into(),
            metadata,
            permissions,
            conflict_status: ConflictStatus::NoConflicts,
            _marker: PhantomData,
        }
    }

    /// Creates a new GenericCloudObject with the given model, owner, and initial folder id.
    /// This is for the local creation flow, as opposed to creating from a server update.
    pub fn new_local(
        model: M,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        client_id: ClientId,
    ) -> Self {
        Self {
            id: SyncId::ClientId(client_id),
            model: model.into(),
            metadata: CloudObjectMetadata {
                pending_changes_statuses: CloudObjectStatuses {
                    content_sync_status: CloudObjectSyncStatus::InFlight(NumInFlightRequests(1)),
                    has_pending_metadata_change: false,
                    has_pending_permissions_change: false,
                    pending_untrash: false,
                    pending_delete: false,
                },
                folder_id: initial_folder_id,
                revision: Default::default(),
                metadata_last_updated_ts: Default::default(),
                current_editor_uid: Default::default(),
                trashed_ts: Default::default(),
                // Objects created from the client are never welcome objects.
                is_welcome_object: false,
                creator_uid: None,
                last_editor_uid: None,
                last_task_run_ts: None,
            },
            permissions: CloudObjectPermissions {
                owner,
                anyone_with_link: None,
                guests: Default::default(),
                permissions_last_updated_ts: None,
            },
            conflict_status: ConflictStatus::NoConflicts,
            _marker: PhantomData,
        }
    }

    /// Returns portable upsert parameters for this object.
    pub fn upsert_params(&self, object_type: ObjectType) -> CloudObjectUpsertParams<M>
    where
        M: Clone,
    {
        CloudObjectUpsertParams {
            id: self.id,
            object_type,
            metadata: self.metadata.clone(),
            permissions: self.permissions.clone(),
            model: self.model().clone(),
        }
    }
}

impl<K, M> From<CloudObjectUpsertParams<M>> for GenericCloudObject<K, M> {
    fn from(params: CloudObjectUpsertParams<M>) -> Self {
        Self::new(params.id, params.model, params.metadata, params.permissions)
    }
}
