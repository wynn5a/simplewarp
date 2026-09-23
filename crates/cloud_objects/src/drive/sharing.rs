use serde::{Deserialize, Serialize};
use session_sharing_protocol::common::{ProfileData as SessionSharingProfileData, Role};
use warp_graphql::object_permissions::AccessLevel;

use crate::auth::UserUid;

#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SharingAccessLevel {
    View,
    Edit,
    Full,
}

impl SharingAccessLevel {
    /// Whether or not this access level implies the `ChangeOwner` action.
    pub fn can_move_drive(self) -> bool {
        self >= SharingAccessLevel::Full
    }
}

impl From<AccessLevel> for SharingAccessLevel {
    fn from(server_access: AccessLevel) -> Self {
        match server_access {
            AccessLevel::Viewer => Self::View,
            AccessLevel::Editor => Self::Edit,
            AccessLevel::Full => Self::Full,
        }
    }
}

impl From<SharingAccessLevel> for AccessLevel {
    fn from(val: SharingAccessLevel) -> Self {
        match val {
            SharingAccessLevel::View => AccessLevel::Viewer,
            SharingAccessLevel::Edit => AccessLevel::Editor,
            SharingAccessLevel::Full => AccessLevel::Full,
        }
    }
}

impl From<Role> for SharingAccessLevel {
    fn from(role: Role) -> Self {
        match role {
            Role::Reader => Self::View,
            Role::Executor => Self::Edit,
            Role::Full => Self::Full,
        }
    }
}

impl From<SharingAccessLevel> for Role {
    fn from(access_level: SharingAccessLevel) -> Self {
        match access_level {
            SharingAccessLevel::View => Self::Reader,
            SharingAccessLevel::Edit | SharingAccessLevel::Full => Self::Executor,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LinkSharingSubjectType {
    None,
    Anyone,
}
/// A `Subject` is someone with access to a shared object, like its owner or a directly-added
/// guest.
#[derive(Debug, Clone, PartialEq)]
pub enum Subject {
    User(UserKind),
}

/// A kind of user. In all cases, there is an underlying Warp account, but it's represented
/// differently in certain cases.
#[derive(Debug, Clone)]
pub enum UserKind {
    /// A Warp user account, tracked in the [`UserProfiles`] model.
    Account(UserUid),
    /// A session-sharing participant.
    // TODO(CLD-2283): Remove this once we have Firebase UIDs for shared session participants.
    SharedSessionParticipant(SessionSharingProfileData),
}

impl Subject {
    /// Checks if this subject refers to a given Firebase user directly.
    pub fn is_user(&self, other_uid: UserUid) -> bool {
        match self {
            Subject::User(UserKind::Account(user_uid)) => *user_uid == other_uid,
            Subject::User(UserKind::SharedSessionParticipant(profile_data)) => {
                profile_data.firebase_uid.as_str() == other_uid.as_str()
            }
        }
    }
}

impl PartialEq for UserKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Account(self_uid), Self::Account(other_uid)) => self_uid == other_uid,
            // Shared session participant data does not implement `PartialEq`. We only compare
            // `UserKind`s in tests, so support isn't yet needed.
            _ => false,
        }
    }
}
