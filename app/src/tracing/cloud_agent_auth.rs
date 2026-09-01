//! Provides authenticated OTLP trace transport for opted-in cloud agents.
//!
//! Dispatch bootstraps tracing with a bearer token and expiry in the process environment. The
//! exporter is built once around [`AuthenticatedHttpClient`], which reads a shared token snapshot
//! immediately before every request. Processes without the endpoint switch or a currently valid
//! dispatch credential never initialize this module.
//!
//! Tokens must never appear in diagnostics or formatted values. Cached authorization headers are
//! marked sensitive, manual `Debug` implementations omit secrets, and token-store locks are always
//! released before network I/O.
use std::fmt;
use std::sync::{Arc, RwLock};

use anyhow::{Context as _, anyhow};
use async_compat::Compat;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use http::header::{AUTHORIZATION, HeaderValue};
use opentelemetry_http::{Bytes, HttpClient, HttpError, Request, Response};

/// The environment variables form the immutable dispatch-time authentication bootstrap.
const CLOUD_AGENT_OTLP_TOKEN: &str = "WARP_CLOUD_AGENT_OTLP_TOKEN";
const CLOUD_AGENT_OTLP_TOKEN_EXPIRES_AT: &str = "WARP_CLOUD_AGENT_OTLP_TOKEN_EXPIRES_AT";

/// Shared dispatch authentication state for the exporter.
#[derive(Clone)]
pub(super) struct AuthContext {
    token_store: TokenStore,
}

impl AuthContext {
    /// Seeds authentication from a currently valid dispatch credential in the environment.
    ///
    /// The caller treats failure as an opt-out so normal processes and partially rolled-out cloud
    /// agents retain no-op tracing behavior.
    pub(super) fn from_environment() -> anyhow::Result<Self> {
        let token =
            std::env::var(CLOUD_AGENT_OTLP_TOKEN).context("Cloud-agent OTLP token is missing")?;
        // Remove the bootstrap secret as soon as it is owned so child processes cannot inherit it.
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var(CLOUD_AGENT_OTLP_TOKEN) };
        let token = token.trim().to_owned();
        anyhow::ensure!(!token.is_empty(), "Cloud-agent OTLP token is empty");

        let expires_at = std::env::var(CLOUD_AGENT_OTLP_TOKEN_EXPIRES_AT)
            .context("Cloud-agent OTLP token expiry is missing")?;
        let expires_at = DateTime::parse_from_rfc3339(expires_at.trim())
            .context("Cloud-agent OTLP token expiry is not valid RFC3339")?;
        anyhow::ensure!(
            expires_at.offset().local_minus_utc() == 0,
            "Cloud-agent OTLP token expiry is not UTC"
        );
        let expires_at = expires_at.with_timezone(&Utc);
        anyhow::ensure!(
            expires_at > Utc::now(),
            "Cloud-agent OTLP token is already expired"
        );
        let token_store = TokenStore::new(token, expires_at)?;
        Ok(Self { token_store })
    }

    /// Creates a transport sharing the latest credential while leaving the exporter itself stable.
    pub(super) fn http_client(&self) -> AuthenticatedHttpClient {
        AuthenticatedHttpClient {
            inner: reqwest::Client::new(),
            token_store: self.token_store.clone(),
        }
    }
}

impl fmt::Debug for AuthContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthContext")
            .field("token_store", &self.token_store)
            .finish_non_exhaustive()
    }
}

/// A snapshot of the latest credential, stored behind a short-lived reader/writer lock.
///
/// Readers clone only the sensitive authorization header, and no caller holds this lock during
/// network I/O. Replacement constructs and validates a complete snapshot before taking the write
/// lock so failures preserve the last valid credential.
#[derive(Clone)]
struct TokenStore {
    inner: Arc<RwLock<TokenSnapshot>>,
}

