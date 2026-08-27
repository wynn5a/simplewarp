#[cfg(any(test, feature = "test-util"))]
pub use warp_server_client::auth::MockAuthClient;
pub use warp_server_client::auth::{
    AuthClient, FetchUserResult, MintCustomTokenError, SyncedUserSettings, UserAuthenticationError,
};

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
