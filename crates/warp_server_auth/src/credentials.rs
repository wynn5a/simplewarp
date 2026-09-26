//! Representation of Warp user credentials.
//!
//! The primary representation is [`Credentials`], which is the source of truth for how a user is
//! authenticated to Warp. There is no login: the only real credential is the remote-server
//! daemon's bearer token; the remaining variants exist for tests.

/// Represents the different ways a user can authenticate with Warp.
#[derive(Clone, Debug)]
pub enum Credentials {
    /// Request-scoped or externally managed bearer token.
    Bearer(String),
    /// Authentication derived from an ambient browser session cookie.
    SessionCookie,
    /// Test credentials used in unit tests, integration tests, and skip_login builds.
    #[cfg(any(test, feature = "integration_tests", feature = "skip_login"))]
    Test,
}

impl Credentials {
    /// Returns the bearer token to use in an Authorization header, or `None` if these
    /// credentials are not header-based.
    pub fn bearer_token(&self) -> Option<&str> {
        match self {
            Credentials::Bearer(token) => Some(token),
            Credentials::SessionCookie => None,
            #[cfg(any(test, feature = "integration_tests", feature = "skip_login"))]
            Credentials::Test => None,
        }
    }
}
