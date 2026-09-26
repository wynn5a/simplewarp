use serde::Serialize;
use serde::de::DeserializeOwned;

use super::generic_string_model::StringModel;
use crate::cloud_object::JsonObjectType;

/// A `JsonModel` is a string model that can be serialized to and deserialized from JSON.
pub trait JsonModel: StringModel + Serialize + DeserializeOwned + 'static {
    /// Returns the JsonObjectType for this model.
    fn json_object_type() -> JsonObjectType;
}
