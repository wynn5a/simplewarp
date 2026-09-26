use std::sync::Arc;

use serde::Deserialize;
use warp_core::context_flag::ContextFlag;
use warp_errors::{AnyhowErrorExt, ErrorExt, register_error};
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::server::network_logging::NetworkLogModel;

/// We use a special error code header `X-Warp-Error-Code` to allow the server to send
/// more specific error code information, so that the client can discern between different
/// errors with the same error code.
/// See errors/http_error_codes.go on the server for possible values.
const WARP_ERROR_CODE_HEADER: &str = "X-Warp-Error-Code";

/// An error indicating the user is out of credits. The server sends 429s to communicate this
/// state, but if Cloud Run is overloaded, it can also send 429s that aren't credit-related.
/// So we use this to distinguish between the two cases.
const WARP_ERROR_CODE_OUT_OF_CREDITS: &str = "OUT_OF_CREDITS";

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
            AIApiError::QuotaLimit { .. } | AIApiError::ServerOverloaded => false,
        }
    }
}
register_error!(AIApiError);

/// A singleton entity that provides access to the app's shared HTTP client.
pub struct ServerApiProvider {
    http_client: Arc<http_client::Client>,
}

impl ServerApiProvider {
    /// Constructs a new ServerApiProvider.
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let mut client = http_client::Client::new();
        if ContextFlag::NetworkLogConsole.is_enabled() {
            NetworkLogModel::handle(ctx).update(ctx, |model, model_ctx| {
                model.install_on_clients([&mut client], model_ctx);
            });
        }
        Self {
            http_client: Arc::new(client),
        }
    }

    /// Constructs a new SeverApiProvider for tests.
    #[cfg(test)]
    pub fn new_for_test() -> Self {
        Self {
            http_client: Arc::new(http_client::Client::new_for_test()),
        }
    }

    /// Returns the shared HTTP client. This client is wired into network logging
    /// and includes standard Warp request headers.
    pub fn get_http_client(&self) -> Arc<http_client::Client> {
        self.http_client.clone()
    }
}

impl Entity for ServerApiProvider {
    type Event = ();
}

impl SingletonEntity for ServerApiProvider {}