impl TokenStore {
    /// Creates the initial store from the validated dispatch credential.
    fn new(token: String, expires_at: DateTime<Utc>) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(RwLock::new(TokenSnapshot::new(token, expires_at)?)),
        })
    }

    /// Returns a cloned sensitive header only while the current credential remains unexpired.
    fn valid_authorization_header(&self) -> Option<HeaderValue> {
        let snapshot = self.inner.read().unwrap_or_else(|err| err.into_inner());
        (snapshot.expires_at > Utc::now()).then(|| snapshot.authorization_header.clone())
    }

}

impl fmt::Debug for TokenStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let snapshot = self.inner.read().unwrap_or_else(|err| err.into_inner());
        formatter
            .debug_struct("TokenStore")
            .field("expires_at", &snapshot.expires_at)
            .finish_non_exhaustive()
    }
}

/// An already-parsed sensitive authorization header and its trusted server expiry.
struct TokenSnapshot {
    authorization_header: HeaderValue,
    expires_at: DateTime<Utc>,
}

impl TokenSnapshot {
    /// Constructs a snapshot whose header redacts its value from standard debug formatting.
    fn new(token: String, expires_at: DateTime<Utc>) -> anyhow::Result<Self> {
        let mut authorization_header = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| anyhow!("Cloud-agent OTLP token cannot be used as an HTTP header"))?;
        authorization_header.set_sensitive(true);
        Ok(Self {
            authorization_header,
            expires_at,
        })
    }
}

impl fmt::Debug for TokenSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenSnapshot")
            .field("authorization_header", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// The set of errors that can occur when making an HTTP request using [`AuthenticatedHttpClient`].
#[derive(thiserror::Error, Debug)]
enum AuthenticatedHttpError {
    #[error("No unexpired cloud-agent OTLP token is available")]
    NoValidToken,
    #[error("Cloud-agent OTLP request failed with HTTP status {0}")]
    HttpStatus(u16),
}

/// An HTTP client that injects the latest valid token immediately before each request.
///
/// The token-store lock is released before network I/O begins. A manual `Debug` implementation
/// prevents the client from formatting cached state, while sensitive [`HeaderValue`] instances
/// redact request headers. Expired credentials are removed and refused rather than sent.
pub(super) struct AuthenticatedHttpClient {
    inner: reqwest::Client,
    token_store: TokenStore,
}

impl fmt::Debug for AuthenticatedHttpClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedHttpClient")
            .field("token_store", &self.token_store)
            .finish_non_exhaustive()
    }
}

impl AuthenticatedHttpClient {
    /// Overwrites any supplied authorization header with the latest unexpired credential.
    ///
    /// Removing the supplied header first ensures an expired store fails closed rather than
    /// accidentally sending a stale or caller-provided credential.
    fn authorize_request(
        &self,
        request: &mut Request<Bytes>,
    ) -> Result<(), AuthenticatedHttpError> {
        request.headers_mut().remove(AUTHORIZATION);
        let authorization = self
            .token_store
            .valid_authorization_header()
            .ok_or(AuthenticatedHttpError::NoValidToken)?;
        request.headers_mut().insert(AUTHORIZATION, authorization);
        Ok(())
    }
}

#[async_trait]
impl HttpClient for AuthenticatedHttpClient {
    async fn send_bytes(&self, mut request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        self.authorize_request(&mut request)?;

        let request: reqwest::Request = request.try_into()?;
        // Reqwest requires a Tokio-compatible context, while the exporter may use another executor.
        let (status, response) = Compat::new(async {
            let mut response = self.inner.execute(request).await?;
            let status = response.status();
            let response = if status.is_success() {
                let headers = std::mem::take(response.headers_mut());
                Some((headers, response.bytes().await?))
            } else {
                None
            };
            Ok::<_, reqwest::Error>((status, response))
        })
        .await?;
        let Some((headers, body)) = response else {
            return Err(AuthenticatedHttpError::HttpStatus(status.as_u16()).into());
        };

        let mut response = Response::builder().status(status).body(body)?;
        *response.headers_mut() = headers;
        Ok(response)
    }
}

#[cfg(test)]
#[path = "cloud_agent_auth_tests.rs"]
mod tests;
