pub mod ai;
pub mod auth;
pub mod harness_support;
pub mod integrations;
pub mod managed_secrets;
pub mod object;
pub(crate) mod presigned_upload;
pub mod team;
pub mod workspace;

use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ai::AIClient;
use anyhow::{Result, anyhow};
use auth::AuthClient;
use channel_versions::ChannelVersions;
use chrono::{DateTime, FixedOffset};
use instant::Instant;
use object::ObjectClient;
use serde::{Deserialize, Serialize};
use team::TeamClient;
use warp_core::context_flag::ContextFlag;
use warp_core::telemetry::TelemetryEvent;
use warp_errors::{AnyhowErrorExt, ErrorExt, register_error};
use warp_managed_secrets::client::ManagedSecretsClient;
use warp_server_client::auth::{AuthClientImpl, AuthEvent};
use warp_server_client::base_client::{
    AuthenticatedGraphqlConfig, BaseClient, GraphqlRoutingConfig,
};
use warp_server_client::network_logging::NetworkLogModel;
use warpui::r#async::BoxFuture;
use warpui::{Entity, ModelContext, SingletonEntity};
use workspace::WorkspaceClient;

use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::get_relevant_files::api::{GetRelevantFiles, GetRelevantFilesResponse};
use crate::ai::predict::generate_ai_input_suggestions::GenerateAIInputSuggestionsRequest;
use crate::ai::predict::generate_am_query_suggestions::GenerateAMQuerySuggestionsRequest;
use crate::ai::predict::predict_am_queries::{PredictAMQueriesRequest, PredictAMQueriesResponse};
use crate::ai::predict::{generate_ai_input_suggestions, generate_am_query_suggestions};
use crate::ai::voice::transcribe::{TranscribeRequest, TranscribeResponse};
use crate::auth::auth_manager::AuthManager;
use crate::auth::auth_state::AuthState;
use crate::server::telemetry::TelemetryApi;
use crate::settings::PrivacySettingsSnapshot;

pub const FETCH_CHANNEL_VERSIONS_TIMEOUT: std::time::Duration = Duration::from_secs(60);
/// We use a special error code header `X-Warp-Error-Code` to allow the server to send
/// more specific error code information, so that the client can discern between different
/// errors with the same error code.
/// See errors/http_error_codes.go on the server for possible values.
const WARP_ERROR_CODE_HEADER: &str = "X-Warp-Error-Code";

/// An error indicating the user is out of credits. The server sends 429s to communicate this
/// state, but if Cloud Run is overloaded, it can also send 429s that aren't credit-related.
/// So we use this to distinguish between the two cases.
const WARP_ERROR_CODE_OUT_OF_CREDITS: &str = "OUT_OF_CREDITS";

/// ResponseType received by Client
#[derive(thiserror::Error, Debug, Serialize, Deserialize)]
#[error("{error}")]
pub struct ClientError {
    pub error: String,
    // We unconditionally check for GitHub auth errors in any public API response. It'd be much better
    // to have the server return error codes that we can parse, but this isn't yet supported.
    // See REMOTE-666
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
}

impl Deref for ServerApi {
    type Target = BaseClient;

    fn deref(&self) -> &Self::Target {
        &self.base_client
    }
}

/// Error when the user is at their cloud agent concurrency limit.
#[derive(thiserror::Error, Debug, Clone, Deserialize)]
#[error("{error} (running agents: {running_agents})")]
pub struct CloudAgentCapacityError {
    pub error: String,
    pub running_agents: i32,
}

#[derive(Debug, Clone)]
pub struct ServerTime {
    time_at_fetch: DateTime<FixedOffset>,
    fetched_at: Instant,
}

impl ServerTime {
    pub fn current_time(&self) -> DateTime<FixedOffset> {
        let elapsed = chrono::Duration::from_std(self.fetched_at.elapsed())
            .expect("duration should not be bigger than limit");
        self.time_at_fetch + elapsed
    }
}

/// Wrapper for deserialization errors. This covers both:
/// * Using `serde` directly
/// * Using `reqwest` decoding utilities
#[derive(thiserror::Error, Debug)]
pub enum DeserializationError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Transport(reqwest::Error),
}

