use std::result::Result as StdResult;
use std::sync::Arc;

use settings::Setting as _;
use uuid::Uuid;
use warp_core::channel::ChannelState;
use warp_errors::{report_error, report_if_error};
use warp_server_auth::API_KEY_PREFIX;
use warp_server_auth::user::persistence::PersistedUser;
use warpui::{Entity, ModelContext, SingletonEntity, UpdateModel};

use super::auth_state::{AuthState, PersistAction};
use super::auth_view_modal::AuthRedirectPayload;
use super::credentials::{Credentials, FirebaseToken, LoginToken};
use super::user::User;
use super::user_properties::UserProperties;
use super::{AuthStateProvider, UserUid};
use crate::ai::AIRequestUsageModel;
use crate::ai::llms::LLMPreferences;
use crate::persistence::ModelEvent;
use crate::server::server_api::ServerApi;
use crate::server::server_api::auth::{AuthClient, FetchUserResult, UserAuthenticationError};
use crate::settings::PrivacySettings;
use crate::settings::cloud_preferences_syncer::CloudPreferencesSyncer;
use crate::settings::initializer::SettingsInitializer;
use crate::terminal::general_settings::GeneralSettings;
use crate::{GlobalResourceHandlesProvider, TelemetryEvent, persistence, send_telemetry_from_ctx};

#[derive(Debug)]
pub enum AuthManagerEvent {
    /// Successfully authenticated a user with no errors.
    AuthComplete,
    /// Failed to authenticate a user, due to a particular `UserAuthenticationError`.
    AuthFailed(UserAuthenticationError),
    /// The user now needs to reauthenticate. If the user needs to reauth, an `AuthFailed`
    /// event might be triggered instead, but there are some code paths where we don't
    /// refresh the entire user, only their token, which is when this event might be emitted.
    NeedsReauth,
    /// The user is anonymous and has attempted to access a login-gated feature or link.
    AttemptedLoginGatedFeature,
    // The current user is anonymous and the client has received a browser intent to sign in with a different Warp account.
    // Holds an auth payload from the received browser intent.
    LoginOverrideDetected(AuthRedirectPayload),
}

pub type LoginGatedFeature = &'static str;

/// AuthManager is a singleton model which manages the currently logged-in user's state.
/// If you need to access the state, use `AuthStateProvider`.
pub struct AuthManager {
    auth_state: Arc<AuthState>,
    server_api: Arc<ServerApi>,
    auth_client: Arc<dyn AuthClient>,
    /// A generated state token that the web app must provide back to the client.
    pending_auth_state: Option<String>,
}

impl AuthManager {
    /// Creates a new instance of the AuthManager. The auth state must already be initialized through
    /// [`AuthStateProvider`].
    pub fn new(
        server_api: Arc<ServerApi>,
        auth_client: Arc<dyn AuthClient>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let auth_state = AuthStateProvider::as_ref(ctx).get().clone();

        Self {
            auth_state,
            server_api,
            auth_client,
            pending_auth_state: None,
        }
    }

    #[cfg(test)]
    pub fn new_for_test(ctx: &mut ModelContext<Self>) -> Self {
        use crate::server::server_api::ServerApiProvider;

        let server_api_provider = ServerApiProvider::as_ref(ctx);
        let server_api = server_api_provider.get();
        let auth_client = server_api_provider.get_auth_client();
        let auth_state = AuthStateProvider::as_ref(ctx).get().clone();

        Self {
            auth_state,
            server_api,
            auth_client,
            pending_auth_state: None,
        }
    }

