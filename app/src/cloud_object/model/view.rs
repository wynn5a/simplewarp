use std::cell::RefCell;
use std::collections::HashMap;

use chrono::{Duration, Utc};
use cloud_objects::time::ServerTimestamp;
use warpui::{AppContext, Entity, ModelContext, ModelHandle, SingletonEntity};

use super::persistence::{CloudModel, CloudModelEvent};
use crate::auth::AuthStateProvider;
use crate::cloud_object::CloudObject;
use crate::cloud_object::folders::CloudFolder;
use crate::safe_info;
use crate::server::ids::{ObjectUid, SyncId};

pub const EDITOR_TIMEOUT_DURATION_MINUTES: i64 = 15;

#[derive(Default, Clone, Debug, PartialEq)]
pub enum EditorState {
    #[default]
    None,
    CurrentUser,
    OtherUserActive,
    OtherUserIdle,
}

/// Stores information about the current editor of a particular notebook, for display purposes.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Editor {
    pub state: EditorState,
}

impl Editor {
    pub fn no_editor() -> Self {
        Self {
            state: EditorState::None,
        }
    }
}

/// Singleton model for storing and querying the data and logic logic needed by various view, based on the information
/// stored in [CloudModel]. As a general, rule, any new API that requires logic beyond just retrieving the raw value
/// in [CloudModel], should be stored here. This includes logic such as object trashed status, the object current editor,
/// and object location.
///
/// Any API added to this model should be unit tested in model_test.rs
pub struct CloudViewModel {
    folder_timestamp_cache: FolderTimestampCache,
}

type FolderTimestampCache = RefCell<HashMap<SyncId, ServerTimestamp>>;

pub enum CloudViewModelEvent {
    /// A model change has invalidated object sort timestamps.
    SortTimestampsChanged,
}

