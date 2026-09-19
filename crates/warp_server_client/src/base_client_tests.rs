use std::sync::Arc;

use warp_server_auth::auth_state::AuthState;

use super::{BaseClient, GraphqlRoutingConfig};

fn client() -> BaseClient {
    let (event_sender, _) = async_channel::unbounded();
    BaseClient::new(
        Arc::new(http_client::Client::new()),
        Arc::new(AuthState::new_for_test()),
        event_sender,
        GraphqlRoutingConfig {
            path_prefix: Some("/routing-only".to_string()),
        },
    )
}

#[test]
fn explicit_token_graphql_options_route_without_authenticated_headers() {
    let client = client();

    let options = client.graphql_request_options_with_token(Some("token".to_string()));

    assert_eq!(options.path_prefix.as_deref(), Some("/routing-only"));
    assert_eq!(options.auth_token.as_deref(), Some("token"));
    assert!(options.headers.is_empty());
}
