use std::time::SystemTime;

#[cfg(not(target_family = "wasm"))]
use futures::channel::oneshot;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp_core::send_telemetry_from_ctx;
use warp_errors::report_error;
use warp_multi_agent_api as api;
use warpui_core::{Entity, ModelContext, SingletonEntity};
use warpui_extras::secure_storage::{self, AppContextExt};

use crate::LLMProvider;
pub use crate::aws_credentials::{AwsCredentials, AwsCredentialsState};
#[cfg(not(target_family = "wasm"))]
pub use crate::geap_credentials::GeapRefreshOutcome;
pub use crate::geap_credentials::{
    GEAP_MINT_FAILURE_COOLDOWN, GEAP_REFRESH_LEAD_TIME, GeapCredentials, GeapCredentialsState,
    GeapFederation, GeapMintBinding, LoadGeapCredentialsError,
};
use crate::telemetry::{
    AITelemetryEvent, ProviderCredentialTelemetryAction, ProviderCredentialTelemetryKind,
    ProviderCredentialTelemetryProvider,
};

const SECURE_STORAGE_KEY: &str = "AiApiKeys";

/// Emitted when user-provided API keys are updated in-memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyManagerEvent {
    KeysUpdated,
}

/// User-provided API keys for AI providers.
///
/// These are used for "Bring Your Own API Key" functionality, allowing
/// users to use their own API keys instead of Warp's.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiKeys {
    pub google: Option<String>,
    pub anthropic: Option<String>,
    pub openai: Option<String>,
    pub open_router: Option<String>,
    pub custom_endpoints: Vec<CustomEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomEndpoint {
    pub name: String,
    pub url: String,
    pub api_key: String,
    pub models: Vec<CustomEndpointModel>,
    pub schema: CustomEndpointSchema,
}

/// The request/response protocol used by a custom inference endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomEndpointSchema {
    /// OpenAI Chat Completions, retained as the legacy/default protocol.
    #[default]
    OpenaiChatCompletions,
    /// OpenAI Responses.
    OpenaiResponses,
    /// Anthropic Messages.
    AnthropicMessages,
}

