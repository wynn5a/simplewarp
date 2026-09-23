use std::fmt::{self, Debug};
use std::marker::PhantomData;
use std::sync::Arc;

use super::{ServerMetadata, ServerPermissions};
use crate::ids::SyncId;

#[derive(Clone, Debug, Default)]
pub enum ConflictStatus<T> {
    #[default]
    NoConflicts,
    ConflictingChanges {
        object: Arc<T>,
    },
}

impl<T> ConflictStatus<T> {
    /// Utility function that allows for a more ergonomic way of figuring out whether there is a
    /// conflict (for cases where we don't care about the conflict details).
    pub fn has_conflicts(&self) -> bool {
        matches!(self, ConflictStatus::ConflictingChanges { .. })
    }
}

/// An object that maps directly to the data returned from the server
/// for a given model and id type.
pub struct GenericServerObject<K, M> {
    pub id: SyncId,
    pub model: M,
    pub metadata: ServerMetadata,
    pub permissions: ServerPermissions,
    _marker: PhantomData<fn() -> K>,
}

impl<K, M> Clone for GenericServerObject<K, M>
where
    M: Clone,
{
    fn clone(&self) -> Self {
        Self::new(
            self.id,
            self.model.clone(),
            self.metadata.clone(),
            self.permissions.clone(),
        )
    }
}

impl<K, M> Debug for GenericServerObject<K, M>
where
    M: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericServerObject")
            .field("id", &self.id)
            .field("model", &self.model)
            .field("metadata", &self.metadata)
            .field("permissions", &self.permissions)
            .finish()
    }
}

impl<K, M> GenericServerObject<K, M> {
    /// Constructs a server object from its server-provided parts.
    pub fn new(
        id: SyncId,
        model: M,
        metadata: ServerMetadata,
        permissions: ServerPermissions,
    ) -> Self {
        Self {
            id,
            model,
            metadata,
            permissions,
            _marker: PhantomData,
        }
    }
}
