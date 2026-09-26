use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use derivative::Derivative;
use pathfinder_geometry::vector::vec2f;
use serde::{Deserialize, Serialize};
use warp_core::ui::Icon;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::Fill;
use warpui_core::Element;
use warpui_core::elements::{
    Align, ChildAnchor, ConstrainedBox, Hoverable, MouseStateHandle, OffsetPositioning,
    ParentAnchor, ParentElement, ParentOffsetBounds, Stack,
};
use warpui_core::ui_components::components::UiComponent;

use crate::auth::UserUid;
use crate::drive::sharing::{SharingAccessLevel, Subject};
use crate::ids::{ServerId, SyncId};
use crate::time::ServerTimestamp;

mod generic_cloud_object;
mod generic_string_model;

pub use generic_cloud_object::*;
pub use generic_string_model::*;
/// The type of object id each ObjectType corresponds to.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ObjectIdType {
    Notebook,
    Workflow,
    Folder,
    GenericStringObject,
}

impl ObjectIdType {
    /// Returns the prefix for server IDs as we store them in sqlite. The prefix for these
    /// objects is in title case unlike how we store the object types, which is why two different
    /// APIs are needed.
    pub fn sqlite_prefix(&self) -> &'static str {
        match self {
            ObjectIdType::Notebook => "Notebook",
            ObjectIdType::Workflow => "Workflow",
            ObjectIdType::Folder => "Folder",
            ObjectIdType::GenericStringObject => "GenericStringObject",
        }
    }
}

/// A type for communicating the type of cloud object to/from the server, absent of the object itself.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
pub enum ObjectType {
    Notebook,
    Workflow,
    Folder,
    GenericStringObject(GenericStringObjectFormat),
}

impl ObjectType {
    /// Returns the serialized string for the object type, to be used for storing object_type in sqlite.
    pub fn sqlite_object_type_as_str(&self) -> Cow<'_, str> {
        match self {
            ObjectType::Notebook => "NOTEBOOK".into(),
            ObjectType::Workflow => "WORKFLOW".into(),
            ObjectType::Folder => "FOLDER".into(),
            ObjectType::GenericStringObject(format) => format.to_string().into(),
        }
    }
}

const NOTEBOOK_OBJECT_STRING: &str = "notebook";
const WORKFLOW_OBJECT_STRING: &str = "workflow";
const PROMPT_OBJECT_STRING: &str = "prompt";
const FOLDER_OBJECT_STRING: &str = "folder";
const ENV_VAR_COLLECTION_STRING: &str = "env-vars";

impl FromStr for ObjectType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            NOTEBOOK_OBJECT_STRING => Ok(Self::Notebook),
            WORKFLOW_OBJECT_STRING => Ok(Self::Workflow),
            PROMPT_OBJECT_STRING => Ok(Self::Workflow),
            FOLDER_OBJECT_STRING => Ok(Self::Folder),
            ENV_VAR_COLLECTION_STRING => Ok(Self::GenericStringObject(
                GenericStringObjectFormat::Json(JsonObjectType::EnvVarCollection),
            )),
            _ => Err(anyhow!("Unexpected object type")),
        }
    }
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectType::Notebook => write!(f, "{NOTEBOOK_OBJECT_STRING}"),
            ObjectType::Workflow => write!(f, "{WORKFLOW_OBJECT_STRING}"),
            ObjectType::Folder => write!(f, "{FOLDER_OBJECT_STRING}"),
            ObjectType::GenericStringObject(GenericStringObjectFormat::Json(
                JsonObjectType::EnvVarCollection,
            )) => write!(f, "{ENV_VAR_COLLECTION_STRING}"),
            ObjectType::GenericStringObject(GenericStringObjectFormat::Json(
                JsonObjectType::AIFact,
            )) => write!(f, "rule"),
            ObjectType::GenericStringObject(_) => write!(f, "string_object_placeholder"), // placeholder value
        }
    }
}

impl From<ObjectType> for ObjectIdType {
    fn from(value: ObjectType) -> Self {
        match value {
            ObjectType::Notebook => ObjectIdType::Notebook,
            ObjectType::Workflow => ObjectIdType::Workflow,
            ObjectType::Folder => ObjectIdType::Folder,
            ObjectType::GenericStringObject(_) => ObjectIdType::GenericStringObject,
        }
    }
}

