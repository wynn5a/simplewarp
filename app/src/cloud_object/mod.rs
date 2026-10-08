use std::any::Any;
use std::collections::HashSet;
use std::fmt::Debug;

use async_trait::async_trait;
use cloud_objects::UserUid;
use cloud_objects::cloud_object::SerializedModel;
use lazy_static::lazy_static;
use regex::Regex;
use warpui::{AppContext, SingletonEntity};

use self::model::generic_string_model::{
    GenericStringModel, GenericStringObjectId, Serializer, StringModel,
};
use self::model::persistence::CloudModel;
use crate::persistence::ModelEvent;
use crate::server::ids::{HashableId, HashedSqliteId, ObjectUid, SyncId, ToServerId};
use crate::util::time_format::format_approx_duration_from_now_utc;

pub mod cloud_object_styling;
pub mod drive_object_type;
pub mod export;
pub mod folders;
pub mod model;
pub mod toast_message;

pub use cloud_objects::cloud_object::*;
pub use cloud_objects::drive::CloudObjectTypeAndId;
pub use drive_object_type::DriveObjectType;

/// A CloudObject represents
/// therefore shareable and editable (i.e. Notebooks and Workflows). In order
/// to support collaborative editing of these objects, they must each store local
/// revision numbers to ensure a stable way of accepting and rejecting edits.
///
/// Note that this trait must be object-safe and non-generic.  The reason for this
/// is that (a) we need to be able to store instances of it as trait objects in
/// CloudModel and (b) we need to be able to support mixed collections of different
/// instances of it (e.g. in the map of id -> CloudObject in CloudModel).
///
/// There are two closely related types to this:
/// 1) GenericCloudObject: This is the concrete generic implementation of CloudObject that
///    holds onto a model of type CloudModelType and an id of type SyncId.
/// 2) CloudModelType: This is a trait that defines the model type for a CloudObject -
///    this is what implementors of new cloud types typically have to implement.
///
/// These types are tightly coupled.  In an ideal world, rust would allow a mechanism
/// for us having a single interface that new model types could implement that could
/// be generic on id and model types, but as far as I (zach) can tell, that's not currently
/// possible.
///
/// The typical usage pattern for these types is to use dyn CloudObject whenever you
/// don't need access to a model or id, and to downcast to a GenericCloudObject whenever you do.
///
/// This implies that, for now, *all* CloudObjects must implement GenericCloudObject.
///
/// For more info on revisions: https://docs.google.com/document/d/1SGtX_5AiSJmUxXCRk5NzGTzrC_XrxQRsio-KZOec_ng/edit
pub trait CloudObject: Debug {
    /// Returns the name of this model type (e.g. Workflow, Folder, Notebook)
    fn model_type_name(&self) -> &'static str;

    /// Returns the  uid for this object.
    fn uid(&self) -> ObjectUid;

    /// Returns the [`SyncId`] that currently identifies this object.
    fn sync_id(&self) -> SyncId;

    /// Returns the id used to index into sqlite, this is the object's UID with its type
    /// prefixed, such as "Workflow-{UID}"
    fn hashed_sqlite_id(&self) -> HashedSqliteId;

    /// Returns the CloudObjectMetadata struct associated with this object.
    fn metadata(&self) -> &CloudObjectMetadata;

    /// Returns a mutable reference to the CloudObjectMetadata struct associated with this object.
    fn metadata_mut(&mut self) -> &mut CloudObjectMetadata;

    /// Returns the CloudObjectPermissions struct associated with this object.
    fn permissions(&self) -> &CloudObjectPermissions;

    /// Returnsa mutable reference to the CloudObjectPermissions struct associated with this object.
    fn permissions_mut(&mut self) -> &mut CloudObjectPermissions;

    /// Returns the ObjectType i.e. 'Workflow' or 'Notebook'
    fn object_type(&self) -> ObjectType;

    /// Returns the CloudObjectTypeAndId for this object.
    fn cloud_object_type_and_id(&self) -> CloudObjectTypeAndId;

    // Whether to clear this object from the local SQLite DB on a unique key conflict.
    fn should_clear_on_unique_key_conflict(&self) -> bool {
        false
    }

    /// Whether to show a warning if this object is unsaved at quit time
    /// (which typically blocks the user from quitting)
    fn warn_if_unsaved_at_quit(&self) -> bool {
        true
    }

    /// Returns the "upsert" event for inserting / updating this object in the SQLite DB.
    fn upsert_event(&self) -> ModelEvent;

    // Returns the name of the object.
    fn display_name(&self) -> String;