impl CloudViewModel {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        ctx.subscribe_to_model(&CloudModel::handle(ctx), Self::handle_cloud_model_event);
        Self {
            folder_timestamp_cache: Default::default(),
        }
    }

    #[cfg(test)]
    pub fn mock(ctx: &mut ModelContext<Self>) -> Self {
        Self::new(ctx)
    }

    /// Returns the current editor of the object based on what current exists in CloudModel. If the current editor
    /// matches the logged in user's email, we assume that that user is the current editor.
    /// If the current editor hasn't made an edit in the past 15 minutes, they are considered idle and
    /// we instead just return Editor::OtherUserIdle. This is to prevent introducing friction into the baton grabbing process
    /// when it's not needed. For more info see:
    /// https://docs.google.com/document/d/1KgDFLApPg1uDVP-vOwhZzL1kRIviS8mMECIZg2VCKLY/edit
    pub fn object_current_editor(&self, uid: &ObjectUid, ctx: &AppContext) -> Option<Editor> {
        let cloud_model = CloudModel::as_ref(ctx);
        let object = cloud_model.get_by_uid(uid)?;

        match &object.metadata().current_editor_uid {
            Some(uid) => {
                let auth_state = AuthStateProvider::as_ref(ctx).get();
                let user_uid = auth_state.user_id();

                // If the logged in user matches the current UID, then the editor is the current
                // user.
                if user_uid.is_some_and(|user_uid| user_uid.as_string() == uid.clone()) {
                    return Some(Editor {
                        state: EditorState::CurrentUser,
                    });
                }

                match &object.metadata().revision {
                    Some(revision) => {
                        let time_since_last_edit = Utc::now() - revision.utc();
                        let time_since_last_metadata_change = Utc::now()
                            - object
                                .metadata()
                                .metadata_last_updated_ts
                                .unwrap_or(Utc::now().into())
                                .utc();
                        if time_since_last_edit > Duration::minutes(EDITOR_TIMEOUT_DURATION_MINUTES)
                            && time_since_last_metadata_change
                                > Duration::minutes(EDITOR_TIMEOUT_DURATION_MINUTES)
                        {
                            safe_info!(
                                safe: ("Current editor idle, eagerly grabbing edit access for notebook"),
                                full: ("Current editor idle, eagerly grabbing edit access for notebook with editor: {}", uid.clone())
                            );
                            Some(Editor {
                                state: EditorState::OtherUserIdle,
                            })
                        } else {
                            Some(Editor {
                                state: EditorState::OtherUserActive,
                            })
                        }
                    }
                    None => Some(Editor {
                        state: EditorState::OtherUserActive,
                    }),
                }
            }
            _ => Some(Editor::no_editor()),
        }
    }

    fn handle_cloud_model_event(
        &mut self,
        _: ModelHandle<CloudModel>,
        event: &CloudModelEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        match event {
            CloudModelEvent::ObjectUpdated { type_and_id, .. }
            | CloudModelEvent::ObjectTrashed { type_and_id, .. }
            | CloudModelEvent::ObjectUntrashed { type_and_id, .. } => {
                // If an object is updated, we need to recompute the timestamps of its parents.
                if self.invalidate_object_timestamps(&type_and_id.uid(), CloudModel::as_ref(ctx)) {
                    ctx.emit(CloudViewModelEvent::SortTimestampsChanged);
                }
            }
            CloudModelEvent::ObjectCreated { type_and_id } => {
                // There are three cases for an ObjectCreated event:
                // 1. We created a new object locally (in which case type_and_id is a client ID)
                // 2. We were notified about a new object from the server.
                // 3. A locally-created object was saved to the server, so we now have a server ID
                //    for it.
                // Because we sort on server timestamps, only the second or third cases can affect
                // sorting.
                if type_and_id.has_server_id()
                    && self
                        .invalidate_object_timestamps(&type_and_id.uid(), CloudModel::as_ref(ctx))
                {
                    ctx.emit(CloudViewModelEvent::SortTimestampsChanged);
                }
            }
            CloudModelEvent::ObjectDeleted { folder_id, .. } => {
                if let Some(folder_id) = folder_id
                    && self.invalidate_folder_timestamps(folder_id, CloudModel::as_ref(ctx))
                {
                    ctx.emit(CloudViewModelEvent::SortTimestampsChanged);
                }
            }
            CloudModelEvent::ObjectForceExpanded { .. }
            | CloudModelEvent::EnvironmentLastTaskRunTimestampsUpdated => (),
        }
    }

    /// Invalidate all cached timestamps for the object with the given ID, and its parents.
    fn invalidate_object_timestamps(&mut self, uid: &ObjectUid, cloud_model: &CloudModel) -> bool {
        let Some(object) = cloud_model.get_by_uid(uid) else {
            return false;
        };
        let folder: Option<&CloudFolder> = object.into();
        match folder {
            Some(folder) => self.invalidate_folder_timestamps(&folder.id, cloud_model),
            None => {
                if let Some(parent_id) = object.metadata().folder_id {
                    self.invalidate_folder_timestamps(&parent_id, cloud_model)
                } else {
                    false
                }
            }
        }
    }

    /// Invalidate all cached timestamps for the given folder and its parents.
    fn invalidate_folder_timestamps(
        &mut self,
        folder_id: &SyncId,
        cloud_model: &CloudModel,
    ) -> bool {
        let had_revision_ts = self
            .folder_timestamp_cache
            .borrow_mut()
            .remove(folder_id)
            .is_some();

        let had_parent_ts = cloud_model
            .get_folder(folder_id)
            .and_then(|folder| folder.metadata().folder_id.as_ref())
            .is_some_and(|parent| self.invalidate_folder_timestamps(parent, cloud_model));
        had_revision_ts || had_parent_ts
    }
}

impl Entity for CloudViewModel {
    type Event = CloudViewModelEvent;
}

/// Mark CloudViewModel as global application state.
impl SingletonEntity for CloudViewModel {}
