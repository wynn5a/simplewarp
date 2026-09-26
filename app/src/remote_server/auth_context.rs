use std::sync::Arc;

use remote_server::auth::RemoteServerAuthContext;
use warpui::r#async::BoxFuture;

use crate::auth::auth_state::AuthState;

/// Builds the app-wide auth context used by remote-server connections.
///
/// There is no login: connections carry no bearer token and identify with the
/// anonymous id.
pub fn server_api_auth_context(
    auth_state: Arc<AuthState>,
    crash_reporting_enabled: bool,
) -> RemoteServerAuthContext {
    let user_id = auth_state
        .user_id()
        .map(|uid| uid.as_string())
        .unwrap_or_default();
    let user_email = auth_state.user_email().unwrap_or_default();

    RemoteServerAuthContext::new(
        || -> BoxFuture<'static, Option<String>> { Box::pin(async { None }) },
        move || auth_state.anonymous_id(),
        user_id,
        user_email,
        crash_reporting_enabled,
    )
}
