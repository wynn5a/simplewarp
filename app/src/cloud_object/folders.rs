use async_trait::async_trait;
pub use cloud_object_models::{CloudFolder, CloudFolderModel};
use cloud_objects::cloud_object::SerializedModel;
pub use cloud_objects::ids::FolderId;

use super::{CloudModelType, CloudObjectUpsertParams, ObjectType};
use crate::cloud_object::CloudObjectTypeAndId;
use crate::persistence::ModelEvent;
use crate::server::ids::SyncId;

#[async_trait]
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

    fn serialized(&self) -> SerializedModel {
        SerializedModel::new(self.name.to_owned())
    }
}