    /// Returns whether this model type should show update toasts in the UI.
    fn should_show_activity_toasts(&self) -> bool {
        true
    }

    // Returns the names of all the containing "objects" for this object, ordered from
    // the personal space down to the direct parent folder.
    fn containing_object_names(&self, app: &AppContext) -> Vec<String> {
        let mut names = vec![PERSONAL_SPACE_NAME.to_string()];
        if let Some(folder_id) = self.metadata().folder_id {
            let cloud_model = CloudModel::as_ref(app);
            let mut chain = Vec::new();
            let mut current = cloud_model.get_folder_by_uid(&folder_id.uid());
            while let Some(folder) = current {
                chain.push(folder.display_name());
                current = folder
                    .metadata()
                    .folder_id
                    .and_then(|parent_id| cloud_model.get_folder_by_uid(&parent_id.uid()));
            }
            names.extend(chain.into_iter().rev());
        }
        names
    }

    fn breadcrumbs(&self, app: &AppContext) -> String {
        self.containing_object_names(app).join(" / ")
    }

    fn is_welcome_object(&self) -> bool {
        self.metadata().is_welcome_object
    }

    /// The folder this object is placed in directly (even if that folder is nested), or `None`
    /// when it sits at the top level of the personal space.
    fn parent_folder(&self, cloud_model: &CloudModel) -> Option<SyncId> {
        self.metadata()
            .folder_id
            .filter(|folder_id| cloud_model.get_folder(folder_id).is_some())
    }

    /// Return true is this object or any of its ancestors are trashed. Also returns true
    /// if a cycle is detected.
    fn is_trashed(&self, cloud_model: &CloudModel) -> bool {
        self.is_trashed_internal(cloud_model, &mut HashSet::new())
    }

    /// Helper function for is_trashed.
    fn is_trashed_internal(
        &self,
        cloud_model: &CloudModel,
        ancestors: &mut HashSet<String>,
    ) -> bool {
        // Base case: If the object is trashed, return true.
        if self.metadata().trashed_ts.is_some() {
            return true;
        }

        // Else: return true if the object's parent is trashed. Return false if the object has no parent.
        match self.metadata().folder_id.map(|parent_id| parent_id.uid()) {
            Some(hashed_parent_id) => {
                // We need to check for cycles to avoid causing a stack overflow. If a cycle is detected, return that the object is trashed.
                if ancestors.contains(&hashed_parent_id) {
                    return true;
                }

                let parent = cloud_model.get_by_uid(&hashed_parent_id);

                // Insert before checking parent to avoid infinite recursion in case of cycles.
                ancestors.insert(hashed_parent_id);

                match parent {
                    Some(parent) => parent.is_trashed_internal(cloud_model, ancestors),
                    None => {
                        // If the object has a parent, but the parent is not in CloudModel,
                        // treat the object as trashed.
                        true
                    }
                }
            }
            None => false,
        }
    }

    /// Whether or not this object can be exported.
    fn can_export(&self) -> bool;

    /// Returns this object as a ref to the Any type.  Needed for typecasts.
    fn as_any(&self) -> &dyn Any;

    /// Returns this object as a mut ref to Any type.  Needed for typecasts.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Returns the trait object as a concrete type reference by downcasting it.
    /// Returns None if the downcast fails.
    fn as_model_type<K, M>(cloud_object: &dyn CloudObject) -> Option<&GenericCloudObject<K, M>>
    where
        Self: Sized,
        K: HashableId + ToServerId + Debug + Into<String> + Clone + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        cloud_object
            .as_any()
            .downcast_ref::<GenericCloudObject<K, M>>()
    }

    /// Returns the trait object as a concrete mutable type reference by downcasting it.
    /// Returns None if the downcast fails.
    fn as_model_type_mut<K, M>(
        cloud_object: &mut dyn CloudObject,
    ) -> Option<&mut GenericCloudObject<K, M>>
    where
        Self: Sized,
        K: HashableId + ToServerId + Debug + Into<String> + Clone + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        cloud_object
            .as_any_mut()
            .downcast_mut::<GenericCloudObject<K, M>>()
    }

    /// Returns a cloned boxed version of this cloud object.
    /// Note that we can't force the CloudObject trait to derive from Cloned
    /// directly because that would make the trait not object safe.  This
    /// is a workaround.
    fn clone_box(&self) -> Box<dyn CloudObject>;
}

