use serde::{Deserialize, Serialize};

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
}

impl Subject {
    /// Checks if this subject refers to a given Firebase user directly.
    pub fn is_user(&self, other_uid: UserUid) -> bool {
        match self {
            Subject::User(UserKind::Account(user_uid)) => *user_uid == other_uid,
        }
    }
}

impl PartialEq for UserKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Account(self_uid), Self::Account(other_uid)) => self_uid == other_uid,
        }
    }
}
