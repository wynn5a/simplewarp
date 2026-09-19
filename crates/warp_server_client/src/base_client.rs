use std::sync::Arc;

use anyhow::Result;
use warp_graphql::client::RequestOptions;
use warp_server_auth::auth_state::AuthState;
use warp_server_auth::credentials::AuthToken;
#[cfg(feature = "agent_mode_evals")]
use warp_server_auth::credentials::Credentials;

use crate::auth::{AuthEvent, AuthSession, UserUid};

/// IDs in the staging database that were created specifically for evals.
///
/// Keep this list in sync with `script/populate_agent_mode_eval_user.sql` in warp-server.
#[cfg(feature = "agent_mode_evals")]
const EVAL_USER_IDS: [i32; 11] = [
    2162, 2164, 2165, 2166, 2167, 2168, 2169, 2172, 2173, 2174, 2175,
];

/// Provides GraphQL path routing that applies independently of authentication.
#[derive(Clone, Debug, Default)]
pub struct GraphqlRoutingConfig {
    pub path_prefix: Option<String>,
}

/// Owns shared transport, authentication, and authenticated request decoration.
pub struct BaseClient {
    client: Arc<http_client::Client>,
    auth_state: Arc<AuthState>,
    auth_session: Arc<AuthSession>,
    graphql_routing: GraphqlRoutingConfig,
}

impl BaseClient {
    pub fn new(
        client: Arc<http_client::Client>,
        auth_state: Arc<AuthState>,
        event_sender: async_channel::Sender<AuthEvent>,
        graphql_routing: GraphqlRoutingConfig,
    ) -> Self {
        // We generate one random user ID per client so evals can run in parallel.
        #[cfg(feature = "agent_mode_evals")]
        let eval_user_id = {
            use rand::Rng as _;

            Some(EVAL_USER_IDS[rand::thread_rng().gen_range(0..EVAL_USER_IDS.len())])
        };
        #[cfg(feature = "agent_mode_evals")]
        if let Some(eval_user_id) = eval_user_id {
            // Set a deterministic per-user API key so all requests — including
            // REST endpoints like the SSE event stream — carry a real
            // Authorization header. The key format mirrors what SeedEvalAPIKeys()
            // inserts in warp-server at eval startup:
            // wk-1.<user_id as 64-char zero-padded lowercase hex>.
            let eval_key = format!("wk-1.{eval_user_id:0>64x}");
            auth_state.set_credentials(Some(Credentials::ApiKey {
                key: eval_key,
                owner_type: None,
            }));
        }
        let auth_session = Arc::new(AuthSession::new(
            client.clone(),
            auth_state.clone(),
            event_sender,
        ));
        Self {
            client,
            auth_state,
            auth_session,
            graphql_routing,
        }
    }

    /// Returns an owned handle to the shared HTTP client for GraphQL operations.
    pub fn owned_http_client(&self) -> Arc<http_client::Client> {
        self.client.clone()
    }

    pub fn auth_session(&self) -> Arc<AuthSession> {
        self.auth_session.clone()
    }

    pub fn anonymous_id(&self) -> String {
        self.auth_state.anonymous_id()
    }

    pub fn user_id(&self) -> Option<UserUid> {
        self.auth_state.user_id()
    }

    pub async fn get_or_refresh_access_token(&self) -> Result<AuthToken> {
        self.auth_session.get_or_refresh_access_token().await
    }

    /// Returns GraphQL options for bootstrap or explicit-token operations.
    pub fn graphql_request_options_with_token(&self, auth_token: Option<String>) -> RequestOptions {
        RequestOptions {
            auth_token,
            path_prefix: self.graphql_routing.path_prefix.clone(),
            ..RequestOptions::default()
        }
    }
}

#[cfg(test)]
#[path = "base_client_tests.rs"]
mod tests;