#[derive(Deserialize, Debug)]
struct OutOfCreditsResponse {
    #[serde(default, rename = "userDisplayMessage")]
    user_display_message: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum AIApiError {
    #[error("Request failed due to lack of AI quota.")]
    QuotaLimit {
        user_display_message: Option<String>,
    },

    #[error("Warp is currently overloaded. Please try again later.")]
    ServerOverloaded,

    #[error("Internal error occurred at transport layer.")]
    Transport(#[source] reqwest::Error),

    #[error("Failed to deserialize API response.")]
    Deserialization(#[source] DeserializationError),

    #[error("No context found on context search.")]
    NoContextFound,

    #[error("Failed with status code {0}: {1}")]
    ErrorStatus(http::StatusCode, String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),

    #[error("Got error when streaming {stream_type}: {source:#}")]
    Stream {
        stream_type: &'static str,
        #[source]
        source: anyhow::Error,
    },

    /// Synthesized client-side when a response stream ends without a stream-finished
    /// event: the server always sends one, but the transport can truncate the response
    /// between chunks, surfacing as a clean EOF.
    #[error("Response stream ended unexpectedly before completion.")]
    UnexpectedEof,

    /// Synthesized client-side when a request that uses the connected Grok
    /// subscription can't be sent because its expired OAuth token failed to
    /// refresh. Surfaced as a terminal, user-visible error asking the user to
    /// reconnect, rather than sending a request that would fail authentication.
    #[error(
        "Grok subscription token could not be refreshed. Please try reconnecting your subscription."
    )]
    GrokSubscriptionTokenRefreshFailed,
}

impl From<http_client::ResponseError> for AIApiError {
    fn from(err: http_client::ResponseError) -> Self {
        let http_client::ResponseError {
            source,
            headers,
            body,
        } = err;
        Self::from_response_error(source, &headers, body)
    }
}

impl From<reqwest::Error> for AIApiError {
    fn from(err: reqwest::Error) -> Self {
        Self::from_transport_error(err)
    }
}

impl From<serde_json::Error> for AIApiError {
    fn from(err: serde_json::Error) -> Self {
        AIApiError::Deserialization(err.into())
    }
}

impl AIApiError {
    /// Converts a reqwest error to an AIApiError, using response headers to distinguish
    /// between different types of 429 errors.
    fn from_response_error(
        err: reqwest::Error,
        headers: &::http::HeaderMap,
        body: Option<String>,
    ) -> Self {
        // For HTTP 429 errors, check the X-Warp-Error-Code header to distinguish
        // between out-of-credits and server-overload.
        if err.status() == Some(http::StatusCode::TOO_MANY_REQUESTS) {
            return Self::error_for_429(headers, body);
        }

        Self::from_transport_error(err)
    }

    /// Converts a transport-level reqwest error (no HTTP response) to an AIApiError.
    fn from_transport_error(err: reqwest::Error) -> Self {
        // Unfortunately, `reqwest` reports some non-decoding errors as decoding errors (e.g.
        // unexpected disconnects or timeouts while deserializing a response body). Since we
        // render deserialization and transport errors differently, we try to detect those cases
        // here.
        if err.is_timeout() {
            return AIApiError::Transport(err);
        }
        if err.is_decode() {
            #[cfg(not(target_family = "wasm"))]
            {
                use std::error::Error as _;
                let mut source = err.source();
                while let Some(underlying) = source {
                    if underlying.is::<hyper::Error>() {
                        return AIApiError::Transport(err);
                    }

                    source = underlying.source();
                }
            }

            return AIApiError::Deserialization(DeserializationError::Transport(err));
        }

        AIApiError::Transport(err)
    }

    /// Returns the appropriate error for a 429 response by checking the X-Warp-Error-Code header.
    fn error_for_429(headers: &::http::HeaderMap, body: Option<String>) -> Self {
        if headers
            .get(WARP_ERROR_CODE_HEADER)
            .and_then(|v| v.to_str().ok())
            == Some(WARP_ERROR_CODE_OUT_OF_CREDITS)
        {
            let user_display_message = body
                .and_then(|body| serde_json::from_str::<OutOfCreditsResponse>(&body).ok())
                .and_then(|r| r.user_display_message);
            AIApiError::QuotaLimit {
                user_display_message,
            }
        } else {
            AIApiError::ServerOverloaded
        }
    }

