use std::fmt;

use itertools::Itertools;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::cloud_object::ObjectIdType;

/// Convert ID enums into and from a hashed UUID.
pub trait HashableId: Sized + Send + Sync {
    fn to_hash(&self) -> String;
    fn from_hash(hash: &str) -> Option<Self>;
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(description = "A client-generated unique identifier.")]
pub struct ClientId(Uuid);

impl HashableId for ClientId {
    fn to_hash(&self) -> String {
        self.to_string()
    }

    fn from_hash(hash: &str) -> Option<ClientId> {
        hash.strip_prefix("Client-")
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(ClientId)
    }
}

impl ClientId {
    pub fn new() -> ClientId {
        Self(Uuid::new_v4())
    }

    pub fn sqlite_hash(&self) -> String {
        self.to_string()
    }
}

impl Default for ClientId {
    fn default() -> Self {
        ClientId::new()
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Client-{}", self.0)
    }
}

impl From<String> for ClientId {
    fn from(s: String) -> Self {
        ClientId::from_hash(&s).unwrap_or_default()
    }
}

/// ID of a cloud object. Every object is created on this machine, so the id is always a
/// client-generated UUID.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(description = "Identifier for a locally stored object.")]
pub struct SyncId(ClientId);

impl SyncId {
    pub fn uid(&self) -> ObjectUid {
        self.0.to_string()
    }

    pub fn sqlite_uid_hash(&self) -> String {
        self.0.sqlite_hash()
    }

    pub fn client_id(self) -> ClientId {
        self.0
    }
}

impl settings_value::SettingsValue for SyncId {}

impl fmt::Display for SyncId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<ClientId> for SyncId {
    fn from(id: ClientId) -> SyncId {
        SyncId(id)
    }
}

/// Creates a deterministic id from an i64 for use in tests: 123 becomes
/// `Client-00000000-0000-0000-0000-00000000007b`.
#[cfg(any(test, feature = "test-util"))]
impl From<i64> for SyncId {
    fn from(id: i64) -> Self {
        SyncId(ClientId(Uuid::from_u128(id.unsigned_abs().into())))
    }
}

/// Serializes as the `Client-{uuid}` hash, the form that is stored in sqlite and app state.
impl Serialize for SyncId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.to_hash().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SyncId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        ClientId::from_hash(s.as_str())
            .map(SyncId)
            .ok_or_else(|| serde::de::Error::custom(format!("not a client id: {s}")))
    }
}

/// Length of the ServerId, should be in sync with the length picked for the server.
const SERVER_ID_LENGTH: usize = 22;

/// ServerId is a representation of a string-based unique ID we generate on the server,
/// of length SERVER_ID_LENGTH.
/// Because it's of fixed length, it can implement the Copy trait
/// (in contrast to simply using a String type).
#[derive(Clone, Copy, Default, Hash, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(description = "A server-assigned unique identifier.")]
pub struct ServerId([char; SERVER_ID_LENGTH]);

/// For server IDs, this is the value that is stored
/// in the database. For client IDs, it is of the form "Client-{id}".
/// Used to index into cloud model and in most object read, write, and metadata
/// mutation server APIs.
pub type ObjectUid = String;

/// Corresponds to what is stored for a given object id within the local sqlite
/// database. Needed for backwards compatibility of the sqlite db following a refactor
/// that stripped the object type away from SyncID.
///
/// Of the format {sqlite_prefix}-{uid}.
///
/// Other than sqlite model events, this id is used for embedded objects within notebooks.
pub type HashedSqliteId = String;

#[derive(Debug, thiserror::Error)]
pub enum ParseServerIdError {
    #[error("ServerId must be exactly {SERVER_ID_LENGTH} characters, got {len}")]
    InvalidLength { len: usize },
}

/// Removes the prefix from sqlite IDs to extract the UIDs. Should not be used unless there
/// is not other way to cleanly do the conversion, i.e., when we don't know the ID type.
#[allow(clippy::result_unit_err)]
pub fn parse_sqlite_id_to_uid(hashed_sqlite_id: HashedSqliteId) -> Result<ObjectUid, ()> {
    let Some(uid) = hashed_sqlite_id.split("-").last() else {
        return Err(());
    };

    Ok(uid.to_owned())
}

