use std::borrow::Cow;
use std::collections::HashSet;

use lazy_static::lazy_static;
use parking_lot::Mutex;

use super::Channel;
use crate::AppId;
use crate::channel::config::{ChannelConfig, McpOAuthProviderConfig};
use crate::features::FeatureFlag;

lazy_static! {
    static ref CHANNEL_STATE: Mutex<ChannelState> = Mutex::new(ChannelState::init());
}

#[cfg(feature = "test-util")]
lazy_static! {
    static ref APP_VERSION: Mutex<Option<&'static str>> = Mutex::new(None);
}

#[derive(Debug)]
pub struct ChannelState {
    channel: Channel,

    /// The set of additional features to enable (on top of default-enabled ones).
    additional_features: HashSet<FeatureFlag>,

    config: ChannelConfig,
}

impl ChannelState {
    pub fn init() -> Self {
        let channel = Channel::Oss;
        let app_id = AppId::new("dev", "warp", "WarpOss");
        Self {
            channel,
            additional_features: Default::default(),
            config: ChannelConfig {
                app_id,
                url_scheme: "warposs",
                logfile_name: "".into(),
                mcp_static_config: None,
            },
        }
    }

    pub fn new(channel: Channel, mut config: ChannelConfig) -> Self {
        if let Some(app_id) = app_id_from_bundle() {
            config.app_id = app_id;
        }
        Self {
            channel,
            additional_features: Default::default(),
            config,
        }
    }

    pub fn with_additional_features(mut self, overrides: &[FeatureFlag]) -> Self {
        self.additional_features.extend(overrides);
        self
    }

    pub fn set(state: ChannelState) {
        *CHANNEL_STATE.lock() = state;
    }

    pub fn is_release_bundle() -> bool {
        cfg!(feature = "release_bundle")
    }

    pub fn enable_debug_features() -> bool {
        cfg!(debug_assertions)
    }

    /// Returns the canonical identifier for the application.
    ///
    /// This should not be used for namespacing persisted data - such use cases
    /// should make use of [`Self::data_domain`] instead.
    pub fn app_id() -> AppId {
        CHANNEL_STATE.lock().config.app_id.clone()
    }

    /// Returns a profile name for isolating user data. This should be used to
    /// sandbox how user data is stored.
    ///
    /// This is a debugging tool for isolating development instances of Warp, and is not
    /// supported in release builds.
    pub fn data_profile() -> Option<String> {
        if cfg!(debug_assertions) {
            std::env::var("WARP_DATA_PROFILE").ok()
        } else {
            None
        }
    }

    /// Returns a value that should be used for namespacing persisted data.
    ///
    /// In release builds, this is identical to the app ID; in debug builds,
    /// it optionally includes a suffix derived from the `WARP_DATA_PROFILE`
    /// environment variable.
    pub fn data_domain() -> String {
        match Self::data_profile() {
            Some(profile) => format!("{}-{profile}", Self::app_id()),
            None => Self::app_id().to_string(),
        }
    }

    /// Returns the data domain if overridden from the default, otherwise None.
    pub fn data_domain_if_not_default() -> Option<String> {
        Self::data_profile().map(|_| Self::data_domain())
    }

    pub fn additional_features() -> HashSet<FeatureFlag> {
        CHANNEL_STATE
            .lock()
            .additional_features
            .iter()
            .cloned()
            .collect()
    }

    pub fn debug_str() -> String {
        format!("{:?}", *CHANNEL_STATE.lock())
    }

    pub fn logfile_name() -> Cow<'static, str> {
        CHANNEL_STATE.lock().config.logfile_name.clone()
    }

    pub fn channel() -> Channel {
        CHANNEL_STATE.lock().channel
    }

    #[cfg(feature = "test-util")]
    pub fn app_version() -> Option<&'static str> {
        let version = APP_VERSION.lock();

        version.or_else(|| option_env!("GIT_RELEASE_TAG"))
    }

    #[cfg(feature = "test-util")]
    pub fn set_app_version(version: Option<&'static str>) {
        *APP_VERSION.lock() = version;
    }

    #[cfg(not(feature = "test-util"))]
    pub fn app_version() -> Option<&'static str> {
        option_env!("GIT_RELEASE_TAG")
    }

    /// Returns the MCP OAuth provider config matching the given client ID, if any.
    pub fn mcp_oauth_provider_by_client_id(client_id: &str) -> Option<McpOAuthProviderConfig> {
        CHANNEL_STATE
            .lock()
            .config
            .mcp_static_config
            .as_ref()
            .and_then(|c| c.providers.iter().find(|p| p.client_id == client_id))
            .cloned()
    }

    /// Returns the MCP OAuth provider config matching the given issuer URL, if any.
    pub fn mcp_oauth_provider_by_issuer(issuer: &str) -> Option<McpOAuthProviderConfig> {
        CHANNEL_STATE
            .lock()
            .config
            .mcp_static_config
            .as_ref()
            .and_then(|c| c.providers.iter().find(|p| p.issuer == issuer))
            .cloned()
    }

    pub fn url_scheme() -> &'static str {
        CHANNEL_STATE.lock().config.url_scheme
    }
}

fn app_id_from_bundle() -> Option<AppId> {
    // On macOS, attempt to determine the app ID from the containing bundle,
    // falling back to the channel-keyed "default" ID if we cannot retrieve
    // bundle information.
    //
    // We skip this for tests, as the call to `mainBundle` can take 30+ms,
    // which is a significant portion of the total test runtime.
    #[cfg(all(target_os = "macos", not(feature = "test-util")))]
    {
        use objc2_foundation::NSBundle;

        let bundle = NSBundle::mainBundle();
        if let Some(bundle_identifier) = bundle.bundleIdentifier() {
            let app_id = bundle_identifier.to_string();
            if !app_id.is_empty() {
                return Some(
                    AppId::parse(&app_id)
                        .expect("macOS bundle identifier has an unexpected format"),
                );
            }
        }
    }

    None
}
