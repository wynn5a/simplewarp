use super::*;

fn status(status: u16) -> bool {
    is_transient(&Failure::Status { status, body: "" })
}

fn stream(code: &str) -> bool {
    is_transient(&Failure::Stream { code, message: "" })
}

#[test]
fn rate_limits_and_server_errors_are_transient() {
    for code in [408, 429, 500, 502, 503, 504, 529, 599] {
        assert!(status(code), "{code}");
    }
    for code in [501, 505] {
        assert!(!status(code), "{code}");
    }
}

#[test]
fn exhausted_quota_is_not_transient() {
    for body in [
        r#"{"error":{"code":"insufficient_quota"}}"#,
        r#"{"error":{"type":"usage_limit_reached"}}"#,
        "Check your plan and Billing details",
    ] {
        assert!(
            !is_transient(&Failure::Status { status: 429, body }),
            "{body}"
        );
    }
    assert!(is_transient(&Failure::Status {
        status: 429,
        body: r#"{"error":{"type":"rate_limit_error"}}"#,
    }));
}

#[test]
fn client_errors_are_not_transient() {
    for code in [0, 400, 401, 403, 404] {
        assert!(!status(code), "{code}");
    }
}

#[test]
fn overloaded_and_rate_limited_streams_are_transient() {
    for code in [
        "overloaded_error",
        "rate_limit_exceeded",
        "server_error",
        "503",
    ] {
        assert!(stream(code), "{code}");
    }
    for code in ["insufficient_quota", "invalid_request_error", "400"] {
        assert!(!stream(code), "{code}");
    }
}

#[test]
fn a_dropped_connection_is_transient() {
    assert!(is_transient(&Failure::Connection));
}

#[test]
fn retry_after_milliseconds_win_over_seconds() {
    assert_eq!(
        parse_retry_after(Some("5"), Some("1500")),
        Some(Duration::from_millis(1500))
    );
    assert_eq!(
        parse_retry_after(Some("5"), None),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        parse_retry_after(Some("2.5"), None),
        Some(Duration::from_millis(2500))
    );
}

#[test]
fn unreadable_retry_after_is_ignored() {
    for value in [
        "soon",
        "-1",
        "Wed, 21 Oct 2026 07:28:00 GMT",
        "",
        "1e20",
        "inf",
        "NaN",
    ] {
        assert_eq!(parse_retry_after(Some(value), None), None, "{value}");
    }
    assert_eq!(parse_retry_after(None, None), None);
}

#[test]
fn the_backoff_doubles_from_two_seconds() {
    assert_eq!(delay(1, None), Duration::from_secs(2));
    assert_eq!(delay(2, None), Duration::from_secs(4));
    assert_eq!(delay(3, None), Duration::from_secs(8));
    assert_eq!(
        delay(2, Some(Duration::from_secs(10))),
        Duration::from_secs(10)
    );
}

#[test]
fn a_wait_longer_than_a_minute_is_not_retried() {
    let rate_limit = Failure::Status {
        status: 429,
        body: "",
    };
    assert!(!is_retryable(&rate_limit, Some(Duration::from_secs(61))));
    assert!(is_retryable(&rate_limit, Some(Duration::from_secs(60))));
    assert!(is_retryable(&rate_limit, None));
}

#[test]
fn three_retries_then_give_up() {
    for (retries_done, expected) in [(0, 2), (1, 4), (2, 8)] {
        assert_eq!(
            decide(true, retries_done, true, None),
            Decision::Retry {
                retry: retries_done + 1,
                delay: Duration::from_secs(expected),
            }
        );
    }
    assert_eq!(decide(true, 3, true, None), Decision::GiveUp);
    assert!(will_retry(true, 2));
    assert!(!will_retry(true, 3));
}

#[test]
fn nothing_is_retried_when_disabled_or_not_retryable() {
    assert_eq!(decide(false, 0, true, None), Decision::GiveUp);
    assert_eq!(decide(true, 0, false, None), Decision::GiveUp);
    assert!(!will_retry(false, 0));
}