/// The object type prefix for generic string objects.
pub const GENERIC_STRING_OBJECT_PREFIX: &str = "GENERIC_STRING_";

/// The object type prefix for json objects.
pub const JSON_OBJECT_PREFIX: &str = "JSON_";

/// The data format for the generic string object type.
/// Right now we only support json, but this is left
/// open to support markdown, yaml and other text based types.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
pub enum GenericStringObjectFormat {
    Json(JsonObjectType),
}

/// Represents a unique key for a generic string object. The server enforces that
/// no two generic string objects have the same key.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct GenericStringObjectUniqueKey {
    /// The unique key. E.g. for cloud prefs this is the storage key of the pref.
    pub key: String,

    /// Whether this key is unique for all generic string objects, or unique per user.
    pub unique_per: UniquePer,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum UniquePer {
    User,
}

// Temporarily suppress clippy warnings about the `ToString` impl until we
// move `ObjectType` away from using `std::fmt::Display` for serialization.
#[allow(clippy::to_string_trait_impl)]
impl ToString for GenericStringObjectFormat {
    fn to_string(&self) -> String {
        match self {
            GenericStringObjectFormat::Json(json_object_type) => format!(
                "{}{}{}",
                GENERIC_STRING_OBJECT_PREFIX,
                JSON_OBJECT_PREFIX,
                json_object_type.as_str()
            ),
        }
    }
}

/// An object sub-type for objects that implement the JsonModel trait.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
pub enum JsonObjectType {
    Preference,
    EnvVarCollection,
    WorkflowEnum,
    AIFact,
    MCPServer,
    AIExecutionProfile,
    TemplatableMCPServer,
    CloudEnvironment,
    ScheduledAmbientAgent,
    CloudAgentConfig,
}

impl JsonObjectType {
    pub fn as_str(&self) -> &'static str {
        match self {
            JsonObjectType::Preference => "PREFERENCE",
            JsonObjectType::EnvVarCollection => "ENVVARCOLLECTION",
            JsonObjectType::WorkflowEnum => "WORKFLOWENUM",
            JsonObjectType::AIFact => "AIFACT",
            JsonObjectType::MCPServer => "MCPSERVER",
            JsonObjectType::AIExecutionProfile => "AIEXECUTIONPROFILE",
            JsonObjectType::TemplatableMCPServer => "TEMPLATABLEMCPSERVER",
            JsonObjectType::CloudEnvironment => "CLOUDENVIRONMENT",
            JsonObjectType::ScheduledAmbientAgent => "SCHEDULEDAMBIENTAGENT",
            JsonObjectType::CloudAgentConfig => "CLOUDAGENTCONFIG",
        }
    }
}

impl TryFrom<&str> for JsonObjectType {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "PREFERENCE" => Ok(JsonObjectType::Preference),
            "ENVVARCOLLECTION" => Ok(JsonObjectType::EnvVarCollection),
            "WORKFLOWENUM" => Ok(JsonObjectType::WorkflowEnum),
            "AIFACT" => Ok(JsonObjectType::AIFact),
            "MCPSERVER" => Ok(JsonObjectType::MCPServer),
            "AIEXECUTIONPROFILE" => Ok(JsonObjectType::AIExecutionProfile),
            "TEMPLATABLEMCPSERVER" => Ok(JsonObjectType::TemplatableMCPServer),
            "CLOUDENVIRONMENT" => Ok(JsonObjectType::CloudEnvironment),
            "SCHEDULEDAMBIENTAGENT" => Ok(JsonObjectType::ScheduledAmbientAgent),
            "CLOUDAGENTCONFIG" => Ok(JsonObjectType::CloudAgentConfig),
            _ => Err(anyhow!("could not convert unknown json object type")),
        }
    }
}

/// The revision timestamp at which an object was edited. This is used by the server
/// to determine if an edit to an object was at the latest revision. Edits at older
/// revisions are rejected by the server.
#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, PartialEq, PartialOrd, Ord)]
pub struct Revision(ServerTimestamp);

