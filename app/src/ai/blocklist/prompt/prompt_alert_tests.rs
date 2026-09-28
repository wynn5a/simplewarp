use warpui::App;

use super::*;
use crate::server::server_api::ServerApiProvider;

fn initialize_app(app: &mut App) {
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    if app
        .models_of_type::<settings::PrivatePreferences>()
        .is_empty()
    {
        app.update(crate::settings::init_and_register_user_preferences);
    }
    app.update(|ctx| {
        warpui_extras::secure_storage::register_noop("test", ctx);
        ctx.add_singleton_model(ApiKeyManager::new);
    });
    app.add_singleton_model(|_| AIRequestUsageModel::new());
}

fn determine_state(app: &mut App) -> PromptAlertState {
    app.read(PromptAlertView::determine_state)
}

/// The point of the fork: nothing about the account, the plan, or a request
/// quota can raise an alert, because SimpleWarp does not meter requests. These
/// tests replace the server-availability mapping tests that this state machine
/// used to need.
#[test]
fn no_alert_when_online() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        assert_eq!(determine_state(&mut app), PromptAlertState::NoAlert);
    });
}

#[test]
fn offline_is_the_only_state_that_blocks_a_request() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        NetworkStatus::handle(&app).update(&mut app, |status, ctx| {
            status.reachability_changed(false, ctx);
        });

        assert_eq!(determine_state(&mut app), PromptAlertState::NoConnection);
        assert!(app.read(PromptAlertView::does_alert_block_ai_requests));
    });
}
