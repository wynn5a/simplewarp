// Re-export types from cloud_objects.
pub use cloud_objects::drive::sharing::SharingAccessLevel;

/// Whether not a shared object's contents are editable by the current user.
///
/// This is not purely a function of their access level since anonymous users are not allowed to
/// edit (due to the lack of attribution).
#[derive(Debug, Clone, Copy)]
pub enum ContentEditability {
    ReadOnly,
    RequiresLogin,
    Editable,
}

impl ContentEditability {
    pub fn can_edit(self) -> bool {
        matches!(self, ContentEditability::Editable)
    }
}
