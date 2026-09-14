pub use cloud_object_models::{
    CloudTemplatableMCPServer, CloudTemplatableMCPServerModel, GalleryData, JsonTemplate,
    TemplatableMCPServer, TemplateVariable,
};
use warp_core::ui::appearance::Appearance;

use crate::cloud_object::model::generic_string_model::StringModel;
use crate::cloud_object::model::json_model::JsonModel;
use crate::cloud_object::{
    CloudObjectUuid, GenericStringObjectFormat, GenericStringObjectUniqueKey, JsonObjectType,
    UniquePer, WarpDriveItem,
};
use crate::server::ids::SyncId;

const UNIQUENESS_KEY_PREFIX: &str = "templatable_mcp_server";

impl CloudObjectUuid for TemplatableMCPServer {
    fn uuid(&self) -> uuid::Uuid {
        self.uuid
    }
}

impl StringModel for TemplatableMCPServer {
    type CloudObjectType = CloudTemplatableMCPServer;

    fn model_type_name(&self) -> &'static str {
        "MCP server"
    }

    fn should_enforce_revisions() -> bool {
        true
    }

    fn model_format() -> GenericStringObjectFormat {
        GenericStringObjectFormat::Json(JsonObjectType::TemplatableMCPServer)
    }

    fn should_show_activity_toasts() -> bool {
        true
    }

    fn warn_if_unsaved_at_quit() -> bool {
        true
    }

    fn display_name(&self) -> String {
        self.name.clone()
    }

    fn uniqueness_key(&self) -> Option<GenericStringObjectUniqueKey> {
        Some(GenericStringObjectUniqueKey {
            key: format!("{UNIQUENESS_KEY_PREFIX}_{}", self.uuid),
            unique_per: UniquePer::User,
        })
    }

    fn renders_in_warp_drive(&self) -> bool {
        false
    }

    fn to_warp_drive_item(
        &self,
        _id: SyncId,
        _appearance: &Appearance,
        _templatable_mcp_server: &CloudTemplatableMCPServer,
    ) -> Option<Box<dyn WarpDriveItem>> {
        None
    }
}

impl JsonModel for TemplatableMCPServer {
    fn json_object_type() -> JsonObjectType {
        JsonObjectType::TemplatableMCPServer
    }
}
