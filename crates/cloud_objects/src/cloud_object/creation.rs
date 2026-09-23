use super::ServerPermissions;
use crate::ids::ServerIdAndType;

/// The creation-specific data returned by the server, which is inserted into CloudModel and persisted
/// just once.
#[derive(Debug, PartialEq, Clone)]
pub struct ServerCreationInfo {
    pub server_id_and_type: ServerIdAndType,
    pub creator_uid: Option<String>,
    pub permissions: ServerPermissions,
}
