//! This crate defines shared SQLite persistence infrastructure for Warp cloud objects.
//!
//! It owns model-agnostic persistence helpers for object metadata, permissions,
//! callback-based object upsert and delete
//! operations, and generic string object table access.
//!
//! It should not depend on `cloud_object_models`; model-specific read and write adapters
//! should live with the corresponding model modules.

mod objects;

pub use objects::{
    CloudObjectId, CloudObjectReadContext, CreateCloudObjectFn, DeleteCloudObjectFn,
    GenericStringObjectPersistenceData, GenericStringObjectRow, UpdateCloudObjectFn,
    delete_cloud_object, delete_generic_string_object, id_from_metadata,
    load_cloud_object_read_context, metadata_object_type_key, read_generic_string_object_rows,
    to_cloud_object_metadata, to_cloud_object_permissions, update_object_metadata,
    upsert_cloud_object, upsert_generic_string_objects,
};
