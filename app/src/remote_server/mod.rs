#[cfg(not(target_family = "wasm"))]
use remote_server::manager::RemoteServerManager;
// Re-export everything from the `remote_server` crate so existing
// `crate::remote_server::*` imports in `app` continue to work.
pub use remote_server::*;
#[cfg(not(target_family = "wasm"))]
use warpui::SingletonEntity as _;

#[cfg(not(target_family = "wasm"))]
pub mod auth_context;
#[cfg(not(target_family = "wasm"))]
pub mod diff_state_proto;
#[cfg(not(target_family = "wasm"))]
pub mod diff_state_tracker;
pub mod git_status_proto;
#[cfg(not(target_family = "wasm"))]
mod ripgrep_search;
#[cfg(not(target_family = "wasm"))]
pub mod server_buffer_tracker;
#[cfg(not(target_family = "wasm"))]
pub mod server_model;
#[cfg(not(target_family = "wasm"))]
pub mod ssh_transport;
#[cfg(unix)]
pub mod unix;

/// Run the `remote-server-proxy` subcommand.
#[cfg(unix)]
pub fn run_proxy(identity_key: String) -> anyhow::Result<()> {
    unix::proxy::run(&identity_key)
}

#[cfg(not(unix))]
pub fn run_proxy(_identity_key: String) -> anyhow::Result<()> {
    anyhow::bail!("remote-server-proxy is not supported on this platform")
}

/// Run the `remote-server-daemon` subcommand.
#[cfg(unix)]
pub fn run_daemon(identity_key: String) -> anyhow::Result<()> {
    unix::run_daemon(identity_key)
}

#[cfg(not(unix))]
pub fn run_daemon(_identity_key: String) -> anyhow::Result<()> {
    anyhow::bail!("remote-server-daemon is not supported on this platform")
}

/// Forwards privacy preference change events to the remote-server manager.
#[cfg(not(target_family = "wasm"))]
pub fn wire_privacy_preference_forwarding(ctx: &mut warpui::AppContext) {
    // Forward crash reporting preference changes to all connected daemons.
    use crate::settings::{PrivacySettings, PrivacySettingsChangedEvent};
    let manager = RemoteServerManager::handle(ctx);
    let privacy_settings = PrivacySettings::handle(ctx);
    ctx.subscribe_to_model(&privacy_settings, move |_, event, ctx| {
        if let &PrivacySettingsChangedEvent::UpdateIsCrashReportingEnabled { new_value, .. } = event
        {
            for client in manager.as_ref(ctx).all_connected_clients() {
                client.update_preferences(new_value);
            }
        }
    });
}
