use warpui::{App, SingletonEntity};

use super::AuthManager;
use crate::ServerApiProvider;
use crate::auth::credentials::Credentials;
use crate::auth::user::{FirebaseAuthTokens, TEST_USER_UID, User};
use crate::auth::{AuthStateProvider, UserUid};

fn initialize_app(app: &mut App) {
    app.add_singleton_model(|_ctx| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(AuthManager::new_for_test);
}

#[test]
fn validated_api_key_is_promoted_with_its_user() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let auth_state = app.read(|ctx| AuthStateProvider::as_ref(ctx).get().clone());
        auth_state.set_user(None);
        auth_state.set_credentials(None);

        AuthManager::handle(&app).update(&mut app, |auth_manager, ctx| {
            auth_manager.complete_authentication(
                User::test(),
                Credentials::ApiKey {
                    key: "wk-validated-key".to_owned(),
                    owner_type: None,
                },
                ctx,
            );
        });

        assert_eq!(auth_state.api_key().as_deref(), Some("wk-validated-key"));
        assert_eq!(auth_state.user_id(), Some(UserUid::new(TEST_USER_UID)));
        assert!(auth_state.is_logged_in());
    });
}

// These two tests verify that `persist` skips writing to secure storage under certain conditions.
// They rely on the fact that no secure storage singleton is registered in the test app: if
// `write_to_secure_storage` were ever called, it would panic trying to look up the unregistered
// singleton, causing the test to fail.

#[test]
fn test_persist_skips_when_refresh_token_is_empty() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        // Override default test credentials with Firebase tokens that have an empty refresh token.
        app.update(|ctx| {
            let tokens = FirebaseAuthTokens {
                id_token: String::new(),
                refresh_token: String::new(),
                expiration_time: chrono::Utc::now().fixed_offset() + chrono::Duration::days(365),
            };
            AuthStateProvider::as_ref(ctx)
                .get()
                .set_credentials(Some(Credentials::Firebase(tokens)));
        });

        AuthManager::handle(&app).update(&mut app, |auth_manager, ctx| {
            auth_manager.persist(ctx);
        });
    });
}

#[test]
fn test_persist_skips_when_api_key_authenticated() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        app.update(|ctx| {
            AuthStateProvider::as_ref(ctx)
                .get()
                .set_credentials(Some(Credentials::ApiKey {
                    key: "wk-test-key".to_owned(),
                    owner_type: None,
                }));
        });

        AuthManager::handle(&app).update(&mut app, |auth_manager, ctx| {
            auth_manager.persist(ctx);
        });
    });
}
