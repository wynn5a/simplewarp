use warpui::AppContext;

use super::{CloudObject, Space};
use crate::cloud_object::WarpDriveItemId;
use crate::cloud_object::folders::CloudFolder;
use crate::drive::CloudObjectTypeAndId;
use crate::ui_components::breadcrumb::Breadcrumb;

// Encapsulates an object that can contain other objects, and keeps
// information necessary to show breadcrumbs.
#[derive(Clone, Debug)]
pub struct ContainingObject {
    pub name: String,
    pub kind: ContainingObjectKind,
    /// Whether clicking this breadcrumb to view the object in Warp Drive is worth wiring up.
    /// Defaults to `true`; callers building a *clickable* breadcrumb trail should call
    /// [`ContainingObject::disable_drive_link`] when Warp Drive itself isn't available (e.g. no
    /// account), since the click would only land on Drive's "sign in" dead end. Kept out of
    /// `containing_objects_path` itself (which has no `WarpDriveSettings` dependency and is also
    /// used by plain-text callers like `breadcrumbs()`) so it stays a pure data lookup.
    drive_viewable: bool,
}

impl Breadcrumb for ContainingObject {
    fn label(&self) -> String {
        self.name.clone()
    }

    fn enabled(&self) -> bool {
        self.drive_viewable
    }
}

impl ContainingObject {
    /// Marks this breadcrumb as not worth clicking through to Warp Drive.
    pub fn disable_drive_link(&mut self) {
        self.drive_viewable = false;
    }
}

impl From<&CloudFolder> for ContainingObject {
    fn from(folder: &CloudFolder) -> Self {
        Self {
            name: folder.display_name().clone(),
            kind: ContainingObjectKind::Object(CloudObjectTypeAndId::Folder(folder.id)),
            drive_viewable: true,
        }
    }
}

impl Space {
    pub fn into_containing_object(self, app: &AppContext) -> ContainingObject {
        ContainingObject {
            name: self.name(app).clone(),
            kind: ContainingObjectKind::Space(self),
            drive_viewable: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ContainingObjectKind {
    Space(Space),
    Object(CloudObjectTypeAndId),
}

impl ContainingObjectKind {
    pub fn into_item_id(self) -> WarpDriveItemId {
        match self {
            ContainingObjectKind::Space(space) => WarpDriveItemId::Space(space),
            ContainingObjectKind::Object(object) => WarpDriveItemId::Object(object),
        }
    }
}
