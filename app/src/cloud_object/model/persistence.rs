use std::collections::{HashMap, HashSet};
use std::sync::mpsc::SyncSender;

use chrono::{DateTime, Utc};
use warp_errors::report_error;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use crate::cloud_object::folders::{CloudFolder, CloudFolderModel};
use crate::cloud_object::{
    CloudModelType, CloudObject, CloudObjectLocation, CloudObjectTypeAndId, GenericCloudObject,
    ObjectIdType, Owner, Space,
};
use crate::env_vars::CloudEnvVarCollection;
use crate::notebooks::CloudNotebook;
use crate::persistence::ModelEvent;
use crate::server::ids::{HashableId, ObjectUid, SyncId, ToServerId};
use crate::workflows::CloudWorkflow;
use crate::workflows::workflow_enum::CloudWorkflowEnum;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudModelEvent {
    ObjectUpdated {
        type_and_id: CloudObjectTypeAndId,
    },
    ObjectTrashed {
        type_and_id: CloudObjectTypeAndId,
    },
    ObjectUntrashed {
        type_and_id: CloudObjectTypeAndId,
    },
    ObjectCreated {
        type_and_id: CloudObjectTypeAndId,
    },
    /// An object was permanently deleted.
    ObjectDeleted {
        type_and_id: CloudObjectTypeAndId,
        /// The parent folder of this object, since it's no longer in the model.
        folder_id: Option<SyncId>,
    },
    /// An object identified by `id` was force expanded.
    ObjectForceExpanded {
        id: String,
    },
    /// Environment last-task timestamps fetched outside the generic cloud-object sync were merged.
    EnvironmentLastTaskRunTimestampsUpdated,
}

/// Persistence model for [CloudObject] information. In an ideal world, this singleton model
/// is a 1:1 mapping for what we persist in sqlite. Any logic beyond a basic update
/// or query to data in [CloudModel] should instead be stored in [CloudViewModel] and tested in
/// model_test.rs.
pub struct CloudModel {
    objects_by_id: HashMap<ObjectUid, Box<dyn CloudObject>>,
    model_event_sender: Option<SyncSender<ModelEvent>>,
}

impl CloudModel {
    pub fn new(
        model_event_sender: Option<SyncSender<ModelEvent>>,
        cached_objects: Vec<Box<dyn CloudObject>>,
    ) -> Self {
        let objects_by_id = cached_objects
            .into_iter()
            .map(|object| (object.uid().to_owned(), object))
            .collect::<HashMap<ObjectUid, Box<dyn CloudObject>>>();

        Self {
            objects_by_id,
            model_event_sender,
        }
    }

    pub fn get_by_uid(&self, uid: &ObjectUid) -> Option<&dyn CloudObject> {
        self.objects_by_id.get(uid).map(|o| o.as_ref())
    }

    pub fn get_mut_by_uid(&mut self, uid: &ObjectUid) -> Option<&mut Box<dyn CloudObject>> {
        self.objects_by_id.get_mut(uid)
    }

    pub fn cloud_objects(&self) -> impl Iterator<Item = &Box<dyn CloudObject>> {
        self.objects_by_id.values()
    }

    pub fn create_object(
        &mut self,
        id: SyncId,
        object: impl CloudObject + 'static,
        ctx: &mut ModelContext<CloudModel>,
    ) {
        ctx.emit(CloudModelEvent::ObjectCreated {
            type_and_id: object.cloud_object_type_and_id(),
        });
        self.objects_by_id.insert(id.uid(), Box::new(object));
        ctx.notify();
    }

    pub fn delete_objects_by_id(
        &mut self,
        uids: Vec<ObjectUid>,
        ctx: &mut ModelContext<Self>,
    ) -> (Vec<(SyncId, ObjectIdType)>, i32) {
        let mut count = 0;
        let mut sync_ids_and_types: Vec<(SyncId, ObjectIdType)> = Vec::new();
        for uid in uids {
            if let Some(object) = self.objects_by_id.remove(&uid) {
                let cloud_object_type_and_id = object.cloud_object_type_and_id();
                sync_ids_and_types.push((
                    cloud_object_type_and_id.sync_id(),
                    cloud_object_type_and_id.object_id_type(),
                ));

                ctx.emit(CloudModelEvent::ObjectDeleted {
                    type_and_id: object.cloud_object_type_and_id(),
                    folder_id: object.metadata().folder_id,
                });
                count += 1;
            }
        }
        ctx.notify();
        (sync_ids_and_types, count)
    }

