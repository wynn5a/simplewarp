use std::sync::Arc;

use warpui::App;

use super::*;
use crate::auth::AuthStateProvider;
use crate::server::server_api::ServerApiProvider;
use crate::server::server_api::team::MockTeamClient;
use crate::server::server_api::workspace::MockWorkspaceClient;
use crate::server::telemetry::context_provider::AppTelemetryContextProvider;
use crate::workspaces::workspace::{Workspace, WorkspaceUid};

fn initialize_app(app: &mut App) {
    initialize_app_with_workspaces(app, vec![]);
}

fn initialize_app_with_workspaces(app: &mut App, workspaces: Vec<Workspace>) {
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(AppTelemetryContextProvider::new_context_provider);
    app.add_singleton_model(|ctx| {
        UserWorkspaces::mock(
            Arc::new(MockTeamClient::new()),
            Arc::new(MockWorkspaceClient::new()),
            workspaces,
            ctx,
        )
    });
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
    app.add_singleton_model(|_| crate::pricing::PricingInfoModel::new());
    app.add_singleton_model(|ctx| {
        AIRequestUsageModel::new_for_test(ServerApiProvider::as_ref(ctx).get_ai_client(), ctx)
    });
}

fn determine_state(app: &mut App) -> PromptAlertState {
    app.read(PromptAlertView::determine_state)
}

/// The point of the fork: nothing about the account, the plan, or a request
/// quota can raise an alert, because SimpleWarp does not meter requests. These
/// tests replace the server-availability mapping tests that this state machine
/// used to need.
#[test]
fn no_alert_without_a_workspace() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        assert_eq!(determine_state(&mut app), PromptAlertState::NoAlert);
    });
}

#[test]
fn no_alert_even_with_a_workspace_that_would_once_have_gated_ai() {
    App::test((), |mut app| async move {
        // Before the quota went, a workspace carrying no credit allowance and no
        // overage policy produced `RequestLimitReached`. It must not now.
        let uid = WorkspaceUid::from(crate::server::ids::ServerId::from(1_i64));
        let workspace = Workspace::from_local_cache(uid, "Test Workspace".to_string(), None);
        initialize_app_with_workspaces(&mut app, vec![workspace]);

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