    /// Whether the error is worth an automatic recovery attempt — a fresh request may
    /// succeed. Gates both retry (pre-actions) and resume (post-actions).
    pub fn is_recoverable(&self) -> bool {
        // Don't recover from client errors, except timeouts and rate limits.
        fn is_recoverable_status(status: http::StatusCode) -> bool {
            !status.is_client_error()
                || status == http::StatusCode::REQUEST_TIMEOUT
                || status == http::StatusCode::TOO_MANY_REQUESTS
        }

        match self {
            AIApiError::ErrorStatus(status, _) => is_recoverable_status(*status),
            AIApiError::Transport(e) => {
                if let Some(status) = e.status() {
                    return is_recoverable_status(status);
                }
                true
            }
            // A failed Grok token refresh is a credential problem the user must
            // fix by reconnecting, so retrying or resuming won't help.
            AIApiError::GrokSubscriptionTokenRefreshFailed => false,
            // By default, attempt recovery on error.
            _ => true,
        }
    }
}

impl ErrorExt for AIApiError {
    fn is_actionable(&self) -> bool {
        match self {
            AIApiError::Deserialization(error) => match error {
                DeserializationError::Json(_) => true,
                DeserializationError::Transport(error) => error.is_actionable(),
            },
            AIApiError::Transport(error) => error.is_actionable(),
            AIApiError::Other(error) => error.is_actionable(),
            AIApiError::Stream { source, .. } => source.is_actionable(),
            AIApiError::ErrorStatus(_, _) => self.is_recoverable(),
            AIApiError::UnexpectedEof => true,
            AIApiError::QuotaLimit { .. }
            | AIApiError::ServerOverloaded
            | AIApiError::NoContextFound
            | AIApiError::GrokSubscriptionTokenRefreshFailed => false,
        }
    }
}
register_error!(AIApiError);

#[derive(thiserror::Error, Debug)]
pub enum TranscribeError {
    #[error("Request failed due to lack of Voice quota.")]
    QuotaLimit,

    #[error("Warp is currently overloaded. Please try again later.")]
    ServerOverloaded,

    #[error("Internal error occurred at transport layer.")]
    Transport,

    #[error("Failed to deserialize JSON.")]
    Deserialization,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// SimpleWarp has no Warp server, so every request primitive below fails here
/// rather than opening a connection. The channel config points at hostnames that
/// cannot resolve, which already stops traffic; failing in the client turns a DNS
/// timeout into an immediate, legible error and leaves nothing to time out on.
const LOCAL_ONLY_MESSAGE: &str =
    "SimpleWarp is a local-only build; this operation needs Warp's servers";

pub(crate) fn local_only_error() -> anyhow::Error {
    anyhow!(LOCAL_ONLY_MESSAGE)
}

/// An API wrapper struct with methods to requests to warp-server.
///
/// Prefer NOT adding new methods directly on this struct; instead, add to one of the existing
/// client trait objects, or create your own. This helps keep `ServerApi` from being overloaded
/// with disparate types of calls, and allows you to mock methods in tests.
pub struct ServerApi {
    base_client: Arc<BaseClient>,
    // TODO(jeff): Make `TelemetryApi` another type of client, and move it off `ServerApi`.
    telemetry_api: TelemetryApi,
}

impl ServerApi {
    fn new(
        auth_state: Arc<AuthState>,
        event_sender: async_channel::Sender<AuthEvent>,
        agent_source: Option<ai::AgentSource>,
        ctx: &mut ModelContext<ServerApiProvider>,
    ) -> Self {
        let mut client = http_client::Client::new();
        let mut telemetry_api = TelemetryApi::new();
        if ContextFlag::NetworkLogConsole.is_enabled() {
            NetworkLogModel::handle(ctx).update(ctx, |model, model_ctx| {
                model.install_on_clients([&mut client, &mut telemetry_api.client], model_ctx);
            });
        }
        Self::new_with_parts(
            Arc::new(client),
            auth_state,
            event_sender,
            agent_source,
            telemetry_api,
        )
    }

