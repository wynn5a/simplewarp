#[cfg(not(target_family = "wasm"))]
pub mod persistence;

use std::fmt::Debug;

use anyhow::Result;
use cloud_objects::cloud_object::{JsonObjectType, SerializedModel, Serializer};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A JSON-backed cloud object payload.
pub trait JsonModel: Clone + Debug + Send + Sync + Serialize + DeserializeOwned + 'static {
    /// Returns the JSON object type used by the generic string object API.
    fn json_object_type() -> JsonObjectType;
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct JsonSerializer;

impl<M: JsonModel> Serializer<M> for JsonSerializer {
    fn serialize(model: &M) -> SerializedModel {
        SerializedModel::new(serde_json::to_string(model).expect("model should serialize"))
    }

    fn deserialize_owned(serialized: &str) -> Result<M>
    where
        Self: Sized,
    {
        Ok(serde_json::from_str(serialized)?)
    }
}