    /// Updates the per-environment "last used" timestamp.
    ///
    /// This timestamp is derived from `CloudEnvironment.lastTaskCreated.createdAt`.
    pub fn update_environment_last_task_run_timestamps(
        &mut self,
        timestamps: HashMap<String, DateTime<Utc>>,
        ctx: &mut ModelContext<Self>,
    ) {
        for (uid, timestamp) in timestamps {
            if let Some(object) = self.objects_by_id.get_mut(&uid) {
                object.metadata_mut().last_task_run_ts = Some(timestamp.into());
            }
        }
        ctx.emit(CloudModelEvent::EnvironmentLastTaskRunTimestampsUpdated);
        ctx.notify();
    }

    /// Update an object in the cloud model as part of a local user edit. This should not be used
    /// for updates received from the server.
    pub fn update_object_from_edit<K, M>(
        &mut self,
        model: M,
        object_id: SyncId,
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
        if let Some(cloud_object) = self.get_object_of_type_mut(&object_id) {
            cloud_object.set_model(model);
            ctx.emit(CloudModelEvent::ObjectUpdated {
                type_and_id: cloud_object.cloud_object_type_and_id(),
            });
            ctx.notify();
        }
    }

    fn open_folder_and_persist(&mut self, folder_id: SyncId, ctx: &mut ModelContext<Self>) {
        if let Some(folder) = self.get_folder_mut(&folder_id) {
            folder.set_model(CloudFolderModel {
                is_open: true,
                is_warp_pack: folder.model().is_warp_pack,
                name: folder.model().name.clone(),
            });

            let folder_clone = folder.clone();
            if let Some(model_event_sender) = &self.model_event_sender
                && let Err(e) = model_event_sender.send(folder_clone.upsert_event())
            {
                report_error!(anyhow::Error::new(e).context("Error persisting folder"));
            }

            ctx.notify();
        }
    }

    /// Force expands the object identified by `hash_id` and any of its ancestors. If an object is
    /// identified by `id`, [`CloudModelEvent::ObjectForceExpanded`] is emitted.
    pub fn force_expand_object_and_ancestors(&mut self, id: SyncId, ctx: &mut ModelContext<Self>) {
        let hashed_id = &id.uid();
        if !self.objects_by_id.contains_key(hashed_id) {
            return;
        }

        self.force_expand_object_and_ancestors_internal(id, ctx);
        ctx.emit(CloudModelEvent::ObjectForceExpanded {
            id: hashed_id.clone(),
        });
    }

    fn force_expand_object_and_ancestors_internal(
        &mut self,
        id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(object) = self.objects_by_id.get(&id.uid()) else {
            return;
        };

        let parent_folder_id = object.metadata().folder_id;
        let folder: Option<&CloudFolder> = object.into();

        if let Some(folder) = folder {
            self.open_folder_and_persist(folder.id, ctx);
        }

        if let Some(parent_folder_id) = parent_folder_id {
            self.force_expand_object_and_ancestors_internal(parent_folder_id, ctx);
        }
    }

    pub fn get_folder_by_uid(&self, uid: &str) -> Option<&CloudFolder> {
        self.objects_by_id.get(uid).and_then(|object| object.into())
    }

    pub fn get_folder(&self, folder_id: &SyncId) -> Option<&CloudFolder> {
        self.objects_by_id
            .get(&folder_id.uid())
            .and_then(|object| object.into())
    }

    pub fn get_folder_mut(&mut self, folder_id: &SyncId) -> Option<&mut CloudFolder> {
        self.objects_by_id
            .get_mut(&folder_id.uid())
            .and_then(|object| object.into())
    }

    /// Returns only active (not trashed) folders in cloud model.
    pub fn get_all_active_folders(&self) -> impl Iterator<Item = &CloudFolder> {
        self.objects_by_id
            .values()
            .filter(|object| !object.is_trashed(self))
            .filter_map(|object| object.into())
    }

    pub fn get_workflow(&self, workflow_id: &SyncId) -> Option<&CloudWorkflow> {
        self.objects_by_id
            .get(&workflow_id.uid())
            .and_then(|object| object.into())
    }

    pub fn get_workflow_by_uid(&self, uid: &str) -> Option<&CloudWorkflow> {
        self.objects_by_id.get(uid).and_then(|object| object.into())
    }

