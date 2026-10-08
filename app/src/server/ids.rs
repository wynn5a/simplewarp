// Re-export types from cloud_objects.
#[allow(unused_imports)]
pub use cloud_objects::ids::GenericStringObjectId;
#[allow(unused_imports)]
pub use cloud_objects::ids::{
    ClientId, HashableId, HashedSqliteId, ObjectUid, ServerId, SyncId, parse_sqlite_id_to_uid,
};

#[cfg(test)]
#[path = "ids_tests.rs"]
mod tests;