    fn new_with_parts(
        client: Arc<http_client::Client>,
        auth_state: Arc<AuthState>,
        event_sender: async_channel::Sender<AuthEvent>,
        agent_source: Option<ai::AgentSource>,
        telemetry_api: TelemetryApi,
    ) -> Self {
        let graphql_routing = GraphqlRoutingConfig {
            #[cfg(feature = "agent_mode_evals")]
            path_prefix: Some("/agent-mode-evals".to_string()),
            #[cfg(not(feature = "agent_mode_evals"))]
            path_prefix: None,
        };
        let authenticated_graphql = AuthenticatedGraphqlConfig::default();
        let base_client = Arc::new(BaseClient::new(
            client,
            auth_state,
            event_sender,
            agent_source.map(|source| source.as_str().to_string()),
            graphql_routing,
            authenticated_graphql,
        ));

        Self {
            base_client,
            telemetry_api,
        }
    }

    #[cfg(test)]
    fn new_for_test() -> Self {
        let (tx, _) = async_channel::unbounded();
        let auth_state = Arc::new(AuthState::new_for_test());
        let client = Arc::new(http_client::Client::new_for_test());

        Self::new_with_parts(client, auth_state, tx, None, TelemetryApi::new())
    }

    #[cfg(all(test, feature = "skip_login"))]
    fn new_for_test_with_bearer_token(
        bearer_token: Option<String>,
        event_sender: async_channel::Sender<AuthEvent>,
    ) -> Self {
        let auth_state = Arc::new(AuthState::new_logged_out_for_test());
        if let Some(bearer_token) = bearer_token {
            auth_state.set_remote_server_bearer_token(bearer_token);
        }
        Self::new_with_parts(
            Arc::new(http_client::Client::new_for_test()),
            auth_state,
            event_sender,
            None,
            None,
            TelemetryApi::new(),
        )
    }

    /// Sets the ambient agent task ID to be sent with all subsequent requests.
    pub fn set_ambient_agent_task_id(&self, task_id: Option<AmbientAgentTaskId>) {
        self.base_client
            .set_ambient_agent_task_id(task_id.map(|task_id| task_id.to_string()));
    }

    pub fn send_graphql_request<'a, QF, O: warp_graphql::client::Operation<QF> + Send + 'a>(
        &'a self,
        _operation: O,
        _timeout: Option<Duration>,
    ) -> BoxFuture<'a, Result<QF>>
    where
        QF: 'a,
    {
        Box::pin(async { Err(local_only_error()) })
    }

    /// Opens an SSE stream to the agent event-push endpoint.
    ///
    /// The returned `EventSourceStream` yields `reqwest_eventsource::Event`
    /// items until the connection closes or an error occurs. The caller is
    /// responsible for reading the stream and handling reconnection.
    ///
    /// The stream is served by warp-server-rtc (not the main warp-server pool),
    /// so the URL is built from `ChannelState::rtc_http_url()` rather than
    /// `server_root_url()`.
    pub async fn stream_agent_events(
        &self,
        _run_ids: &[String],
        _since_sequence: i64,
    ) -> Result<http_client::EventSourceStream> {
        Err(local_only_error())
    }

    /// Opens an SSE stream against the ancestor-scoped agent event endpoint.
    pub async fn stream_agent_events_for_ancestor(
        &self,
        _ancestor_run_id: &str,
        _include_self: bool,
        _since_sequence: i64,
    ) -> Result<http_client::EventSourceStream> {
        Err(local_only_error())
    }

    pub async fn stream_agent_events_for_task(
        &self,
        _task_id: &AmbientAgentTaskId,
        _run_ids: &[String],
        _since_sequence: i64,
    ) -> Result<http_client::EventSourceStream> {
        Err(local_only_error())
    }

    /// Sends an authenticated empty POST request to /client/login, which signals to the server
    /// that the user is logged in.
    pub async fn notify_login(&self) {
        log::debug!("Skipping login notification: {}", LOCAL_ONLY_MESSAGE);
    }

