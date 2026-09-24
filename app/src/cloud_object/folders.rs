use async_trait::async_trait;
pub use cloud_object_models::{CloudFolder, CloudFolderModel};
use cloud_objects::cloud_object::SerializedModel;
pub use cloud_objects::ids::FolderId;

use super::{CloudModelType, CloudObjectUpsertParams, ObjectType, Space};
use crate::appearance::Appearance;
use crate::cloud_object::WarpDriveItem;
use crate::drive::CloudObjectTypeAndId;
use crate::drive::items::folder::WarpDriveFolder;
use crate::persistence::ModelEvent;
use crate::server::ids::SyncId;

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl CloudModelType for CloudFolderModel {
    type CloudObjectType = CloudFolder;
    type IdType = FolderId;

    fn model_type_name(&self) -> &'static str {
        "Folder"
    }

    fn object_type(&self) -> ObjectType {
        ObjectType::Folder
    }

    fn cloud_object_type_and_id(&self, id: SyncId) -> CloudObjectTypeAndId {
        CloudObjectTypeAndId::Folder(id)
    }

    fn display_name(&self) -> String {
        self.name.clone()
    }

    fn upsert_event(params: CloudObjectUpsertParams<Self>) -> ModelEvent {
        ModelEvent::UpsertFolder {
            folder: CloudFolder::from(params),
        }
    }

    fn should_update_after_server_conflict(&self) -> bool {
        false
    }

    fn serialized(&self) -> SerializedModel {
        SerializedModel::new(self.name.to_owned())
    }

    fn can_move_to_space(&self, current_space: Space, new_space: Space) -> bool {
        // We don't currently support moving folders across spaces.
        current_space == new_space
    }
    fn renders_in_warp_drive(&self) -> bool {
        true
    }

    fn to_warp_drive_item(
        &self,
        id: SyncId,
        _appearance: &Appearance,
        folder: &CloudFolder,
    ) -> Option<Box<dyn WarpDriveItem>> {
        Some(Box::new(WarpDriveFolder::new(
            self.cloud_object_type_and_id(id),
            folder.clone(),
        )))
    }
}
