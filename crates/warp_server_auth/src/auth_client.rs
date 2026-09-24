use std::result::Result as StdResult;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};
use async_trait::async_trait;
use cynic::QueryBuilder;
use thiserror::Error;
use warp_errors::{AnyhowErrorExt, ErrorExt, register_error};
use warp_graphql::client::{Operation as _, RequestOptions};
use warp_graphql::queries::get_user::{GetUser, GetUserVariables, UserOutput as GqlUserOutput};

use crate::auth_state::AuthState;
use crate::credentials::{AuthToken, Credentials, LoginToken};
use crate::firebase::FirebaseError;
use crate::session::{AuthEvent, AuthSession};

/// Header key used to associate unauthenticated requests with an experiment identity.
pub const EXPERIMENT_ID_HEADER: &str = "X-Warp-Experiment-Id";

/// Protocol-level results of fetching the current user.
pub struct FetchUserResult {
    pub user_output: GqlUserOutput,
    /// The credentials used to authenticate this user.
    pub credentials: Credentials,
    /// Whether this attempt to fetch the user was for refreshing an existing logged-in user.
    pub from_refresh: bool,
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait AuthClient: Send + Sync {
    /// Returns the cached access token if it is still valid.
    ///
    /// If it has expired, this fetches a new access token using the user's refresh
    /// token, caches it, and then returns it. It may return an auth mode that does
    /// not require an Authorization header, such as session cookies or test credentials.
    async fn get_or_refresh_access_token(&self) -> Result<AuthToken>;

    /// Fetches the user's metadata and authentication tokens.
    async fn fetch_user(
        &self,
        token: LoginToken,
        for_refresh: bool,
    ) -> StdResult<FetchUserResult, UserAuthenticationError>;
}

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

/// Implements the [`AuthClient`] trait over the shared HTTP client and auth session.
pub struct AuthClientImpl {
    client: Arc<http_client::Client>,
    auth_state: Arc<AuthState>,
    auth_session: Arc<AuthSession>,
    graphql_routing: GraphqlRoutingConfig,
}

impl AuthClientImpl {
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

    async fn fetch_user_properties(&self, auth_token: Option<&str>) -> Result<GqlUserOutput> {
        let operation = GetUser::build(GetUserVariables {
            request_context: warp_graphql::client::get_request_context(),
        });
        let mut options = RequestOptions {
            auth_token: auth_token.map(ToOwned::to_owned),
            path_prefix: self.graphql_routing.path_prefix.clone(),
            ..RequestOptions::default()
        };
        options.headers.insert(
            EXPERIMENT_ID_HEADER.to_string(),
            self.auth_state.anonymous_id(),
        );
        let response = operation
            .send_request(self.client.clone(), options)
            .await?
            .data
            .ok_or_else(|| anyhow!("Expected valid response.data"))?;
        match response.user {
            warp_graphql::queries::get_user::UserResult::UserOutput(user_output) => Ok(user_output),
            warp_graphql::queries::get_user::UserResult::Unknown => {
                Err(anyhow!("Unable to fetch user"))
            }
        }
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl AuthClient for AuthClientImpl {
    async fn get_or_refresh_access_token(&self) -> Result<AuthToken> {
        self.auth_session.get_or_refresh_access_token().await
    }

    async fn fetch_user(
        &self,
        token: LoginToken,
        for_refresh: bool,
    ) -> StdResult<FetchUserResult, UserAuthenticationError> {
        let new_credentials = self.auth_session.exchange_credentials(token).await?;
        let auth_token = new_credentials.bearer_token();
        let user_output = self
            .fetch_user_properties(auth_token.as_bearer_token())
            .await
            .context("Failed to fetch user response data")
            .map_err(UserAuthenticationError::Unexpected)?;
        // Store the owner type if using an API key.
        let new_credentials = match new_credentials {
            Credentials::ApiKey { key, .. } => Credentials::ApiKey {
                key,
                owner_type: user_output.api_key_owner_type,
            },
            other => other,
        };
        Ok(FetchUserResult {
            user_output,
            credentials: new_credentials,
            from_refresh: for_refresh,
        })
    }
}

/// Error type when retrieving a user and validating it against Firebase.
#[derive(Error, Debug)]
pub enum UserAuthenticationError {
    /// The user's refresh token is invalid, which can occur after the user changes
    /// a password for Google or GitHub authentication.
    #[error("Firebase returned a token error when fetching an ID token")]
    DeniedAccessToken(FirebaseError),
    /// The user's account is invalid, which can occur after the user requests
    /// account deletion under GDPR or CCPA.
    #[error("Firebase returned a user error when fetching an ID token")]
    UserAccountDisabled(FirebaseError),
    #[error("Invalid state parameter in auth redirect")]
    InvalidStateParameter,
    #[error("Missing state parameter in auth redirect")]
    MissingStateParameter,
    #[error("unexpected error occurred when fetching an ID token: {0:#}")]
    Unexpected(#[from] anyhow::Error),
}

impl ErrorExt for UserAuthenticationError {
    fn is_actionable(&self) -> bool {
        match self {
            UserAuthenticationError::DeniedAccessToken(error) => {
                // If a request to our server failed because the user's refresh token
                // has expired, they should reauthenticate, but there is no value in
                // reporting this back to us.
                log::info!("ignoring denied access token error: {error:#}");
                false
            }
            UserAuthenticationError::UserAccountDisabled(error) => {
                // If the user's account is disabled, they cannot make requests.
                log::info!("ignoring user account disabled error: {error:#}");
                false
            }
            UserAuthenticationError::Unexpected(error) => error.is_actionable(),
            UserAuthenticationError::InvalidStateParameter
            | UserAuthenticationError::MissingStateParameter => {
                // These errors remain actionable because a surplus could indicate a problem in
                // the login flow, although an attempt to spoof the `state` variable is not actionable.
                true
            }
        }
    }
}
register_error!(UserAuthenticationError);

impl From<FirebaseError> for UserAuthenticationError {
    fn from(error: FirebaseError) -> Self {
        // These Firebase errors indicate that the user's token is in an errored state
        // and that the user likely just needs to log in again.
        const SOFT_ERRORS: &[&str] = &[
            "TOKEN_EXPIRED",
            "INVALID_REFRESH_TOKEN",
            "MISSING_REFRESH_TOKEN",
        ];
        // These Firebase errors indicate that the user's account is in an errored state
        // and that the user likely can no longer sign in with it.
        const HARD_ERRORS: &[&str] = &["USER_DISABLED", "USER_NOT_FOUND"];
        if SOFT_ERRORS.contains(&error.message.as_str()) {
            UserAuthenticationError::DeniedAccessToken(error)
        } else if HARD_ERRORS.contains(&error.message.as_str()) {
            UserAuthenticationError::UserAccountDisabled(error)
        } else {
            UserAuthenticationError::Unexpected(
                anyhow::Error::from(error)
                    .context("Failed to exchange refresh token with access token."),
            )
        }
    }
}
