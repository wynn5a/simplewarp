//! Persistence glue for previously synced scheduled-agent objects: the sync
//! that produced them is gone, but locally persisted rows must still
//! deserialize. No new objects are ever created in this build.

pub use cloud_object_models::{CloudScheduledAmbientAgent, ScheduledAmbientAgent};

use crate::cloud_object::model::generic_string_model::StringModel;
use crate::cloud_object::model::json_model::JsonModel;
use crate::cloud_object::{
    GenericStringObjectFormat, GenericStringObjectUniqueKey, JsonObjectType,
};

impl StringModel for ScheduledAmbientAgent {
    type CloudObjectType = CloudScheduledAmbientAgent;

    fn model_type_name(&self) -> &'static str {
        "Scheduled ambient agent"
    }

    fn should_enforce_revisions() -> bool {
        true
    }

    fn model_format() -> GenericStringObjectFormat {
        GenericStringObjectFormat::Json(JsonObjectType::ScheduledAmbientAgent)
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

impl JsonModel for ScheduledAmbientAgent {
    fn json_object_type() -> JsonObjectType {
        JsonObjectType::ScheduledAmbientAgent
    }
}