    /// Fetches and ultimately sets the user's auth state from an auth payload.
    /// Typically, this function is triggered when a user clicks the intent link from their browser
    /// back to Warp after login (or pastes the URL in the app).
    pub fn initialize_user_from_auth_payload(
        &mut self,
        auth_payload: AuthRedirectPayload,
        enforce_state_validation: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let AuthRedirectPayload {
            refresh_token,
            user_uid,
            deleted_anonymous_user,
            state,
        } = auth_payload.clone();

        if let Some(received_state) = &state {
            if !self.consume_auth_state(received_state) {
                if self.should_silently_ignore_stale_redirect(&user_uid) {
                    log::info!(
                        "Dropping auth redirect with stale state for already-logged-in user"
                    );
                    return;
                }
                ctx.emit(AuthManagerEvent::AuthFailed(
                    UserAuthenticationError::InvalidStateParameter,
                ));
                return;
            }
        } else if enforce_state_validation {
            if self.should_silently_ignore_stale_redirect(&user_uid) {
                log::info!("Dropping auth redirect without state for already-logged-in user");
                return;
            }
            ctx.emit(AuthManagerEvent::AuthFailed(
                UserAuthenticationError::MissingStateParameter,
            ));
            return;
        }

        let auth_client = self.auth_client.clone();

        if self.auth_state.is_user_anonymous().unwrap_or_default() {
            let incoming_user_matches_current_user = match user_uid {
                None => false,
                Some(incoming_user_uid) => self
                    .auth_state
                    .user_id()
                    .map(|current_user_uid| current_user_uid == incoming_user_uid)
                    .unwrap_or_default(),
            };
            if !incoming_user_matches_current_user && !deleted_anonymous_user.unwrap_or_default() {
                ctx.emit(AuthManagerEvent::LoginOverrideDetected(auth_payload));
                return;
            }
            send_telemetry_from_ctx!(TelemetryEvent::AnonymousUserLinkedFromBrowser, ctx);
        }

        let _ = ctx.spawn(
            async move {
                auth_client
                    .fetch_user(
                        LoginToken::Firebase(FirebaseToken::Refresh(refresh_token)),
                        false, /* for_refresh */
                    )
                    .await
            },
            Self::on_user_fetched,
        );
    }

    pub fn resume_interrupted_auth_payload(
        &mut self,
        auth_payload: AuthRedirectPayload,
        ctx: &mut ModelContext<Self>,
    ) {
        let AuthRedirectPayload {
            refresh_token,
            user_uid: _,
            deleted_anonymous_user: _,
            state: _,
        } = auth_payload;

        let auth_client = self.auth_client.clone();

        let _ = ctx.spawn(
            async move {
                auth_client
                    .fetch_user(
                        LoginToken::Firebase(FirebaseToken::Refresh(refresh_token)),
                        false, /* for_refresh */
                    )
                    .await
            },
            Self::on_user_fetched,
        );
    }

    #[cfg(target_family = "wasm")]
    pub fn initialize_user_from_session_cookie(&self, ctx: &mut ModelContext<Self>) {
        let auth_client = self.auth_client.clone();
        let _ = ctx.spawn(
            async move {
                auth_client
                    .fetch_user(LoginToken::SessionCookie, false)
                    .await
            },
            Self::on_user_fetched,
        );
    }

    /// Refreshes the user's auth state using their existing credentials.
    pub fn refresh_user(&self, ctx: &mut ModelContext<Self>) {
        let Some(credentials) = self.auth_state.credentials() else {
            log::warn!("Attempted to refresh user without credentials");
            return;
        };

        let Some(token) = credentials.login_token() else {
            log::info!("Attempted to refresh a user with no login token, skipping");
            return;
        };

        let auth_client = self.auth_client.clone();
        let _ = ctx.spawn(
            async move { auth_client.fetch_user(token, true).await },
            Self::on_user_fetched,
        );
    }
    /// Validates a startup API key without exposing it through shared auth state.
    ///
    /// [`Self::on_user_fetched`] promotes the returned user and credentials only
    /// after the server accepts the key. A failed request leaves the client
    /// fully logged out.
    pub fn authenticate_api_key(&self, api_key: String, ctx: &mut ModelContext<Self>) {
        log::info!("Authenticating via pending API key");
        let api_key = if api_key.starts_with(API_KEY_PREFIX) {
            api_key
        } else {
            format!("{API_KEY_PREFIX}{api_key}")
        };
        let auth_client = self.auth_client.clone();
        let _ = ctx.spawn(
            async move {
                auth_client
                    .fetch_user(LoginToken::ApiKey(api_key), false)
                    .await
            },
            Self::on_user_fetched,
        );
    }