/// Defines a common trait for cloud models to implement.
/// The "model" is the domain specific piece of data for a cloud object,
/// e.g. it contains the notebook, workflow, or folder specific data, but has
/// no logic around metadata, permissions, or sync status.
///
/// See the comments for CloudObject to understand the relationship between
/// this trait, CloudObject and GenericCloudObject.  They are tightly coupled.
///
/// When building new model types (e.g. for settings or launch configs) we should just
/// have to implement this trait, and not the entire CloudObject trait.
#[async_trait]
pub trait CloudModelType: Debug + Clone + Send + Sync {
    /// The associated CloudObject type for this model (e.g. CloudNotebook, CloudWorkflow, etc)
    type CloudObjectType: CloudObject + 'static;
    // TODO: @ianhodge - remove for sync ID refactor.
    type IdType: HashableId + ToServerId + Debug + Into<String> + Clone + 'static;

    /// Returns the name of this model type (e.g. Workflow, Folder, Notebook)
    fn model_type_name(&self) -> &'static str;

    /// Returns the CloudObjectTypeAndId for this object.
    fn cloud_object_type_and_id(&self, id: SyncId) -> CloudObjectTypeAndId;

    /// Returns the ObjectType for this model.
    fn object_type(&self) -> ObjectType;

    /// Returns whether this model type should show update toasts in the UI.
    fn should_show_activity_toasts(&self) -> bool {
        true
    }

    /// Whether to show a warning if this model is unsaved at quit time
    /// (which typically blocks the user from quitting)
    fn warn_if_unsaved_at_quit(&self) -> bool {
        true
    }

    /// Returns the display name for this model.
    fn display_name(&self) -> String;

    /// Sets the display name.  Setting the name
    /// is not currently supported by all object types, hence the default empty
    /// implementation.
    fn set_display_name(&mut self, _name: &str) {}

    /// Returns the upsert event for putting this model into the SQLite database.
    fn upsert_event(params: CloudObjectUpsertParams<Self>) -> ModelEvent
    where
        Self: Sized;

    /// Returns a serialized model.
    fn serialized(&self) -> SerializedModel;

    /// Returns whether this model type should clear on a unique key conflict.
    fn should_clear_on_unique_key_conflict(&self) -> bool {
        false
    }

    /// Returns whether this model type supports web links
    fn supports_linking(&self) -> bool {
        true
    }
    /// Whether this model type can be exported.
    fn can_export(&self) -> bool {
        false
    }
}
/// Provides app-local typed lookup helpers for generic cloud object aliases.
pub trait CloudObjectLookup: Sized + Clone {
    fn get_all(app: &AppContext) -> Vec<Self>;

    fn get_by_id<'a>(sync_id: &'a SyncId, app: &'a AppContext) -> Option<&'a Self>;
}

impl<K, M> CloudObjectLookup for GenericCloudObject<K, M>
where
    K: HashableId + ToServerId + Debug + Into<String> + Clone + 'static,
    M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
{
    fn get_all(app: &AppContext) -> Vec<Self> {
        CloudModel::as_ref(app)
            .get_all_objects_of_type::<K, M>()
            .cloned()
            .collect()
    }

    fn get_by_id<'a>(sync_id: &'a SyncId, app: &'a AppContext) -> Option<&'a Self> {
        CloudModel::as_ref(app).get_object_of_type::<K, M>(sync_id)
    }
}

/// Marks string model payloads that can be looked up by UUID.
pub trait CloudObjectUuid {
    fn uuid(&self) -> uuid::Uuid;
}

/// Provides app-local UUID lookups for cloud objects whose payload exposes a UUID.
pub trait CloudObjectUuidLookup: Sized {
    fn get_by_uuid<'a>(uuid: &'a uuid::Uuid, app: &'a AppContext) -> Option<&'a Self>;
}

impl<T, S> CloudObjectUuidLookup
    for GenericCloudObject<GenericStringObjectId, GenericStringModel<T, S>>
where
    T: StringModel<
            CloudObjectType = GenericCloudObject<GenericStringObjectId, GenericStringModel<T, S>>,
        > + CloudObjectUuid,
    S: Serializer<T>,
{
    fn get_by_uuid<'a>(uuid: &'a uuid::Uuid, app: &'a AppContext) -> Option<&'a Self> {
        CloudModel::as_ref(app)
            .get_all_objects_of_type::<GenericStringObjectId, GenericStringModel<T, S>>()
            .find(|object| object.model().string_model.uuid() == *uuid)
    }
}

lazy_static! {
    static ref SPACE_DETECT_RE: Regex = Regex::new(r"\s+").expect("Expect regex to be valid");
    static ref SAFE_URL_CHAR_RE: Regex =
        Regex::new(r"[^a-zA-Z0-9\s-]").expect("Expect regex to be valid");
}

