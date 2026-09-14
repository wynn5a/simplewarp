pub use cloud_object_models::{AgentConfig, CloudAgentConfig};

use crate::cloud_object::model::generic_string_model::StringModel;
use crate::cloud_object::model::json_model::JsonModel;
use crate::cloud_object::{
    GenericStringObjectFormat, GenericStringObjectUniqueKey, JsonObjectType,
};

impl StringModel for AgentConfig {
    type CloudObjectType = CloudAgentConfig;

    fn model_type_name(&self) -> &'static str {
        "Cloud agent config"
    }

    fn should_enforce_revisions() -> bool {
        true
    }

    fn model_format() -> GenericStringObjectFormat {
        GenericStringObjectFormat::Json(JsonObjectType::CloudAgentConfig)
    }

    fn display_name(&self) -> String {
        self.name.clone()
    }

    fn uniqueness_key(&self) -> Option<GenericStringObjectUniqueKey> {
        None
    }

    fn should_show_activity_toasts() -> bool {
        false
    }

    fn warn_if_unsaved_at_quit() -> bool {
        true
    }
}

impl JsonModel for AgentConfig {
    fn json_object_type() -> JsonObjectType {
        JsonObjectType::CloudAgentConfig
    }
}