    /// Callback for handling a successful fetch of a user from warp-server and Firebase.
    /// This does the heavy-lifting of setting up all components of the application that depend
    /// on a user's authenticated state, and emits events to subscribers that let them know
    /// an auth event has occurred.
    fn on_user_fetched(
        &mut self,
        fetch_user_result: StdResult<FetchUserResult, UserAuthenticationError>,
        ctx: &mut ModelContext<Self>,
    ) {
        match fetch_user_result {
            Ok(fetch_user_result) => {
                let FetchUserResult {
                    user_output,
                    credentials,
                    ..
                } = fetch_user_result;
                let UserProperties { user, llms } = user_output.into();

                self.complete_authentication(user.clone(), credentials, ctx);

                self.set_needs_reauth(false, ctx);

                // Must be called on the main thread.
                #[cfg(feature = "crash_reporting")]
                crate::crash_reporting::set_user_id(
                    user.local_id,
                    Some(user.metadata.email.clone()),
                    ctx,
                );

                SettingsInitializer::handle(ctx).update(ctx, |initializer, ctx| {
                    initializer.handle_user_fetched(self.auth_state.clone(), ctx);
                });

                // Reset the initial-load condition so that any cloud preference
                CloudPreferencesSyncer::handle(ctx).update(ctx, |model, ctx| {
                    model.handle_user_fetched(self.auth_state.clone(), ctx)
                });

                AIRequestUsageModel::handle(ctx).update(ctx, |usage_model, ctx| {
                    usage_model.refresh_request_usage_async(ctx);
                });

                LLMPreferences::handle(ctx).update(ctx, |prefs, ctx| {
                    prefs.update_feature_model_choices(Ok(llms), ctx);
                });

                if !user.is_user_anonymous() {
                    GeneralSettings::handle(ctx).update(ctx, |settings, ctx| {
                        report_if_error!(
                            settings.did_non_anonymous_user_log_in.set_value(true, ctx)
                        );
                    });
                }

                let global_resource_handles =
                    GlobalResourceHandlesProvider::as_ref(ctx).get().clone();

                // As part of Logout v0:
                // Reconstruct the database if it was removed.
                // Do nothing if the database was not removed.
                persistence::reconstruct(&global_resource_handles.model_event_sender);
                if let Some(model_event_sender) = &global_resource_handles.model_event_sender
                    && let Err(e) =
                        model_event_sender.send(ModelEvent::UpsertCurrentUserInformation {
                            user_information: PersistedCurrentUserInformation {
                                email: self.auth_state.user_email().unwrap_or_default(),
                            },
                        })
                {
                    report_error!(
                        anyhow::Error::new(e)
                            .context("Error persisting user information to database")
                    );
                };

                // Fetch the user's privacy settings from the server if any or update the server settings.
                let privacy_settings_handle = PrivacySettings::handle(ctx);
                let privacy_settings_snapshot =
                    privacy_settings_handle.as_ref(ctx).get_snapshot(ctx);
                ctx.update_model(&privacy_settings_handle, |privacy_settings, ctx| {
                    privacy_settings.fetch_or_update_settings(ctx);
                });

                let server_api = self.server_api.clone();
                let user_id = self.auth_state.user_id().unwrap_or_default();
                let anonymous_id = self.auth_state.anonymous_id();
                let _ = ctx.spawn(
                    // Synchronously add the identify and login event to the telemetry event queue and
                    // then flush the queue to ensure the events get to Rudderstack. We need to do this
                    // one-off because the login event happens only once for the user and we don't want
                    // to drop the event if the user quits the app before the next flush of the queue.
                    // TODO(alokedesai): Investigate a more robust way of handling events
                    // that don't get flushed to Rudderstack outside of this event specifically.
                    async move {
                        warpui::telemetry::record_identify_user_event(
                            user_id.as_string(),
                            anonymous_id.clone(),
                            warpui::time::get_current_time(),
                        );
                        warpui::telemetry::record_event(
                            Some(user_id.as_string()),
                            anonymous_id,
                            TelemetryEvent::Login.name().into(),
                            TelemetryEvent::Login.payload(),
                            TelemetryEvent::Login.contains_ugc(),
                            warpui::time::get_current_time(),
                        );

                        // Note that this snapshot might get overwritten to disabled after the server fetch.
                        // However, it is still fine to flush to Rudderstack here as the login event is low-risk
                        // and it is better to err on the side of over-reporting than under-reporting.
                        if let Err(e) = server_api
                            .flush_telemetry_events(privacy_settings_snapshot)
                            .await
                        {
                            log::info!("Failed to flush events from Telemetry queue: {e}");
                        }
                        server_api.notify_login().await;
                    },
                    |_, _, _| {},
                );

                // Once the user is authenticated, attempt to report the sandbox that Warp is running in, if any.
                ctx.spawn(
                    async { warp_isolation_platform::detect() },
                    |_, platform, ctx| {
                        if let Some(platform) = platform {
                            send_telemetry_from_ctx!(
                                TelemetryEvent::DetectedIsolationPlatform { platform },
                                ctx
                            );
                        }
                    },
                );

                ctx.emit(AuthManagerEvent::AuthComplete);
            }
            Err(error) => {
                match error {
                    UserAuthenticationError::DeniedAccessToken(_) => {
                        self.set_needs_reauth(true, ctx);
                    }
                    UserAuthenticationError::UserAccountDisabled(_) => {}
                    UserAuthenticationError::DeviceCodeRequestTimedOut { .. } => {}
                    UserAuthenticationError::Unexpected(_) => {}
                    UserAuthenticationError::InvalidStateParameter => {}
                    UserAuthenticationError::MissingStateParameter => {}
                }

                ctx.emit(AuthManagerEvent::AuthFailed(error));
            }
        }
    }

