pub use cloud_object_models::{CloudPreference, CloudPreferenceModel, Platform, Preference};

use crate::cloud_object::model::generic_string_model::StringModel;
use crate::cloud_object::model::json_model::JsonModel;
use crate::cloud_object::{
    GenericStringObjectFormat, GenericStringObjectUniqueKey, JsonObjectType, UniquePer,
};

/// Keeps cloud preference objects persisted by older builds loadable into the cloud model.
impl StringModel for Preference {
    type CloudObjectType = CloudPreference;

    fn model_type_name(&self) -> &'static str {
        "Preference"
    }

    fn should_enforce_revisions() -> bool {
        // Last write wins for cloud prefs
        false
    }

    fn should_show_activity_toasts() -> bool {
        // No update toasts for cloud prefs
        false
    }

    fn warn_if_unsaved_at_quit() -> bool {
        // Don't block quitting on unsaved cloud prefs changes
        false
    }

    fn model_format() -> GenericStringObjectFormat {
        GenericStringObjectFormat::Json(Self::json_object_type())
    }

    fn display_name(&self) -> String {
        self.model_type_name().to_owned()
    }

    fn should_clear_on_unique_key_conflict(&self) -> bool {
        true
    }

    fn uniqueness_key(&self) -> Option<GenericStringObjectUniqueKey> {
        Some(GenericStringObjectUniqueKey {
            key: format!("{}_{}", self.platform, self.storage_key),
            unique_per: UniquePer::User,
        })
    }
}

impl JsonModel for Preference {
    fn json_object_type() -> JsonObjectType {
        JsonObjectType::Preference
    }
}
