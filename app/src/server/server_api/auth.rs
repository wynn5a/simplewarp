pub use warp_server_client::auth::{AuthClient, FetchUserResult, UserAuthenticationError};

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
