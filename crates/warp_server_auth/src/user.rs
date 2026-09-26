use serde::{Deserialize, Serialize};

use super::UserUid;
pub use super::user_uid::{TEST_USER_EMAIL, TEST_USER_UID};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnonymousUserType {
    /// An anonymous user created from the native client.
    NativeClientAnonymousUser,
    /// An anonymous user created from the native client with feature (rather than time-based) gating.
    NativeClientAnonymousUserFeatureGated,
    /// An anonymous user created from the web client.
    WebClientAnonymousUser,
}

/// Type of principal making the authenticated request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrincipalType {
    #[default]
    User,
    ServiceAccount,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct PersonalObjectLimits {
    pub env_var_limit: usize,
    pub notebook_limit: usize,
    pub workflow_limit: usize,
}

/// The in-memory representation of a logged-in User.
/// This does not include authentication credentials, which are stored separately
/// in the `Credentials` enum.
#[derive(Debug, Clone)]
pub struct User {
    /// The Firebase UID of this user.
    pub local_id: UserUid,
    /// Metadata about the user.
    pub metadata: UserMetadata,
    /// Whether or not the user is onboarded.
    pub is_onboarded: bool,
    /// Whether or not the user needs to link their account via SSO due to an organization setting.
    pub needs_sso_link: bool,
    /// What type of anonymous user this user is. May be `None` if they are not anonymous.
    pub anonymous_user_type: Option<AnonymousUserType>,
    /// Whether or not this user is on what we consider a "work" domain, meaning the domain isn't
    /// from a general email provider (e.g. gmail.com, hotmail.com, proton.me, etc.).
    /// Calculated on warp-server.
    pub is_on_work_domain: bool,
    pub personal_object_limits: Option<PersonalObjectLimits>,
    /// Type of principal (user or service account). Fetched fresh from the server
    /// on each login/refresh.
    pub principal_type: PrincipalType,
    /// Skill specs that should be available to this principal in every agent run.
    pub global_skills: Vec<String>,
}

/// This struct holds extra information about the user. Most of this information comes directly
/// from Firebase.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserMetadata {
    /// The user's email. NOTE: unlike other fields which use `Option`s to denote null values,
    /// an anonymous user will have an empty string as their email here.
    pub email: String,
    /// The user's display name from Firebase. We should prefer showing this over their email, if
    /// we can. Typically this is only populated when using a non-email provider like GitHub.
    pub display_name: Option<String>,
    /// A URL for their profile picture.
    pub photo_url: Option<String>,
}

impl User {
    /// The name for the user that we display. This is the user's display name, if set. If not set,
    /// we then fallback to email (which is always set).
    pub fn username_for_display(&self) -> &str {
        let user_metadata = &self.metadata;
        user_metadata
            .display_name
            .as_deref()
            .unwrap_or(user_metadata.email.as_str())
    }

    /// The display name of the user. Does not fall back to email.
    pub fn display_name(&self) -> Option<String> {
        self.metadata.display_name.clone()
    }

    pub fn test() -> Self {
        Self {
            local_id: UserUid::new(TEST_USER_UID),
            metadata: UserMetadata {
                email: TEST_USER_EMAIL.to_string(),
                display_name: None,
                photo_url: None,
            },
            is_onboarded: true,
            needs_sso_link: false,
            anonymous_user_type: None,
            is_on_work_domain: false,
            personal_object_limits: None,
            principal_type: PrincipalType::User,
            global_skills: Vec::new(),
        }
    }

    pub fn is_user_anonymous(&self) -> bool {
        self.anonymous_user_type().is_some()
    }

    pub fn anonymous_user_type(&self) -> Option<AnonymousUserType> {
        self.anonymous_user_type
    }

    pub fn personal_object_limits(&self) -> Option<PersonalObjectLimits> {
        self.personal_object_limits
    }
}

#[cfg(test)]
#[path = "user_tests.rs"]
mod tests;
