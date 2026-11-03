use std::time::Duration;

use super::*;

fn status(status: u16, body: &str, retry_after: Option<Duration>) -> Error {
    Error::ProviderStatus {
        status,
        body: body.to_string(),
        retry_after,
    }
}

#[test]
fn provider_statuses_are_classified_by_the_retry_policy() {
    assert!(status(503, "", None).is_retryable());
    assert!(status(429, "", Some(Duration::from_secs(5))).is_retryable());
    assert!(!status(429, r#"{"error":{"code":"insufficient_quota"}}"#, None).is_retryable());
    assert!(!status(429, "", Some(Duration::from_secs(120))).is_retryable());
    assert!(!status(400, "", None).is_retryable());
    assert_eq!(
        status(429, "", Some(Duration::from_secs(5))).retry_after(),
        Some(Duration::from_secs(5))
    );
}

#[test]
fn stream_errors_and_config_errors() {
    assert!(
        Error::ProviderStream {
            code: "overloaded_error".to_string(),
            message: String::new(),
        }
        .is_retryable()
    );
    assert!(
        !Error::NoApiKey {
            model: "m".to_string(),
        }
        .is_retryable()
    );
    let decode = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    assert!(!Error::Decode(decode).is_retryable());
}
