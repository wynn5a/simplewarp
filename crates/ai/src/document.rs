use uuid::Uuid;

/// The id of an AI document (plan). Plans are gone; notebooks that were once plans still carry
/// this id, so the type stays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AIDocumentId(Uuid);

impl AIDocumentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for AIDocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for AIDocumentId {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::try_parse(&value)?))
    }
}

impl TryFrom<&str> for AIDocumentId {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::try_parse(value)?))
    }
}
