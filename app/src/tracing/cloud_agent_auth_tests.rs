use chrono::TimeDelta;

use super::*;

fn client_with_expiry(token: &str, expires_at: DateTime<Utc>) -> AuthenticatedHttpClient {
    AuthenticatedHttpClient {
        inner: reqwest::Client::new(),
        token_store: TokenStore::new(token.to_owned(), expires_at).unwrap(),
    }
}

#[test]
fn authorization_overwrites_supplied_header() {
    let client = client_with_expiry(
        "current-test-token",
        Utc::now() + TimeDelta::try_minutes(5).unwrap(),
    );
    let mut request = Request::builder()
        .header(AUTHORIZATION, "Bearer stale-test-token")
        .body(Bytes::new())
        .unwrap();

    client.authorize_request(&mut request).unwrap();

    assert_eq!(
        request.headers().get(AUTHORIZATION).unwrap(),
        "Bearer current-test-token"
    );
}

#[test]
fn expired_token_is_refused_and_supplied_header_is_removed() {
    let client = client_with_expiry(
        "expired-test-token",
        Utc::now() - TimeDelta::try_minutes(5).unwrap(),
    );
    let mut request = Request::builder()
        .header(AUTHORIZATION, "Bearer stale-test-token")
        .body(Bytes::new())
        .unwrap();

    assert!(matches!(
        client.authorize_request(&mut request),
        Err(AuthenticatedHttpError::NoValidToken)
    ));
    assert!(!request.headers().contains_key(AUTHORIZATION));
}

#[test]
fn debug_output_redacts_token() {
    let client = client_with_expiry(
        "secret-test-token",
        Utc::now() + TimeDelta::try_minutes(5).unwrap(),
    );

    let debug_output = format!("{client:?}");

    assert!(!debug_output.contains("secret-test-token"));
    assert!(debug_output.contains("expires_at"));
}

#[test]
fn authorized_request_debug_redacts_token() {
    let client = client_with_expiry(
        "secret-request-test-token",
        Utc::now() + TimeDelta::try_minutes(5).unwrap(),
    );
    let mut request = Request::builder().body(Bytes::new()).unwrap();

    client.authorize_request(&mut request).unwrap();
    let request_debug = format!("{request:?}");
    let headers_debug = format!("{:?}", request.headers());

    assert!(!request_debug.contains("secret-request-test-token"));
    assert!(!headers_debug.contains("secret-request-test-token"));
    assert!(request_debug.contains("Sensitive"));
    assert!(headers_debug.contains("Sensitive"));
}