    /// Synchronously sends a [`TelemetryEvent`] to the Rudderstack API. Prefer not to call this
    /// directly, use the macros defined in crate::server::telemetry::macros. If telemetry is
    /// disabled, this is a no-op.
    pub async fn send_telemetry_event(
        &self,
        event: impl TelemetryEvent,
        settings_snapshot: PrivacySettingsSnapshot,
    ) -> Result<()> {
        let user_id = self.user_id();
        let anonymous_id = self.anonymous_id();
        self.telemetry_api
            .send_telemetry_event(user_id, anonymous_id, event, settings_snapshot)
            .await
    }

    pub async fn send_agent_tip_shown_analytics_event(&self, _tip: String) -> Result<()> {
        Err(local_only_error())
    }

    /// Drains all queued [`TelemetryEvent`]s into Rudderstack requests containing the corresponding
    /// batch of events. Events are queued using the [`send_telemetry_from_ctx`] or
    /// [`send_telemetry_from_app_ctx`] macros. If telemetry is disabled for the user, this flushes
    /// the UI framework event queue and does nothing with them (no request is made).
    ///
    /// Returns the number of events that were flushed.
    pub async fn flush_telemetry_events(
        &self,
        settings_snapshot: PrivacySettingsSnapshot,
    ) -> Result<usize> {
        self.telemetry_api.flush_events(settings_snapshot).await
    }

    /// Sends a batched Rudder request containing events written to the file at `path`. This is a
    /// no-op if telemetry is disabled.
    pub async fn flush_persisted_events_to_rudder(
        &self,
        path: &Path,
        settings_snapshot: PrivacySettingsSnapshot,
    ) -> Result<()> {
        self.telemetry_api
            .flush_persisted_events_to_rudder(path, settings_snapshot)
            .await
    }

    /// Writes all queued [`TelemetryEvent`]s to a file, limiting the number of written
    /// events to `max_events`. Events are queued using the [`send_telemetry_from_ctx`] or
    /// [`send_telemetry_from_app_ctx`] macros. If telemetry is disabled, no events are written to
    /// disk.
    pub fn persist_telemetry_events(
        &self,
        max_event_count: usize,
        settings_snapshot: PrivacySettingsSnapshot,
    ) -> Result<()> {
        self.telemetry_api
            .flush_and_persist_events(max_event_count, settings_snapshot)
    }

    /// Hits the /ai/generate_input_suggestions endpoint to get the predicted next action, based on past context.
    pub async fn generate_ai_input_suggestions(
        &self,
        _request: &GenerateAIInputSuggestionsRequest,
    ) -> Result<generate_ai_input_suggestions::GenerateAIInputSuggestionsResponseV2, AIApiError>
    {
        Err(AIApiError::Other(local_only_error()))
    }

    pub async fn get_relevant_files(
        &self,
        _request: &GetRelevantFiles,
    ) -> Result<GetRelevantFilesResponse, AIApiError> {
        Err(AIApiError::Other(local_only_error()))
    }

    /// Hits the /ai/generate_am_query_suggestions endpoint to get the predicted next query.
    pub async fn generate_am_query_suggestions(
        &self,
        _request: &GenerateAMQuerySuggestionsRequest,
    ) -> Result<generate_am_query_suggestions::GenerateAMQuerySuggestionsResponse, AIApiError> {
        Err(AIApiError::Other(local_only_error()))
    }

    pub async fn predict_am_queries(
        &self,
        _request: &PredictAMQueriesRequest,
    ) -> Result<PredictAMQueriesResponse, AIApiError> {
        Err(AIApiError::Other(local_only_error()))
    }

    /// Hits the /ai/transcribe endpoint to get the transcription for the given audio.
    pub async fn transcribe(
        &self,
        _request: &TranscribeRequest,
    ) -> Result<TranscribeResponse, TranscribeError> {
        Err(TranscribeError::Other(local_only_error()))
    }

    pub async fn server_time(&self) -> Result<ServerTime> {
        Err(local_only_error())
    }

