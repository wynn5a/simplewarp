use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::AppId;

#[derive(Debug)]
pub struct ChannelConfig {
    /// The application ID for this channel.
    pub app_id: AppId,

    /// The custom URL scheme the app registers with the OS (e.g. in its Info.plist); deep links
    /// and OAuth redirects use it, so it must match what the bundle registers.
    pub url_scheme: &'static str,

    /// The name the app's CLI is invoked as, e.g. the `/usr/local/bin` symlink the app installs.
    pub cli_command_name: &'static str,

    /// The name of the file to which logs should be written.
    pub logfile_name: Cow<'static, str>,

    /// Configuration for statically-bundled MCP OAuth credentials.
    pub mcp_static_config: Option<McpStaticConfig>,
}

/// Configuration for statically-bundled MCP OAuth credentials.
///
/// These are credentials for OAuth providers where dynamic client registration
/// is not supported and we instead ship pre-registered client IDs and secrets.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct McpStaticConfig {
    /// Per-provider OAuth credentials.
    pub providers: Vec<McpOAuthProviderConfig>,
}

/// A single OAuth provider's credentials for MCP authentication.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct McpOAuthProviderConfig {
    /// The issuer URL of the OAuth provider (e.g. `https://github.com/login/oauth`).
    pub issuer: Cow<'static, str>,
    /// The OAuth client ID registered for this channel.
    pub client_id: Cow<'static, str>,
    /// The OAuth client secret registered for this channel.
    pub client_secret: Cow<'static, str>,
    /// A separately registered native client that permits loopback redirects.
    /// This must never be inferred from the custom-scheme client.
    #[serde(default)]
    pub loopback_client: Option<McpOAuthLoopbackClientConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct McpOAuthLoopbackClientConfig {
    pub client_id: Cow<'static, str>,
    /// Native OAuth clients should normally be public, but retain optional
    /// secret support for providers that require one.
    #[serde(default)]
    pub client_secret: Option<Cow<'static, str>>,
}
