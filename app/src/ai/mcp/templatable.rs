pub use cloud_object_models::{
    CloudTemplatableMCPServer, CloudTemplatableMCPServerModel, JsonTemplate, TemplatableMCPServer,
    TemplateVariable,
};

use crate::cloud_object::model::generic_string_model::StringModel;
use crate::cloud_object::model::json_model::JsonModel;
use crate::cloud_object::{CloudObjectUuid, GenericStringObjectFormat, JsonObjectType};

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
}

impl JsonModel for TemplatableMCPServer {
    fn json_object_type() -> JsonObjectType {
        JsonObjectType::TemplatableMCPServer
    }
}
