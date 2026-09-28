//! Representation of Warp user credentials.
//!
//! The primary representation is [`Credentials`], which is the source of truth for how a user is
//! authenticated to Warp. There is no login, so these variants exist only for tests.

/// Represents the different ways a user can authenticate with Warp.
#[derive(Clone, Debug)]
pub enum Credentials {
    /// Authentication derived from an ambient browser session cookie.
    SessionCookie,
    /// Test credentials used in unit tests and integration tests.
    #[cfg(any(test, feature = "integration_tests"))]
    Test,
}
