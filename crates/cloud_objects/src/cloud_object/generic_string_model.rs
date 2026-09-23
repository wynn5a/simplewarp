use std::fmt::Debug;
use std::marker::PhantomData;

use anyhow::Result;

use super::SerializedModel;

/// A serializer goes from a model to a string and back.
pub trait Serializer<M>: Debug + Clone + 'static {
    fn serialize(model: &M) -> SerializedModel;
    fn deserialize_owned(serialized: &str) -> Result<M>
    where
        Self: Sized;
}

/// A `GenericStringModel` is a generic implementation of model types that can serialize to/from string.
/// given a particular serializer.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GenericStringModel<M, S>
where
    S: Serializer<M>,
{
    pub string_model: M,
    _serializer: PhantomData<fn() -> S>,
}

impl<M, S> GenericStringModel<M, S>
where
    S: Serializer<M>,
{
    pub fn deserialize_owned(serialized: &str) -> Result<Self> {
        S::deserialize_owned(serialized).map(Self::new)
    }

    pub fn new(model: M) -> Self {
        Self {
            string_model: model,
            _serializer: PhantomData,
        }
    }
}
