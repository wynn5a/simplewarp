use anyhow::Result;

use crate::auth::credentials::{FirebaseToken, RefreshToken};

#[test]
fn test_firebase_token_urls() -> Result<()> {
    let custom_token = FirebaseToken::Custom("ct".to_string());
    let refresh_token = FirebaseToken::Refresh(RefreshToken::new("rt".to_string()));

    assert_eq!(
        custom_token.access_token_url("api_key"),
        "https://identitytoolkit.googleapis.com/v1/accounts:signInWithCustomToken?key=api_key"
    );
    assert_eq!(
        refresh_token.access_token_url("api_key"),
        "https://securetoken.googleapis.com/v1/token?key=api_key"
    );

    assert_eq!(
        custom_token.access_token_request_body(),
        vec![("returnSecureToken", "true"), ("token", "ct")]
    );
    assert_eq!(
        refresh_token.access_token_request_body(),
        vec![("grant_type", "refresh_token"), ("refresh_token", "rt")],
    );

    assert_eq!(
        custom_token.proxy_url("https://staging.warp.dev", "api_key"),
        "https://staging.warp.dev/proxy/customToken?key=api_key"
    );
    assert_eq!(
        refresh_token.proxy_url("https://staging.warp.dev", "api_key"),
        "https://staging.warp.dev/proxy/token?key=api_key"
    );
    Ok(())
}

#[cfg(feature = "skip_login")]
#[test]
fn access_token_skip_login_rejects_bearer_token() {
    use std::sync::Arc;

    use warp_server_auth::auth_state::AuthState;
    use warp_server_client::auth::{AuthClient, AuthClientImpl, GraphqlRoutingConfig};

    let (event_sender, _) = async_channel::unbounded();
    let auth_state = Arc::new(AuthState::new_logged_out_for_test());
    auth_state.set_remote_server_bearer_token("daemon-token".to_string());
    let auth_client = AuthClientImpl::new(
        Arc::new(http_client::Client::new()),
        auth_state,
        event_sender,
        GraphqlRoutingConfig::default(),
    );

    let error = futures::executor::block_on(auth_client.get_or_refresh_access_token()).unwrap_err();

    assert_eq!(
        error.to_string(),
        "skip_login enabled; failing all authenticated requests"
    );
}