    /// Sets the user and credentials in auth state and persists to secure storage.
    /// Persistence depends on the credential type - currently, we only persist
    /// state if authenticated via a Firebase token.
    fn complete_authentication(
        &self,
        user: User,
        credentials: Credentials,
        ctx: &mut ModelContext<Self>,
    ) {
        self.set_and_persist(Some(user), Some(credentials), ctx);
    }
    fn set_and_persist(
        &self,
        user: Option<User>,
        credentials: Option<Credentials>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.auth_state.set_user(user);
        self.auth_state.set_credentials(credentials);
        self.persist(ctx);
    }

    /// Persists (or removes) the current user and credentials to/from secure storage,
    /// based on the current auth state.
    fn persist(&self, ctx: &mut ModelContext<Self>) {
        match self.auth_state.persist_action() {
            PersistAction::Persist(persisted_user) => {
                if persisted_user.auth_tokens.refresh_token.is_empty() {
                    log::warn!("Skipping user persistence due to empty refresh token");
                    return;
                }
                let _ = persisted_user.write_to_secure_storage(ctx).map_err(|err| {
                    log::warn!("Unable to persist user to secure storage: {err:?}");
                });
            }
            PersistAction::Remove => {
                let _ = PersistedUser::remove_from_secure_storage(ctx).map_err(|err| {
                    log::warn!("Unable to clear user from secure storage: {err:?}");
                });
            }
            PersistAction::DoNothing => {}
        }
    }

    /// Helper function for logging out the user.
    /// NOTE: You probably want to call auth::log_out instead; this only manages the auth state,
    /// it doesn't shut down any other user-dependent parts of the app.
    /// TODO(jeff): Can we move those pieces in here?
    pub(super) fn log_out(&mut self, ctx: &mut ModelContext<Self>) {
        // Clear any dangling CSRF token from an auth flow that was started but never
        // completed before this logout, so it can't be replayed against the next session
        // in the same process.
        self.pending_auth_state = None;
        self.set_and_persist(None, None, ctx);
    }