    pub fn get_workflow_enum(&self, enum_id: &SyncId) -> Option<&CloudWorkflowEnum> {
        self.objects_by_id
            .get(&enum_id.uid())
            .and_then(|object| object.into())
    }

    /// Returns only active (not trashed) workflows in cloud model.
    pub fn get_all_active_workflows(&self) -> impl Iterator<Item = &CloudWorkflow> {
        self.objects_by_id
            .values()
            .filter(|object| !object.is_trashed(self))
            .filter_map(|object| object.into())
    }

    /// Returns all active (not trashed) workflows in the space.
    pub fn active_workflows_in_space<'a>(
        &'a self,
        space: Space,
    ) -> impl Iterator<Item = &'a CloudWorkflow> + 'a {
        self.active_cloud_objects_in_space(space)
            .filter_map(|object| object.into())
    }

    /// Returns all active (not trashed) and non-welcome workflows (ie. non starter workflows) in the space.
    pub fn active_non_welcome_workflows_in_space<'a>(
        &'a self,
        space: Space,
    ) -> impl Iterator<Item = &'a CloudWorkflow> + 'a {
        self.active_non_welcome_cloud_objects_in_space(space)
            .filter_map(|object| object.into())
    }

    /// Returns all active (not trashed) and non-welcome notebooks (ie. non starter notebooks) in the space.
    pub fn active_non_welcome_notebooks_in_space<'a>(
        &'a self,
        space: Space,
    ) -> impl Iterator<Item = &'a CloudNotebook> + 'a {
        self.active_non_welcome_cloud_objects_in_space(space)
            .filter_map(|object| object.into())
    }

    /// Returns all active (not trashed) and non-welcome env var collections in the space.
    pub fn active_non_welcome_env_var_collections_in_space<'a>(
        &'a self,
        space: Space,
    ) -> impl Iterator<Item = &'a CloudEnvVarCollection> + 'a {
        self.active_non_welcome_cloud_objects_in_space(space)
            .filter_map(|object| object.into())
    }

    /// Returns all workflow enums with a given owner.
    pub fn workflow_enums_with_owner<'a>(
        &'a self,
        owner: Owner,
        _: &'a AppContext,
    ) -> impl Iterator<Item = &'a CloudWorkflowEnum> + 'a {
        self.objects_by_id
            .values()
            .filter(move |object| !object.is_trashed(self) && object.permissions().owner == owner)
            .filter_map(|object| object.into())
    }

    pub fn get_object_of_type<K, M>(&self, object_id: &SyncId) -> Option<&GenericCloudObject<K, M>>
    where
        K: HashableId + ToServerId + std::fmt::Debug + Into<String> + Clone + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        self.objects_by_id
            .get(&object_id.uid())
            .and_then(|object| object.into())
    }

    pub fn get_object_of_type_mut<K, M>(
        &mut self,
        object_id: &SyncId,
    ) -> Option<&mut GenericCloudObject<K, M>>
    where
        K: HashableId + ToServerId + std::fmt::Debug + Into<String> + Clone + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        self.objects_by_id
            .get_mut(&object_id.uid())
            .and_then(|object| object.into())
    }

    pub fn get_all_objects_of_type<K, M>(&self) -> impl Iterator<Item = &GenericCloudObject<K, M>>
    where
        K: HashableId + ToServerId + std::fmt::Debug + Into<String> + Clone + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        self.objects_by_id
            .values()
            .filter_map(|object| object.into())
    }

    pub fn get_notebook(&self, notebook_id: &SyncId) -> Option<&CloudNotebook> {
        self.objects_by_id
            .get(&notebook_id.uid())
            .and_then(|object| object.into())
    }

    pub fn get_notebook_by_uid(&self, uid: &str) -> Option<&CloudNotebook> {
        self.objects_by_id.get(uid).and_then(|object| object.into())
    }

    pub fn get_env_var_collection(
        &self,
        env_var_collection_id: &SyncId,
    ) -> Option<&CloudEnvVarCollection> {
        self.objects_by_id
            .get(&env_var_collection_id.uid())
            .and_then(|object| object.into())
    }

    pub fn get_env_var_collection_by_uid(&self, uid: &str) -> Option<&CloudEnvVarCollection> {
        self.objects_by_id.get(uid).and_then(|object| object.into())
    }

    /// Returns only active (not trashed) EVCs in cloud model.
    pub fn get_all_active_env_var_collections(
        &self,
    ) -> impl Iterator<Item = &CloudEnvVarCollection> {
        self.objects_by_id
            .values()
            .filter(|object| !object.is_trashed(self))
            .filter_map(|object| object.into())
    }

    /// Returns only active (not trashed) notebooks in cloud model.
    pub fn get_all_active_notebooks(&self) -> impl Iterator<Item = &CloudNotebook> {
        self.objects_by_id
            .values()
            .filter(|object| !object.is_trashed(self))
            .filter_map(|object| object.into())
    }

    #[cfg(test)]
    pub fn as_cloud_objects(&self) -> impl Iterator<Item = &'_ Box<dyn CloudObject>> {
        self.objects_by_id.values()
    }

    #[cfg(test)]
    pub fn add_object(&mut self, id: SyncId, object: impl CloudObject + 'static) {
        self.objects_by_id.insert(id.uid(), Box::new(object));
    }

    /// Pre-computes the set of UIDs for all active (non-trashed) objects using memoization.
    /// This is O(N) amortized instead of O(N × D) for the naive approach, because each
    /// object's trashed status is computed at most once and cached.
    pub fn active_object_uids(&self) -> HashSet<ObjectUid> {
        let mut cache = HashMap::new();
        let mut visiting = HashSet::new();
        let mut active = HashSet::new();
        for uid in self.objects_by_id.keys() {
            if !self.is_trashed_memoized(uid, &mut cache, &mut visiting) {
                active.insert(uid.clone());
            }
        }
        active
    }

    /// Memoized version of `is_trashed` that caches results to avoid redundant ancestor traversals.
    fn is_trashed_memoized(
        &self,
        uid: &str,
        cache: &mut HashMap<String, bool>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        if let Some(&cached) = cache.get(uid) {
            return cached;
        }

        // Cycle detection: if we're already visiting this UID in the current traversal, treat as trashed.
        if visiting.contains(uid) {
            return true;
        }

        let result = match self.objects_by_id.get(uid) {
            Some(object) => {
                if object.metadata().trashed_ts.is_some() {
                    true
                } else {
                    match object.metadata().folder_id.map(|parent_id| parent_id.uid()) {
                        Some(parent_uid) => {
                            visiting.insert(uid.to_owned());
                            let r = self.is_trashed_memoized(&parent_uid, cache, visiting);
                            visiting.remove(uid);
                            r
                        }
                        None => false,
                    }
                }
            }
            None => true,
        };

        cache.insert(uid.to_owned(), result);
        result
    }

    /// Given a CloudObjectLocation (either a folder or a space), returns an iterator of active (not trashed) cloud objects
    /// that live directly in this location (its children). I.e. this function does NOT look into nested folders in order
    /// to return those children.
    pub fn active_cloud_objects_in_location_without_descendents<'a>(
        &'a self,
        location: CloudObjectLocation,
    ) -> impl Iterator<Item = &'a dyn CloudObject> + 'a {
        self.objects_by_id
            .values()
            .filter(move |object| !object.is_trashed(self) && object.location(self) == location)
            .map(|object| object.as_ref())
    }

    /// Returns all active (not trashed) cloud objects in the space.
    pub fn active_cloud_objects_in_space<'a>(
        &'a self,
        space: Space,
    ) -> impl Iterator<Item = &'a dyn CloudObject> + 'a {
        self.objects_by_id
            .values()
            .filter(move |object| object.is_in_space(space) && !object.is_trashed(self))
            .map(|object| object.as_ref())
    }

    /// Returns all active (not trashed) cloud objects in the space.
    pub fn active_non_welcome_cloud_objects_in_space<'a>(
        &'a self,
        space: Space,
    ) -> impl Iterator<Item = &'a dyn CloudObject> + 'a {
        self.objects_by_id
            .values()
            .filter(move |object| {
                object.is_in_space(space) && !object.is_trashed(self) && !object.is_welcome_object()
            })
            .map(|object| object.as_ref())
    }

    #[cfg(test)]
    pub fn mock(_ctx: &mut ModelContext<Self>) -> Self {
        Self::new(None, Vec::new())
    }

    pub fn reset(&mut self) {
        self.objects_by_id = HashMap::new();
    }
}

impl Entity for CloudModel {
    type Event = CloudModelEvent;
}

/// Mark CloudModel as global application state.
impl SingletonEntity for CloudModel {}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