impl CustomEndpointSchema {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::OpenaiChatCompletions => "OpenAI Chat Completions",
            Self::OpenaiResponses => "OpenAI Responses",
            Self::AnthropicMessages => "Anthropic Messages",
        }
    }

    pub fn from_display_name(name: &str) -> Option<Self> {
        match name {
            "OpenAI Chat Completions" => Some(Self::OpenaiChatCompletions),
            "OpenAI Responses" => Some(Self::OpenaiResponses),
            "Anthropic Messages" => Some(Self::AnthropicMessages),
            _ => None,
        }
    }
    fn to_proto(self) -> api::request::settings::custom_model_providers::CustomEndpointSchema {
        match self {
            Self::OpenaiChatCompletions => {
                api::request::settings::custom_model_providers::CustomEndpointSchema::OpenaiChatCompletions
            }
            Self::OpenaiResponses => {
                api::request::settings::custom_model_providers::CustomEndpointSchema::OpenaiResponses
            }
            Self::AnthropicMessages => {
                api::request::settings::custom_model_providers::CustomEndpointSchema::AnthropicMessages
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomEndpointModel {
    pub name: String,
    pub alias: Option<String>,
    /// Stable identifier used as `ModelConfig.{base,coding,cli_agent,computer_use_agent}` and
    /// as the `CustomModelProviders.providers[*].models[*].config_key` on the request wire.
    /// Generated as a UUIDv4 at model creation.
    pub config_key: String,
}

impl CustomEndpointModel {
    /// Picker label: prefer the user-provided alias; fall back to the raw model name
    /// so a row is never blank.
    pub fn display_label(&self) -> &str {
        match self.alias.as_deref() {
            Some(alias) if !alias.trim().is_empty() => alias,
            _ => &self.name,
        }
    }
}

impl ApiKeys {
    pub fn has_any_key(&self) -> bool {
        self.openai.is_some()
            || self.anthropic.is_some()
            || self.google.is_some()
            || self.open_router.is_some()
            || self
                .custom_endpoints
                .iter()
                .any(|endpoint| !endpoint.api_key.trim().is_empty())
    }

    /// Number of single-provider API keys currently configured (OpenAI,
    /// Anthropic, Google, OpenRouter). Custom endpoints are counted separately
    /// via `custom_endpoints`.
    pub fn provider_key_count(&self) -> usize {
        [
            &self.openai,
            &self.anthropic,
            &self.google,
            &self.open_router,
        ]
        .into_iter()
        .filter(|key| key.as_deref().is_some_and(|v| !v.trim().is_empty()))
        .count()
    }
}

/// A structure that manages API keys for AI providers.
pub struct ApiKeyManager {
    keys: ApiKeys,
    /// Coordinates request-time GEAP refreshes. Installed by the mint kickoff
    /// itself (see `install_geap_refresh_waiter`) immediately before the state
    /// transitions to `Refreshing`, and taken when the mint completes, so
    /// `Some` means a mint is in flight *by construction* rather than by
    /// convention. Holds the completion senders for requests blocked on it;
    /// may be empty for a proactive mint with no waiters.
    #[cfg(not(target_family = "wasm"))]
    pub(crate) geap_refresh_waiters: Option<Vec<oneshot::Sender<GeapRefreshOutcome>>>,
    /// When the last GEAP mint failed, if one has. The timestamp is what
    /// suppresses repeated request-time waits.
    #[cfg(not(target_family = "wasm"))]
    pub(crate) geap_last_mint_failure: Option<SystemTime>,
    pub(crate) aws_credentials_state: AwsCredentialsState,
    /// In-memory Gemini Enterprise (GEAP) credential state.
    pub(crate) geap_credentials_state: GeapCredentialsState,
    secure_storage_write_version: u64,
}

pub struct CustomEndpointParams {
    pub name: String,
    pub url: String,
    pub api_key: String,
    pub models: Vec<(String, Option<String>, Option<String>)>,
    pub schema: CustomEndpointSchema,
}
fn provider_credential_action(is_present: bool) -> ProviderCredentialTelemetryAction {
    if is_present {
        ProviderCredentialTelemetryAction::Added
    } else {
        ProviderCredentialTelemetryAction::Removed
    }
}

fn provider_telemetry_provider(
    provider: LLMProvider,
) -> Option<ProviderCredentialTelemetryProvider> {
    match provider {
        LLMProvider::OpenAI => Some(ProviderCredentialTelemetryProvider::OpenAi),
        LLMProvider::Anthropic => Some(ProviderCredentialTelemetryProvider::Anthropic),
        LLMProvider::Google => Some(ProviderCredentialTelemetryProvider::Google),
        LLMProvider::Unknown => None,
    }
}

fn send_provider_credential_telemetry(
    provider: LLMProvider,
    credential_kind: ProviderCredentialTelemetryKind,
    action: ProviderCredentialTelemetryAction,
    ctx: &mut ModelContext<ApiKeyManager>,
) {
    let Some(provider) = provider_telemetry_provider(provider) else {
        return;
    };
    send_telemetry_from_ctx!(
        AITelemetryEvent::ProviderCredentialChanged {
            provider,
            credential_kind,
            action,
        },
        ctx
    );
}

impl ApiKeyManager {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let keys = Self::load_keys_from_secure_storage(ctx);
        Self {
            keys,
            #[cfg(not(target_family = "wasm"))]
            geap_refresh_waiters: None,
            #[cfg(not(target_family = "wasm"))]
            geap_last_mint_failure: None,
            aws_credentials_state: AwsCredentialsState::Missing,
            geap_credentials_state: GeapCredentialsState::Missing,
            secure_storage_write_version: 0,
        }
    }

    pub fn keys(&self) -> &ApiKeys {
        &self.keys
    }

    /// Reloads API keys after another process updates the active secure-storage namespace.
    ///
    /// GUI edits mutate this manager directly before persisting, so they do not
    /// need to reload. TUI setup commands run in a separate process and notify
    /// the live TUI to refresh its cached keys after a successful write.
    pub fn reload_keys_from_secure_storage(&mut self, ctx: &mut ModelContext<Self>) {
        let keys = Self::load_keys_from_secure_storage(ctx);
        if self.keys == keys {
            return;
        }
        self.keys = keys;
        ctx.emit(ApiKeyManagerEvent::KeysUpdated);
    }

    /// Persists a provider API key before publishing the updated in-memory value.
    pub fn persist_provider_key(
        &mut self,
        provider: LLMProvider,
        key: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) -> anyhow::Result<()> {
        let was_present = provider.api_key(&self.keys).is_some();
        let mut keys = self.keys.clone();
        if !provider.set_api_key(&mut keys, key) {
            return Err(anyhow::anyhow!(
                "{} does not support pasted API keys",
                provider.display_name()
            ));
        }
        let json = serde_json::to_string(&keys)
            .map_err(|error| anyhow::Error::new(error).context("Failed to serialize API keys"))?;
        ctx.secure_storage()
            .write_value(SECURE_STORAGE_KEY, &json)
            .map_err(|error| {
                anyhow::Error::new(error).context("Failed to write API keys to secure storage")
            })?;
        if self.keys != keys {
            let is_present = provider.api_key(&keys).is_some();
            self.keys = keys;
            ctx.emit(ApiKeyManagerEvent::KeysUpdated);
            if was_present != is_present {
                send_provider_credential_telemetry(
                    provider,
                    ProviderCredentialTelemetryKind::PastedKey,
                    provider_credential_action(is_present),
                    ctx,
                );
            }
        }
        Ok(())
    }

    /// Returns `true` when the user has any usable BYO credential: a pasted
    /// provider or custom-endpoint key.
    pub fn has_any_key(&self) -> bool {
        self.keys.has_any_key()
    }

    pub fn set_provider_key(
        &mut self,
        provider: LLMProvider,
        key: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) {
        let was_present = provider.api_key(&self.keys).is_some();
        if !provider.set_api_key(&mut self.keys, key) {
            return;
        }
        ctx.emit(ApiKeyManagerEvent::KeysUpdated);
        self.write_keys_to_secure_storage(ctx);
        let is_present = provider.api_key(&self.keys).is_some();
        if was_present != is_present {
            send_provider_credential_telemetry(
                provider,
                ProviderCredentialTelemetryKind::PastedKey,
                provider_credential_action(is_present),
                ctx,
            );
        }
    }

    pub fn add_custom_endpoint(
        &mut self,
        params: CustomEndpointParams,
        ctx: &mut ModelContext<Self>,
    ) {
        let CustomEndpointParams {
            name,
            url,
            api_key,
            models,
            schema,
        } = params;
        self.keys.custom_endpoints.push(CustomEndpoint {
            name,
            url,
            api_key,
            schema,
            models: models
                .into_iter()
                .map(|(name, alias, config_key)| CustomEndpointModel {
                    name,
                    alias,
                    config_key: config_key
                        .filter(|k| !k.is_empty())
                        .unwrap_or_else(|| Uuid::new_v4().to_string()),
                })
                .collect(),
        });
        ctx.emit(ApiKeyManagerEvent::KeysUpdated);
        self.write_keys_to_secure_storage(ctx);
    }

    pub fn save_custom_endpoint(
        &mut self,
        index: usize,
        params: CustomEndpointParams,
        ctx: &mut ModelContext<Self>,
    ) {
        if index >= self.keys.custom_endpoints.len() {
            return;
        }
        let CustomEndpointParams {
            name,
            url,
            api_key,
            models,
            schema,
        } = params;
        self.keys.custom_endpoints[index] = CustomEndpoint {
            name,
            url,
            api_key,
            schema,
            models: models
                .into_iter()
                .map(|(name, alias, config_key)| CustomEndpointModel {
                    name,
                    alias,
                    config_key: config_key
                        .filter(|k| !k.is_empty())
                        .unwrap_or_else(|| Uuid::new_v4().to_string()),
                })
                .collect(),
        };
        ctx.emit(ApiKeyManagerEvent::KeysUpdated);
        self.write_keys_to_secure_storage(ctx);
    }

    pub fn remove_custom_endpoint(&mut self, index: usize, ctx: &mut ModelContext<Self>) {
        if index >= self.keys.custom_endpoints.len() {
            return;
        }
        self.keys.custom_endpoints.remove(index);
        ctx.emit(ApiKeyManagerEvent::KeysUpdated);
        self.write_keys_to_secure_storage(ctx);
    }

    pub fn clear_custom_endpoints(&mut self, ctx: &mut ModelContext<Self>) {
        if self.keys.custom_endpoints.is_empty() {
            return;
        }
        self.keys.custom_endpoints.clear();
        ctx.emit(ApiKeyManagerEvent::KeysUpdated);
        self.write_keys_to_secure_storage(ctx);
    }

    pub fn set_aws_credentials_state(
        &mut self,
        state: AwsCredentialsState,
        ctx: &mut ModelContext<Self>,
    ) {
        self.aws_credentials_state = state;
        ctx.emit(ApiKeyManagerEvent::KeysUpdated);
    }

    pub fn aws_credentials_state(&self) -> &AwsCredentialsState {
        &self.aws_credentials_state
    }

    /// Builds the `CustomModelProviders` registry that ships with every agent request.
    ///
    /// Emits one [`CustomModelProvider`] per configured [`CustomEndpoint`], each populated with
    /// all of its [`CustomEndpointModel`]s. The per-model `config_key` is what the server uses
    /// to map a `ModelConfig.{base,coding,cli_agent,computer_use_agent}` selection back to a
    /// user-provided endpoint, so it MUST be the same UUID we store locally.
    ///
    /// Returns `None` when custom models should not be included or no endpoint has both a
    /// non-empty URL and API key.
    pub fn custom_model_providers_for_request(
        &self,
        include_custom_models: bool,
    ) -> Option<api::request::settings::CustomModelProviders> {
        if !include_custom_models {
            return None;
        }

        let providers: Vec<_> = self
            .keys
            .custom_endpoints
            .iter()
            .filter(|endpoint| !endpoint.url.trim().is_empty() && !endpoint.api_key.is_empty())
            .map(
                |endpoint| api::request::settings::custom_model_providers::CustomModelProvider {
                    base_url: endpoint.url.clone(),
                    api_key: endpoint.api_key.clone(),
                    schema: endpoint.schema.to_proto() as i32,
                    models: endpoint
                        .models
                        .iter()
                        .filter(|m| !m.name.trim().is_empty() && !m.config_key.is_empty())
                        .map(
                            |m| api::request::settings::custom_model_providers::CustomModel {
                                slug: m.name.clone(),
                                config_key: m.config_key.clone(),
                            },
                        )
                        .collect(),
                },
            )
            .filter(|provider| !provider.models.is_empty())
            .collect();

        if providers.is_empty() {
            None
        } else {
            Some(api::request::settings::CustomModelProviders { providers })
        }
    }

    pub fn api_keys_for_request(
        &self,
        include_byo_keys: bool,
        include_aws_bedrock_credentials: bool,
        geap_binding: Option<GeapMintBinding>,
    ) -> Option<api::request::settings::ApiKeys> {
        let anthropic = include_byo_keys
            .then(|| self.keys.anthropic.clone())
            .flatten()
            .unwrap_or_default();
        let openai = include_byo_keys
            .then(|| self.keys.openai.clone())
            .flatten()
            .unwrap_or_default();
        let google = include_byo_keys
            .then(|| self.keys.google.clone())
            .flatten()
            .unwrap_or_default();
        let open_router = include_byo_keys
            .then(|| self.keys.open_router.clone())
            .flatten()
            .unwrap_or_default();

        let aws_credentials = include_aws_bedrock_credentials
            .then(|| match self.aws_credentials_state {
                AwsCredentialsState::Loaded {
                    ref credentials, ..
                } => Some(credentials.clone().into()),
                _ => None,
            })
            .flatten();

        // Gemini Enterprise (GEAP) credentials attach only when the caller's
        // gate is on AND the stored token was minted for that same
        // (user, audience, SA) binding. `geap_credentials_for_request` is the
        // single source of truth for that rule (see `crate::geap_credentials`).
        let google_cloud_credentials = geap_binding
            .as_ref()
            .and_then(|binding| self.geap_credentials_for_request(binding));

        if anthropic.is_empty()
            && openai.is_empty()
            && google.is_empty()
            && open_router.is_empty()
            && aws_credentials.is_none()
            && google_cloud_credentials.is_none()
        {
            None
        } else {
            Some(api::request::settings::ApiKeys {
                anthropic,
                openai,
                google,
                open_router,
                // Grok subscription OAuth was removed; the proto field stays.
                grok_oauth_access_token: String::new(),
                allow_use_of_warp_credits: false,
                aws_credentials,
                google_cloud_credentials,
            })
        }
    }

    fn load_keys_from_secure_storage(ctx: &mut ModelContext<Self>) -> ApiKeys {
        let key_json = match ctx.secure_storage().read_value(SECURE_STORAGE_KEY) {
            Ok(json) => json,
            Err(e) => {
                if !matches!(e, secure_storage::Error::NotFound) {
                    report_error!(
                        anyhow::Error::new(e)
                            .context("Failed to read API keys from secure storage")
                    );
                }
                return ApiKeys::default();
            }
        };

        match serde_json::from_str(&key_json) {
            Ok(keys) => keys,
            Err(e) => {
                report_error!(anyhow::Error::new(e).context("Failed to deserialize API keys"));
                ApiKeys::default()
            }
        }
    }

    fn write_keys_to_secure_storage(&mut self, ctx: &mut ModelContext<Self>) {
        let json = match serde_json::to_string(&self.keys) {
            Ok(json) => json,
            Err(e) => {
                report_error!(anyhow::Error::new(e).context("Failed to serialize API keys"));
                return;
            }
        };
        self.secure_storage_write_version += 1;
        let write_version = self.secure_storage_write_version;

        // Defer the keychain write so it doesn't block the current event
        // processing. The in-memory state is already updated and events
        // already emitted, so the UI updates immediately while the
        // potentially slow platform secure-storage call runs in a
        // subsequent main-thread callback. Skip stale callbacks so older
        // writes cannot complete after and overwrite a newer payload.
        ctx.spawn(async move { json }, move |me, json, ctx| {
            if write_version != me.secure_storage_write_version {
                return;
            }
            if let Err(e) = ctx.secure_storage().write_value(SECURE_STORAGE_KEY, &json) {
                report_error!(
                    anyhow::Error::new(e).context("Failed to write API keys to secure storage")
                );
            }
        });
    }
}

impl Entity for ApiKeyManager {
    type Event = ApiKeyManagerEvent;
}

impl SingletonEntity for ApiKeyManager {}

#[cfg(test)]
#[path = "api_keys_tests.rs"]
mod tests;