impl Revision {
    pub fn from_unix_timestamp_micros(ms_since_epoch: i64) -> Result<Self> {
        let ts = ServerTimestamp::from_unix_timestamp_micros(ms_since_epoch)?;
        Ok(Self(ts))
    }

    pub fn timestamp_micros(&self) -> i64 {
        self.0.timestamp_micros()
    }

    pub fn utc(&self) -> DateTime<Utc> {
        self.0.utc()
    }

    /// Returns the inner `ServerTimestamp`.
    pub fn timestamp(&self) -> ServerTimestamp {
        self.0
    }

    #[cfg(any(test, feature = "test-util"))]
    pub fn now() -> Self {
        Self(ServerTimestamp::new(Utc::now()))
    }
}

impl From<Revision> for ServerTimestamp {
    fn from(revision: Revision) -> Self {
        revision.0
    }
}

impl From<ServerTimestamp> for Revision {
    fn from(time: ServerTimestamp) -> Self {
        Revision(time)
    }
}

#[cfg(any(test, feature = "test-util"))]
impl From<DateTime<Utc>> for Revision {
    fn from(time: DateTime<Utc>) -> Self {
        Self(ServerTimestamp::new(time))
    }
}

/// The owner for a given object.
#[derive(Copy, Clone, Debug, Eq, Serialize, Deserialize, Derivative)]
#[derivative(PartialEq)]
pub enum Owner {
    /// The owner of the object is a user (the object is in their personal drive).
    User { user_uid: UserUid },
    /// The owner of the object is a team (the object is in a team drive).
    Team { team_uid: ServerId },
}

impl Owner {
    /// A mock [`Owner`] ID for testing.
    #[cfg(any(test, feature = "test-util"))]
    pub fn mock_current_user() -> Owner {
        use crate::auth::TEST_USER_UID;

        Owner::User {
            user_uid: UserUid::new(TEST_USER_UID),
        }
    }
}

impl From<Owner> for Option<ServerId> {
    fn from(owner: Owner) -> Option<ServerId> {
        match owner {
            Owner::User { .. } => None,
            Owner::Team { team_uid, .. } => Some(team_uid),
        }
    }
}

/// Server representation of an object's container. This corresponds to the `Container` GraphQL
/// type.
///
/// Containers are similar to, but not quite the same as, the [`CloudObjectLocation`] type.
/// Locations depend on object and user state - an object might currently be in the trash, or
/// it could be in one user's [shared space](Space::Shared) but another's
/// [team space](Space::Team). Containers, on the other hand, represent an object's canonical
/// parent - its one parent folder or drive that permissions are inherited from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerObjectContainer {
    Folder { folder_uid: ServerId },
    Drive { owner: Owner },
}

#[derive(Clone, Debug)]
pub struct NumInFlightRequests(pub usize);

#[derive(Clone, Debug)]
/// An enum representing what state a local cloud object's content changes can be in,
/// in relation to the server.
pub enum CloudObjectSyncStatus {
    /// The object's content hasn't changed from what we believe the server's representation
    /// to be.
    NoLocalChanges,
    /// The object's content has been modified locally, and is currently in the sync queue
    /// attempting to sync up with the server.
    InFlight(NumInFlightRequests),
    /// The object's content has been modified locally but has unresolved conflict with the server
    /// revision.
    InConflict,
    /// The object's content has been modified locally, but persisting the change on the server
    /// could not complete for some reason.
    Errored,
}

const SYNC_ICON_DIMENSIONS: f32 = 16.;

const SYNC_STATUS_TOOLTIP_LOCAL_ONLY: &str = "Saved locally";
const SYNC_STATUS_TOOLTIP_INFLIGHT: &str = "Saving";
const SYNC_STATUS_TOOLTIP_ERROR: &str = "Failed to save";

#[derive(Debug, Clone, PartialEq)]
pub struct CloudObjectPermissions {
    pub owner: Owner,
    pub permissions_last_updated_ts: Option<ServerTimestamp>,
    pub anyone_with_link: Option<CloudLinkSharing>,
    pub guests: Vec<CloudObjectGuest>,
}

impl CloudObjectPermissions {
    /// Mock permissions for a personal object.
    #[cfg(any(test, feature = "test-util"))]
    pub fn mock_personal() -> Self {
        Self {
            owner: Owner::mock_current_user(),
            permissions_last_updated_ts: Some(Utc::now().into()),
            guests: Vec::new(),
            anyone_with_link: None,
        }
    }

