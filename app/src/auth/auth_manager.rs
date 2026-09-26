use std::sync::Arc;

use warpui::{Entity, ModelContext, SingletonEntity};

use super::AuthStateProvider;
use super::auth_state::AuthState;

#[derive(Debug)]
pub enum AuthManagerEvent {
    /// The user's credentials have become invalid and they need to reauthenticate.
    NeedsReauth,
    /// The user is anonymous and has attempted to access a login-gated feature or link.
    AttemptedLoginGatedFeature,
}

/// AuthManager is a singleton model for auth-related events. If you need to
/// access the auth state, use `AuthStateProvider`.
pub struct AuthManager {
    auth_state: Arc<AuthState>,
}

impl AuthManager {
    /// Creates a new instance of the AuthManager. The auth state must already be initialized through
    /// [`AuthStateProvider`].
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let auth_state = AuthStateProvider::as_ref(ctx).get().clone();

        Self { auth_state }
    }

    #[cfg(test)]
    pub fn new_for_test(ctx: &mut ModelContext<Self>) -> Self {
        Self::new(ctx)
    }

    /// Sets whether or not the user needs to reauth. Emits [`AuthManagerEvent::NeedsReauth`]
    /// on the transition to needing a reauth.
    pub fn set_needs_reauth(&self, needs_reauth: bool, ctx: &mut ModelContext<Self>) {
        let became_true = self.auth_state.set_needs_reauth(needs_reauth);

        if became_true {
            ctx.emit(AuthManagerEvent::NeedsReauth);
        }
    }

    pub fn attempt_login_gated_feature(&self, ctx: &mut ModelContext<Self>) {
        if self.auth_state.is_anonymous_or_logged_out() {
            ctx.emit(AuthManagerEvent::AttemptedLoginGatedFeature);
        };
    }

    pub fn anonymous_user_hit_drive_object_limit(&self, ctx: &mut ModelContext<Self>) {
        if self.auth_state.is_anonymous_or_logged_out() {
            ctx.emit(AuthManagerEvent::AttemptedLoginGatedFeature);
        };
    }

    /// Sets the user as onboarded locally.
    pub fn set_user_onboarded(&self) {
        self.auth_state.set_is_onboarded(true);
    }
}

impl Entity for AuthManager {
    type Event = AuthManagerEvent;
}

impl SingletonEntity for AuthManager {}