impl ServerId {
    /// Convert a string input to a server ID. If the string is not exactly
    /// [`SERVER_ID_LENGTH`] characters long, it will be truncated or padded as
    /// necessary.
    pub fn from_string_lossy(id: impl AsRef<str>) -> Self {
        let id = id.as_ref();
        Self::try_from(id).unwrap_or_else(|err| {
            if cfg!(debug_assertions) {
                panic!("{err}");
            }
            // ServerIds need to be exactly 22 characters, so to prevent a crash, we'll normalize
            // the string. Nothing that uses it will work, but it's better than crashing.
            let normalized = Self::normalize_id_str(id, 0);
            Self::try_from(normalized).expect("id should convert")
        })
    }

    /// Normalizes a string to be exactly 22 characters long.
    fn normalize_id_str(input: &str, prefix_length: usize) -> String {
        let available_len = SERVER_ID_LENGTH - prefix_length;
        let truncated = if input.len() > available_len {
            &input[input.len() - available_len..]
        } else {
            input
        };
        format!("{truncated:0>available_len$}")
    }

    pub fn uid(&self) -> ObjectUid {
        (*self).into()
    }

    /// We need this API for backwards compatibility with local sqlite data.
    /// In sqlite, objects are stored in object typy, uid pairs of the format
    /// {sqlite-prefix}-{uid}. For example, for a workflow this would be
    /// "Workflow-{uid}".
    pub fn sqlite_type_and_uid_hash(&self, object_id_type: ObjectIdType) -> HashedSqliteId {
        format!("{}-{}", object_id_type.sqlite_prefix(), self)
    }
}

impl TryFrom<&str> for ServerId {
    type Error = ParseServerIdError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.chars().collect_array() {
            Some(chars) => Ok(Self(chars)),
            None => Err(ParseServerIdError::InvalidLength {
                len: s.chars().count(),
            }),
        }
    }
}

impl TryFrom<String> for ServerId {
    type Error = ParseServerIdError;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::try_from(id.as_str())
    }
}

/// Creates a conversion between an i64 and a corresponding deterministic ServerId for use in tests.
/// An i64 like 123 will be converted to "test_uid00000000000123".
#[cfg(any(test, feature = "test-util"))]
impl From<i64> for ServerId {
    fn from(id: i64) -> Self {
        let prefix = "test_uid";
        let id_str = id.abs().to_string();
        let normalized = format!(
            "{}{}",
            prefix,
            Self::normalize_id_str(&id_str, prefix.len())
        );
        Self::try_from(normalized).expect("normalized string should always be valid")
    }
}

impl From<ServerId> for String {
    fn from(id: ServerId) -> String {
        String::from_iter(id.0)
    }
}

/// We need our own implementation of serialize, due to ServerId being essentially a char array.
/// The default serializer in this case would spit a string that looks like an array, instead of a
/// nicely formatted string that we want. This implementation would serialize ServerId('a', 'b') to
/// "ab" instead.
impl Serialize for ServerId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s: String = (*self).into();
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for ServerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        ServerId::try_from(s.as_str()).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for ServerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        use std::fmt::Write;
        for ch in self.0.iter() {
            f.write_char(*ch)?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for ServerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "ServerId({self})")
    }
}

/// string_id_traits is a macro used for generating implementations for the type aliases on
/// ServerId, implements different To/From and Display, and HashableId traits.
/// Takes type and desired prefix for HashableId.
#[macro_export]
macro_rules! server_id_traits {
    ($t:ty, $prefix:literal) => {
        #[cfg(any(test, feature = "test-util"))]
        impl From<i64> for $t {
            fn from(id: i64) -> Self {
                Self(id.into())
            }
        }

        impl From<String> for $t {
            fn from(id: String) -> Self {
                Self($crate::ids::ServerId::from_string_lossy(id))
            }
        }

        impl From<$t> for String {
            fn from(id: $t) -> String {
                id.0.into()
            }
        }

        impl std::fmt::Display for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
                write!(f, "{}", self.0)
            }
        }

        impl From<$t> for $crate::ids::ServerId {
            fn from(id: $t) -> Self {
                id.0
            }
        }

        impl $crate::ids::HashableId for $t {
            fn to_hash(&self) -> String {
                format!("{}-{}", $prefix, self)
            }

            fn from_hash(hash: &str) -> Option<$t> {
                hash.strip_prefix(&format!("{}-", $prefix))
                    .map(|s| s.to_string().into())
            }
        }

        impl From<$crate::ids::ServerId> for $t {
            fn from(id: $crate::ids::ServerId) -> Self {
                Self(id)
            }
        }
    };
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash, Default)]
pub struct FolderId(ServerId);
server_id_traits! { FolderId, "Folder" }

/// Object ID type that is common for all generic string objects.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct GenericStringObjectId(ServerId);
crate::server_id_traits! { GenericStringObjectId, "GenericStringObject" }