    /// Fetches updated Warp Channel Versions from Warp Server. If it is the first such request of
    /// the current calendar day, first attempts to call the '/client_version/daily'. If that call
    /// fails or if it not the first request of the calendar day, returns the result of a call to
    /// `/client_version'. The caller can specify whether or not changelog information should be
    /// included in the response based on whether or not it will be used.
    pub async fn fetch_channel_versions(
        &self,
        _include_changelogs: bool,
        _is_daily: bool,
    ) -> Result<ChannelVersions> {
        Err(local_only_error())
    }
}

/// A singleton entity that provides access to the global [`ServerApi`] instance,
/// or any of its implemented trait objects.
pub struct ServerApiProvider {
    server_api: Arc<ServerApi>,
    auth_client: Arc<dyn AuthClient>,
}

impl ServerApiProvider {
    /// Constructs a new ServerApiProvider.
    #[cfg_attr(target_family = "wasm", allow(unused_variables))]
    pub fn new(
        auth_state: Arc<AuthState>,
        agent_source: Option<ai::AgentSource>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let (event_sender, event_receiver) = async_channel::bounded(10);

        let server_api = ServerApi::new(auth_state.clone(), event_sender, agent_source, ctx);

        ctx.spawn_stream_local(
            event_receiver,
            move |_, event, ctx| {
                match event {
                    AuthEvent::UserAccountDisabled => {
                        // We dispatch a global action here because the log out code requires
                        // `server_api`, causing a circular model reference panic when it calls
                        // `ServerApiProvider` to get access.
                        // TODO: We should remove this pattern where `ServerApiProvider` responds
                        // to events; it's prone to these sorts of circular reference issues.
                        ctx.dispatch_global_action("app:log_out", ());
                    }
                    AuthEvent::NeedsReauth => {
                        // AuthManager depends on a reference to ServerApi, so ServerApi can't easily
                        // hold a ref to AuthManager. To get around this, we emit an event on ServerApi
                        // and handle calling the AuthManager here instead.
                        AuthManager::handle(ctx).update(ctx, |auth_manager, ctx| {
                            auth_manager.set_needs_reauth(true, ctx);
                        });
                    }
                    // Re-emit the event for subscribers.
                    // TODO: we probably want a different type for the event emitted to subscribers
                    // from the one that's used for the async channel.
                    _ => ctx.emit(event),
                }
            },
            |_, _| {},
        );
        let server_api = Arc::new(server_api);
        let auth_client = Arc::new(AuthClientImpl::new(server_api.base_client.clone()));
        Self {
            server_api,
            auth_client,
        }
    }

    /// Constructs a new SeverApiProvider for tests.
    #[cfg(test)]
    pub fn new_for_test() -> Self {
        let server_api = Arc::new(ServerApi::new_for_test());
        let auth_client = Arc::new(AuthClientImpl::new(server_api.base_client.clone()));
        Self {
            server_api,
            auth_client,
        }
    }

    /// Returns a handle to the underlying [`ServerApi`] object.
    /// Prefer retrieving a specific trait object related to the methods you're calling.
    pub fn get(&self) -> Arc<ServerApi> {
        self.server_api.clone()
    }

    pub fn get_auth_client(&self) -> Arc<dyn AuthClient> {
        self.auth_client.clone()
    }

    pub fn get_workspace_client(&self) -> Arc<dyn WorkspaceClient> {
        self.server_api.clone()
    }

    pub fn get_team_client(&self) -> Arc<dyn TeamClient> {
        self.server_api.clone()
    }
    pub fn get_ai_client(&self) -> Arc<dyn AIClient> {
        self.server_api.clone()
    }

    pub fn get_cloud_objects_client(&self) -> Arc<dyn ObjectClient> {
        self.server_api.clone()
    }

    pub fn get_integrations_client(&self) -> Arc<dyn integrations::IntegrationsClient> {
        self.server_api.clone()
    }

    pub fn get_managed_secrets_client(&self) -> Arc<dyn ManagedSecretsClient> {
        self.server_api.clone()
    }

    /// Returns the shared HTTP client. This client is wired into network logging
    /// and includes standard Warp request headers.
    pub fn get_http_client(&self) -> Arc<http_client::Client> {
        self.server_api.owned_http_client()
    }

    #[cfg_attr(target_family = "wasm", expect(dead_code))]
    pub fn get_harness_support_client(&self) -> Arc<dyn harness_support::HarnessSupportClient> {
        self.server_api.clone()
    }
}

impl Entity for ServerApiProvider {
    type Event = AuthEvent;
}

impl SingletonEntity for ServerApiProvider {}
