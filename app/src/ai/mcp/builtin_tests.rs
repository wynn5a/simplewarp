use super::*;
use crate::ai::mcp::parsing::resolve_json;
use crate::ai::mcp::{MCPServer, MCPServerExt as _, TransportType};

#[test]
fn bearer_token_uses_a_bearer_credential() {
    assert_eq!(
        builtin_bearer_token(&Credentials::Bearer("daemon-token".to_string())),
        Some("daemon-token".to_string())
    );
}

#[test]
fn bearer_token_rejects_session_cookie_auth() {
    assert_eq!(builtin_bearer_token(&Credentials::SessionCookie), None);
}

#[test]
fn factory_mcp_url_joins_server_roots_with_and_without_trailing_slash() {
    assert_eq!(
        factory_mcp_url("https://app.warp.dev"),
        "https://app.warp.dev/api/v1/mcp/factory"
    );
    assert_eq!(
        factory_mcp_url("http://localhost:8080/"),
        "http://localhost:8080/api/v1/mcp/factory"
    );
}

#[test]
fn factory_installation_resolves_to_a_preauthenticated_http_server() {
    let installation =
        factory_mcp_installation_for_server_root("https://staging.warp.dev", "tok-123");
    assert_eq!(installation.uuid(), FACTORY_MCP_INSTALLATION_UUID);
    // Fully resolved: nothing for the variable-prompt UI to ask for, and
    // nothing for handlebars to substitute at spawn time.
    assert!(installation.template_variables().is_empty());

    // The resolved JSON must parse into a single HTTP server with the
    // pre-authenticated header, exactly as `spawn_server_impl` will see it.
    let resolved = resolve_json(&installation);
    let mut servers =
        MCPServer::from_user_json(&resolved).expect("built-in template must parse as MCP config");
    assert_eq!(servers.len(), 1);
    let server = servers.pop().expect("one server");
    assert_eq!(server.name, FACTORY_MCP_SERVER_NAME);
    match server.transport_type {
        TransportType::ServerSentEvents(sse) => {
            assert_eq!(sse.url, "https://staging.warp.dev/api/v1/mcp/factory");
            assert_eq!(sse.headers.len(), 1);
            assert_eq!(sse.headers[0].name, "Authorization");
            assert_eq!(sse.headers[0].value, "Bearer tok-123");
        }
        TransportType::CLIServer(_) => panic!("expected an HTTP transport"),
    }
}