    /// Returns `true` if the given user has direct personal access to this object —
    /// either via an explicit user guest ACL entry or via link sharing.
    /// Returns `false` if the only access is through a team guest ACL.
    pub fn has_direct_user_access(&self, user_uid: UserUid) -> bool {
        self.anyone_with_link.is_some() || self.guests.iter().any(|g| g.subject.is_user(user_uid))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CloudLinkSharing {
    pub access_level: SharingAccessLevel,
    // If this sharing setting was inherited, the `source` identifies the container it's inherited
    // from.
    pub source: Option<ServerObjectContainer>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CloudObjectGuest {
    pub subject: Subject,
    pub access_level: SharingAccessLevel,
    /// If this guest was added to a container object, the `source` identifies that object.
    pub source: Option<ServerObjectContainer>,
}

#[derive(Clone, Debug)]
pub struct CloudObjectMetadata {
    pub revision: Option<Revision>,
    pub metadata_last_updated_ts: Option<ServerTimestamp>,
    pub current_editor_uid: Option<String>,
    pub pending_changes_statuses: CloudObjectStatuses,
    pub trashed_ts: Option<ServerTimestamp>,
    pub folder_id: Option<SyncId>,
    /// Welcome objects are created on the server when a user first receives
    /// access to Warp Drive as part of onboarding.
    pub is_welcome_object: bool,
    pub last_editor_uid: Option<String>,
    pub creator_uid: Option<String>,
    /// The "last used" timestamp for this environment.
    ///
    /// This is populated via `GetCloudEnvironments` from
    /// `CloudEnvironment.lastTaskCreated.createdAt`.
    /// Only applicable for CloudEnvironment objects.
    pub last_task_run_ts: Option<ServerTimestamp>,
}

impl CloudObjectMetadata {
    /// Creates a new set of metadata with reasonable defaults for a test:
    /// * Content and metadata timestamps set to now
    /// * No editor information
    /// * No parent folder
    /// * Not trashed
    #[cfg(any(test, feature = "test-util"))]
    pub fn mock() -> Self {
        Self {
            revision: Some(Revision::now()),
            current_editor_uid: None,
            metadata_last_updated_ts: Some(Utc::now().into()),
            pending_changes_statuses: CloudObjectStatuses::mock(),
            trashed_ts: None,
            folder_id: None,
            is_welcome_object: false,
            last_editor_uid: None,
            creator_uid: None,
            last_task_run_ts: None,
        }
    }

    pub fn has_pending_content_changes(&self) -> bool {
        !matches!(
            self.pending_changes_statuses.content_sync_status,
            CloudObjectSyncStatus::NoLocalChanges | CloudObjectSyncStatus::InConflict
        )
    }

    pub fn is_errored(&self) -> bool {
        matches!(
            self.pending_changes_statuses.content_sync_status,
            CloudObjectSyncStatus::Errored
        )
    }

    /// True iff there are unsynced online-only changes for the object.
    pub fn has_pending_online_only_change(&self) -> bool {
        self.pending_changes_statuses.has_pending_permissions_change
            || self.pending_changes_statuses.has_pending_metadata_change
            || self.pending_changes_statuses.pending_untrash
            || self.pending_changes_statuses.pending_delete
    }

    pub fn set_current_editor(&mut self, editor_uid: Option<String>) {
        self.current_editor_uid = editor_uid;
    }
}

/// A struct holding the different statuses of pending changes that a cloud object might have.
/// Note that content is handled differently than permissions/metadata:
///   * Content changes go through the sync queue, and thus can exist in more states
///   * Metadata/permissions changes are synchronous operations, and thus are only either
///     in flight or synced
#[derive(Clone, Debug)]
pub struct CloudObjectStatuses {
    pub content_sync_status: CloudObjectSyncStatus,
    /// True iff there are unsynced permission changes for the object.
    /// We intentionally don't persist this value in sqlite. And if true,
    /// we don't upsert any in-memory permission changes to sqlite.
    pub has_pending_permissions_change: bool,
    /// True iff there are unsynced metadata changes for the object.
    /// We intentionally don't persist this value in sqlite. And if true,
    /// we don't upsert trashed and folder changes to sqlite.
    pub has_pending_metadata_change: bool,

    /// True iff there is an unsynced untrash operation on the object.
    pub pending_untrash: bool,

    /// True iff there is an unsynced delete operation on the object.
    pub pending_delete: bool,
}

impl CloudObjectStatuses {
    /// Empty statuses with no in-flight changes, for use in tests.
    #[cfg(any(test, feature = "test-util"))]
    pub fn mock() -> Self {
        Self {
            content_sync_status: CloudObjectSyncStatus::NoLocalChanges,
            has_pending_permissions_change: false,
            has_pending_metadata_change: false,
            pending_untrash: false,
            pending_delete: false,
        }
    }

    pub fn render_icon(
        &self,
        sync_queue_is_dequeueing: bool,
        hover_state: MouseStateHandle,
        appearance: &Appearance,
    ) -> Option<Box<dyn Element>> {
        let theme = appearance.theme();
        let has_in_flight_requests = match &self.content_sync_status {
            CloudObjectSyncStatus::InFlight(reqs) => reqs.0 > 0,
            _ => false,
        };

        let should_show_local_only_indicator = has_in_flight_requests && !sync_queue_is_dequeueing;
        let should_show_syncing_indicator = has_in_flight_requests
            || self.has_pending_metadata_change
            || self.has_pending_permissions_change
            || self.pending_untrash;
        let should_show_error_indicator = matches!(
            self.content_sync_status,
            CloudObjectSyncStatus::Errored | CloudObjectSyncStatus::InConflict
        );

        let icon_and_tooltip_text = if should_show_local_only_indicator {
            Some((
                Icon::Laptop.to_warpui_icon(theme.main_text_color(theme.surface_1())),
                SYNC_STATUS_TOOLTIP_LOCAL_ONLY,
            ))
        } else if should_show_syncing_indicator {
            Some((
                Icon::Refresh.to_warpui_icon(theme.sub_text_color(theme.surface_2())),
                SYNC_STATUS_TOOLTIP_INFLIGHT,
            ))
        } else if should_show_error_indicator {
            Some((
                Icon::AlertTriangle.to_warpui_icon(Fill::Solid(theme.ui_error_color())),
                SYNC_STATUS_TOOLTIP_ERROR,
            ))
        } else {
            None
        };

        if let Some((icon, tooltip_text)) = icon_and_tooltip_text {
            return Some(
                Align::new(
                    Hoverable::new(hover_state, move |hover_state| {
                        let mut stack = Stack::new().with_child(
                            ConstrainedBox::new(icon.finish())
                                .with_height(SYNC_ICON_DIMENSIONS)
                                .with_width(SYNC_ICON_DIMENSIONS)
                                .finish(),
                        );

                        if hover_state.is_hovered() {
                            let tooltip = appearance
                                .ui_builder()
                                .tool_tip(tooltip_text.to_string())
                                .build()
                                .finish();

                            stack.add_positioned_overlay_child(
                                tooltip,
                                OffsetPositioning::offset_from_parent(
                                    vec2f(0., -24.),
                                    ParentOffsetBounds::Unbounded,
                                    ParentAnchor::Center,
                                    ChildAnchor::Center,
                                ),
                            );
                        }

                        stack.finish()
                    })
                    .finish(),
                )
                .finish(),
            );
        }

        None
    }
}

// Used for event tracking purposes, matches
// up with GraphQL enum of the same name.
#[derive(Copy, Default, Clone, Debug, Eq, PartialEq)]
pub enum CloudObjectEventEntrypoint {
    TeamSettings,
    ResourceCenter,
    UniversalSearch,
    ManagementUI,
    Blocklist,
    ImportModal,
    Onboarding,
    #[default]
    Unknown,
}

// A newtype for a serialized model that wraps a plain string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializedModel(String);

impl SerializedModel {
    pub fn new(s: String) -> Self {
        Self(s)
    }

    pub fn model_as_str(&self) -> &str {
        &self.0
    }

    pub fn take(self) -> String {
        self.0
    }
}

impl From<String> for SerializedModel {
    fn from(s: String) -> Self {
        Self(s)
    }
}