impl<K, M> CloudObject for GenericCloudObject<K, M>
where
    K: HashableId + ToServerId + Debug + Into<String> + Clone + 'static,
    M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
{
    fn model_type_name(&self) -> &'static str {
        self.model().model_type_name()
    }

    fn uid(&self) -> ObjectUid {
        self.id.uid()
    }

    fn hashed_sqlite_id(&self) -> HashedSqliteId {
        self.id.sqlite_uid_hash(self.object_type().into())
    }

    fn sync_id(&self) -> SyncId {
        self.id
    }

    fn should_show_activity_toasts(&self) -> bool {
        self.model().should_show_activity_toasts()
    }

    fn warn_if_unsaved_at_quit(&self) -> bool {
        self.model().warn_if_unsaved_at_quit()
    }

    fn metadata(&self) -> &CloudObjectMetadata {
        &self.metadata
    }

    fn metadata_mut(&mut self) -> &mut CloudObjectMetadata {
        &mut self.metadata
    }

    fn permissions(&self) -> &CloudObjectPermissions {
        &self.permissions
    }

    fn permissions_mut(&mut self) -> &mut CloudObjectPermissions {
        &mut self.permissions
    }

    fn object_type(&self) -> ObjectType {
        self.model().object_type()
    }

    fn cloud_object_type_and_id(&self) -> CloudObjectTypeAndId {
        self.model().cloud_object_type_and_id(self.id)
    }

    fn should_clear_on_unique_key_conflict(&self) -> bool {
        self.model().should_clear_on_unique_key_conflict()
    }

    fn upsert_event(&self) -> ModelEvent {
        M::upsert_event(self.upsert_params(self.object_type()))
    }

    fn display_name(&self) -> String {
        self.model().display_name()
    }

    fn can_export(&self) -> bool {
        self.model().can_export()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn CloudObject> {
        Box::new(self.clone())
    }
}

impl<'a, K, M> From<&'a dyn CloudObject> for Option<&'a GenericCloudObject<K, M>>
where
    K: HashableId + ToServerId + Debug + Into<String> + Clone + 'static,
    M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
{
    fn from(value: &'a dyn CloudObject) -> Self {
        <GenericCloudObject<K, M> as CloudObject>::as_model_type(value)
    }
}

impl<'a, K, M> From<&'a Box<dyn CloudObject>> for Option<&'a GenericCloudObject<K, M>>
where
    K: HashableId + ToServerId + Debug + Into<String> + Clone + 'static,
    M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
{
    fn from(value: &'a Box<dyn CloudObject>) -> Self {
        <GenericCloudObject<K, M> as CloudObject>::as_model_type(value.as_ref())
    }
}

impl<'a, K, M> From<&'a mut Box<dyn CloudObject>> for Option<&'a mut GenericCloudObject<K, M>>
where
    K: HashableId + ToServerId + Debug + Into<String> + Clone + 'static,
    M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
{
    fn from(value: &'a mut Box<dyn CloudObject>) -> Self {
        <GenericCloudObject<K, M> as CloudObject>::as_model_type_mut(value.as_mut())
    }
}

impl Clone for Box<dyn CloudObject> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

impl From<&dyn CloudObject> for ObjectType {
    fn from(value: &dyn CloudObject) -> Self {
        value.object_type()
    }
}

impl From<&Box<dyn CloudObject>> for ObjectType {
    fn from(value: &Box<dyn CloudObject>) -> Self {
        <ObjectType as From<&dyn CloudObject>>::from(value.as_ref())
    }
}

/// Extension trait for CloudObjectMetadata.
pub trait CloudObjectMetadataExt {
    /// Returns a semantic summary of the last edit to the object. For example, "Edited 4 weeks
    /// ago". Returns None if the revision is None.
    fn semantic_editing_history(&self) -> Option<String>;
}

impl CloudObjectMetadataExt for CloudObjectMetadata {
    fn semantic_editing_history(&self) -> Option<String> {
        self.revision
            .map(|r| format!("Edited {}", format_approx_duration_from_now_utc(r.utc())))
    }
}

/// Display name of the single, personal space every object lives in. Also the export
/// subdirectory name.
pub const PERSONAL_SPACE_NAME: &str = "Personal";

/// The uid that owns every object the local user creates. There is no account, so it is fixed.
pub const LOCAL_USER_UID: &str = "local_user";

/// The [`Owner`] for the user's personal drive.
pub fn personal_drive() -> Owner {
    Owner::User {
        user_uid: UserUid::new(LOCAL_USER_UID),
    }
}
