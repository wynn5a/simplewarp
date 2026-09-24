use crate::schema;

#[derive(cynic::Enum, Clone, Debug)]
pub enum AnonymousUserType {
    NativeClientAnonymousUser,
    NativeClientAnonymousUserFeatureGated,
    WebClientAnonymousUser,
    #[cynic(fallback)]
    Other(String),
}
