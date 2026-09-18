use std::future::Future;
use std::time::Duration;

use anyhow::{Result, anyhow};
use warpui::r#async::Timer;
use warpui::duration_with_jitter;

use crate::server::graphql::GraphQLError;

/// Typed error for HTTP operations so retry classifiers can inspect status failures.
#[derive(Debug, thiserror::Error)]
#[error("HTTP request failed with status {status}: {body}")]
pub(crate) struct HttpStatusError {
    pub(crate) status: u16,
    pub(crate) body: String,
}

impl warp_errors::ErrorExt for HttpStatusError {
    fn is_actionable(&self) -> bool {
        !matches!(self.status, 408 | 429)
    }
}

warp_errors::register_error!(HttpStatusError);

/// Classify an HTTP-backed error as transient (worth retrying) or permanent (fail fast).
///
/// Transient: 5xx responses, 408, 429, or any error whose chain does not carry an
/// [`HttpStatusError`] (connection reset, timeout, DNS failure, etc.).
/// Permanent: other 4xx responses (bad signature, 404, 403, etc.).
pub(crate) fn is_transient_http_error(e: &anyhow::Error) -> bool {
    // Callers typically wrap an `HttpStatusError` cause with a `.context(...)` message for
    // human-friendly Display, so the typed error sits somewhere in the chain rather than as
    // the top-level error object — walk the chain.
    for cause in e.chain() {
        if let Some(http_err) = cause.downcast_ref::<HttpStatusError>() {
            return is_transient_status(http_err.status);
        }
    }
    true
}

/// Classify GraphQL/public-API status errors as transient or permanent.
///
/// Unlike [`is_transient_http_error`], errors without a typed HTTP/GraphQL transport
/// cause are treated as permanent. This is intended for GraphQL operations where
/// user-facing GraphQL errors are converted into plain `anyhow` errors at the
/// operation layer and should not be retried or placed into transient cooldowns.
pub(crate) fn is_transient_graphql_or_http_error(e: &anyhow::Error) -> bool {
    for cause in e.chain() {
        if let Some(graphql_err) = cause.downcast_ref::<GraphQLError>() {
            return match graphql_err {
                GraphQLError::RequestError(_) => true,
                GraphQLError::HttpError { status, .. } => is_transient_status(status.as_u16()),
                GraphQLError::StagingAccessBlocked | GraphQLError::ResponseError(_) => false,
            };
        }

        if let Some(http_err) = cause.downcast_ref::<HttpStatusError>() {
            return is_transient_status(http_err.status);
        }
    }

    false
}

fn is_transient_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500..=599)
}

/// Maximum total attempts per operation (initial attempt plus retries on transient errors).
pub(crate) const MAX_ATTEMPTS: usize = 3;

/// Base backoff between retry attempts; each subsequent attempt multiplies by [`BACKOFF_FACTOR`].
const INITIAL_BACKOFF: Duration = Duration::from_millis(500);

/// Exponential growth factor for retry backoff.
const BACKOFF_FACTOR: f32 = 2.0;

/// Maximum jitter as a fraction of the backoff interval.
const BACKOFF_JITTER: f32 = 0.3;

/// Run `attempt_fn` with bounded exponential-backoff retries on transient failures.
///
/// `operation` is included in retry logs so concurrent callers can be distinguished.
///
/// `attempt_fn` is called repeatedly with a fresh `Future` per attempt, so callers that need
/// per-attempt state (e.g. cloning a request body) own that inside their closure.
///
/// Transient errors are retried up to [`MAX_ATTEMPTS`] total. Permanent errors return
/// immediately. A warning is logged between attempts so retries are visible in logs.
pub(crate) async fn with_bounded_retry<T, F, Fut>(operation: &str, mut attempt_fn: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut delay = INITIAL_BACKOFF;
    for attempt in 1..=MAX_ATTEMPTS {
        match attempt_fn().await {
            Ok(value) => return Ok(value),
            Err(e) if attempt >= MAX_ATTEMPTS || !is_transient_http_error(&e) => return Err(e),
            Err(e) => {
                log::warn!("{operation}: attempt {attempt}/{MAX_ATTEMPTS} failed, retrying: {e:#}");
                Timer::after(duration_with_jitter(delay, BACKOFF_JITTER)).await;
                delay = delay.mul_f32(BACKOFF_FACTOR);
            }
        }
    }
    // Unreachable when MAX_ATTEMPTS >= 1.
    Err(anyhow!(
        "retry loop exhausted without attempting operation (MAX_ATTEMPTS={MAX_ATTEMPTS})"
    ))
}
