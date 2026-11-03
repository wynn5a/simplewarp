//! Automatic retries: which provider failures are worth sending the same request again for, and
//! how long to wait first.
//!
//! Only a request that failed before its reply started is ever sent again, so a retry never
//! duplicates part of a reply; whoever runs the retry enforces that.

use std::time::Duration;

/// Retries after the first attempt, before the failure is shown.
pub const MAX_RETRIES: u32 = 3;

/// The wait before the first retry; each later retry waits twice as long.
pub const BASE_DELAY: Duration = Duration::from_secs(2);

/// The longest `retry-after` a retry waits for. A provider that asks for longer is out of quota
/// for the hour or the day rather than briefly rate-limited, and waiting would not help.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// Substrings of a 429 body or a stream error code that mean the account is out of quota or
/// credit, rather than sending too fast.
const EXHAUSTED_QUOTA_MARKERS: [&str; 4] = [
    "insufficient_quota",
    "usage_limit_reached",
    "subscription_sharing_usage_limit_exceeded",
    "billing",
];

/// Stream error codes for an overloaded, rate-limited or failing provider.
const TRANSIENT_STREAM_CODES: [&str; 8] = [
    "overloaded_error",
    "api_error",
    "rate_limit_error",
    "rate_limit_exceeded",
    "server_error",
    "internal_server_error",
    "service_unavailable",
    "timeout",
];

/// How a request to a provider failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure<'a> {
    /// The provider answered with an error status. `status` is 0 when the request was never sent.
    Status { status: u16, body: &'a str },
    /// The provider reported an error inside a stream that had opened.
    Stream { code: &'a str, message: &'a str },
    /// The connection failed or dropped.
    Connection,
}

/// Whether sending the same request again later can succeed.
pub fn is_transient(failure: &Failure) -> bool {
    match failure {
        Failure::Status { status, body } => is_transient_status(*status, body),
        Failure::Stream { code, .. } => match code.parse::<u16>() {
            // OpenRouter reports the upstream status as a numeric code.
            Ok(status) => is_transient_status(status, ""),
            Err(_) => !names_exhausted_quota(code) && TRANSIENT_STREAM_CODES.contains(code),
        },
        Failure::Connection => true,
    }
}

fn is_transient_status(status: u16, body: &str) -> bool {
    match status {
        408 => true,
        429 => !names_exhausted_quota(body),
        501 | 505 => false,
        500..=599 => true,
        _ => false,
    }
}

fn names_exhausted_quota(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    EXHAUSTED_QUOTA_MARKERS
        .iter()
        .any(|marker| text.contains(marker))
}

/// The wait a provider asked for, from its `retry-after-ms` header (OpenAI and Anthropic send it)
/// or else its `retry-after` header in seconds. An HTTP date, a negative or an unreadable value
/// counts as no request.
pub fn parse_retry_after(
    retry_after: Option<&str>,
    retry_after_ms: Option<&str>,
) -> Option<Duration> {
    let parse = |value: Option<&str>, scale: f64| {
        let value = value?.trim().parse::<f64>().ok()?;
        Duration::try_from_secs_f64(value / scale).ok()
    };
    parse(retry_after_ms, 1000.0).or_else(|| parse(retry_after, 1.0))
}

/// Whether a failure is retried: it is transient, and the provider did not ask for a wait longer
/// than [`MAX_RETRY_AFTER`].
pub fn is_retryable(failure: &Failure, retry_after: Option<Duration>) -> bool {
    is_transient(failure) && retry_after.is_none_or(|wait| wait <= MAX_RETRY_AFTER)
}

/// The wait before retry number `retry` (from 1): what the provider asked for, otherwise
/// [`BASE_DELAY`] doubled for each earlier retry.
pub fn delay(retry: u32, retry_after: Option<Duration>) -> Duration {
    retry_after.unwrap_or_else(|| BASE_DELAY * 2u32.saturating_pow(retry.saturating_sub(1)))
}

/// Whether a request about to be sent will be sent again if it fails retryably before its reply.
pub fn will_retry(enabled: bool, retries_done: u32) -> bool {
    enabled && retries_done < MAX_RETRIES
}

/// What to do about a request that failed before its reply started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Send it again after `delay`; this is retry number `retry` of [`MAX_RETRIES`].
    Retry { retry: u32, delay: Duration },
    /// Show the failure.
    GiveUp,
}

/// Decides on a request that failed before its reply. `may_retry` is [`will_retry`] as it was
/// when the request was sent.
pub fn decide(
    may_retry: bool,
    retries_done: u32,
    retryable: bool,
    retry_after: Option<Duration>,
) -> Decision {
    if may_retry && retryable && retries_done < MAX_RETRIES {
        let retry = retries_done + 1;
        Decision::Retry {
            retry,
            delay: delay(retry, retry_after),
        }
    } else {
        Decision::GiveUp
    }
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod tests;