    /// Sets whether or not this user's Firebase credentials are invalid and thus needs to reauth.
    pub fn set_needs_reauth(&self, needs_reauth: bool, ctx: &mut ModelContext<Self>) {
        let became_true = self.auth_state.set_needs_reauth(needs_reauth);

        if became_true {
            send_telemetry_from_ctx!(TelemetryEvent::NeedsReauth, ctx);
            ctx.emit(AuthManagerEvent::NeedsReauth);
        }
    }

    pub fn attempt_login_gated_feature(
        &self,
        feature: LoginGatedFeature,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.auth_state.is_anonymous_or_logged_out() {
            send_telemetry_from_ctx!(
                TelemetryEvent::AnonymousUserAttemptLoginGatedFeature { feature },
                ctx
            );
            ctx.emit(AuthManagerEvent::AttemptedLoginGatedFeature);
        };
    }

    pub fn anonymous_user_hit_drive_object_limit(&self, ctx: &mut ModelContext<Self>) {
        if self.auth_state.is_anonymous_or_logged_out() {
            send_telemetry_from_ctx!(TelemetryEvent::AnonymousUserHitCloudObjectLimit, ctx);
            ctx.emit(AuthManagerEvent::AttemptedLoginGatedFeature);
        };
    }

    /// Generates a unique state parameter for the authentication flow.
    fn generate_auth_state(&mut self) -> String {
        let state = Uuid::new_v4().to_string();
        self.pending_auth_state = Some(state.clone());
        state
    }

    pub fn sign_in_url(&mut self) -> String {
        let state = self.generate_auth_state();
        format!(
            "{}/login/remote?scheme={}&state={}",
            ChannelState::server_root_url(),
            ChannelState::url_scheme(),
            state,
        )
    }

    /// The upgrade confirmation page will kick the user back to the app with a refresh token
    /// if we send a `state` query param to /upgrade
    pub fn upgrade_url(&mut self) -> String {
        let state = self.generate_auth_state();
        format!(
            "{}/upgrade?scheme={}&state={}",
            ChannelState::server_root_url(),
            ChannelState::url_scheme(),
            state,
        )
    }

    /// Validates and consumes the pending auth state token. Returns `true` if the
    /// provided state matches; in that case the pending state is cleared so the
    /// CSRF token is single-use. A subsequent call with the same value will fail.
    fn consume_auth_state(&mut self, received_state: &str) -> bool {
        if self.pending_auth_state.as_deref() == Some(received_state) {
            self.pending_auth_state = None;
            true
        } else {
            false
        }
    }

    /// Returns whether an auth redirect that failed state validation should be
    /// silently dropped rather than surfaced as an error. This covers the
    /// "user clicks the browser's 'Take me to Warp' button twice" case: once
    /// they're fully logged in, a second redirect targeting the same user is
    /// redundant and should not produce a user-visible error.
    fn should_silently_ignore_stale_redirect(&self, incoming_user_uid: &Option<UserUid>) -> bool {
        if self.auth_state.is_anonymous_or_logged_out() {
            return false;
        }
        match (self.auth_state.user_id(), incoming_user_uid) {
            (Some(current_uid), Some(incoming_uid)) => current_uid == *incoming_uid,
            _ => false,
        }
    }

    /// Sets the user as onboarded both on the server and locally.
    /// This method:
    /// 1. Updates the server by calling set_user_is_onboarded
    /// 2. Updates the local auth state and persists the user data
    pub fn set_user_onboarded(&self, ctx: &mut ModelContext<Self>) {
        // Update server
        let auth_client = self.auth_client.clone();
        let _ = ctx.spawn(
            async move { auth_client.set_user_is_onboarded().await },
            |_, _, _| {},
        );

        // Update local auth state and persist
        self.auth_state.set_is_onboarded(true);

        self.persist(ctx);
    }
}

#[derive(Clone, Debug)]
pub struct PersistedCurrentUserInformation {
    pub email: String,
}

impl Entity for AuthManager {
    type Event = AuthManagerEvent;
}

impl SingletonEntity for AuthManager {}

#[cfg(test)]
#[path = "auth_manager_tests.rs"]
mod auth_manager_test;
