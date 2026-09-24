pub use warp_server_auth::auth_client::{AuthClient, FetchUserResult, UserAuthenticationError};

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
