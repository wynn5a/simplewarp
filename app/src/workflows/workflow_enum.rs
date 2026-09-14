pub use cloud_object_models::{
    CloudWorkflowEnum, CloudWorkflowEnumModel, EnumVariants, WorkflowEnum,
};

use crate::cloud_object::model::generic_string_model::StringModel;
use crate::cloud_object::model::json_model::JsonModel;
use crate::cloud_object::{
    GenericStringObjectFormat, GenericStringObjectUniqueKey, JsonObjectType,
};

impl StringModel for WorkflowEnum {
    type CloudObjectType = CloudWorkflowEnum;

    fn model_type_name(&self) -> &'static str {
        "WorkflowEnum"
    }

    fn should_enforce_revisions() -> bool {
        true
    }

    fn model_format() -> GenericStringObjectFormat {
        GenericStringObjectFormat::Json(Self::json_object_type())
    }

    fn should_show_activity_toasts() -> bool {
        false
    }

    fn warn_if_unsaved_at_quit() -> bool {
        true
    }

    fn display_name(&self) -> String {
        self.model_type_name().to_owned()
    }

    fn uniqueness_key(&self) -> Option<GenericStringObjectUniqueKey> {
        None
    }
}

impl JsonModel for WorkflowEnum {
    fn json_object_type() -> JsonObjectType {
        JsonObjectType::WorkflowEnum
    }
}
