use std::sync::Arc;

use ai::api_keys::ApiKeyManager;
use chrono::Duration;
use warp_core::telemetry::testing::MockTelemetryContextProvider;
use warpui::{App, ModelHandle};

use super::*;
use crate::auth::AuthStateProvider;
use crate::pricing::PricingInfoModel;
use crate::server::server_api::ServerApiProvider;
use crate::server::server_api::team::MockTeamClient;
use crate::server::server_api::workspace::MockWorkspaceClient;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::workspaces::workspace::{Workspace, WorkspaceUid};

fn create_test_workspace() -> (WorkspaceUid, Workspace) {
    let server_id: crate::server::ids::ServerId = 1_i64.into();
    let uid = WorkspaceUid::from(server_id);
    let workspace = Workspace::from_local_cache(uid, "Test Workspace".to_string(), None);
    (uid, workspace)
}

fn add_user_workspaces_with_workspace(app: &mut App, workspace: Workspace) {
    app.add_singleton_model(|ctx| {
        UserWorkspaces::mock(
            Arc::new(MockTeamClient::new()),
            Arc::new(MockWorkspaceClient::new()),
            vec![workspace],
            ctx,
        )
    });
}

fn add_request_usage_model(app: &mut App) -> ModelHandle<AIRequestUsageModel> {
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    add_request_usage_model_without_auth(app)
}

fn add_request_usage_model_for_logged_out_users(app: &mut App) -> ModelHandle<AIRequestUsageModel> {
    app.add_singleton_model(|_| AuthStateProvider::new_logged_out_for_test());
    add_request_usage_model_without_auth(app)
}
fn register_user_preferences_for_tests(app: &mut App) {
    if app
        .models_of_type::<settings::PrivatePreferences>()
        .is_empty()
    {
        app.update(crate::settings::init_and_register_user_preferences);
    }
}

fn add_request_usage_model_without_auth(app: &mut App) -> ModelHandle<AIRequestUsageModel> {
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    register_user_preferences_for_tests(app);
    app.update(|ctx| {
        warpui_extras::secure_storage::register_noop("test", ctx);
        MockTelemetryContextProvider::register(ctx);
        ctx.add_singleton_model(ApiKeyManager::new);
    });
    app.add_singleton_model(|_| PricingInfoModel::new());
    app.add_singleton_model(|ctx| {
        AIRequestUsageModel::new_for_test(ServerApiProvider::as_ref(ctx).get_ai_client(), ctx)
    })
}

#[test]
fn refresh_request_usage_returns_no_fresh_limit_when_logged_out() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model_for_logged_out_users(&mut app);
        let refresh =
            request_usage_model.update(&mut app, |model, ctx| model.refresh_request_usage(ctx));

        assert_eq!(refresh.await.unwrap(), None);
    });
}
#[test]
fn test_request_limit_info() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);
        request_usage_model.update(&mut app, |request_usage_model, _ctx| {
            request_usage_model.request_limit_info = RequestLimitInfo {
                limit: 200,
                num_requests_used_since_refresh: 39,
                next_refresh_time: ServerTimestamp::new(Utc::now() + Duration::days(1)),
                is_unlimited: false,
                request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
                is_unlimited_voice: false,
                voice_request_limit: 100,
                voice_requests_used_since_last_refresh: 0,
                is_unlimited_codebase_indices: false,
                max_codebase_indices: 3,
                max_files_per_repo: 5000,
                embedding_generation_batch_size: 100,
            };
            assert_eq!(200, request_usage_model.request_limit());
            assert_eq!(39, request_usage_model.requests_used());
            assert_eq!(161, request_usage_model.requests_remaining());
        })
    });
}

#[test]
fn test_request_limit_info_with_limit() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);
        request_usage_model.update(&mut app, |request_usage_model, _ctx| {
            request_usage_model.request_limit_info = RequestLimitInfo {
                limit: 999999999,
                num_requests_used_since_refresh: 39,
                next_refresh_time: ServerTimestamp::new(Utc::now() + Duration::minutes(1)),
                is_unlimited: false,
                request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
                is_unlimited_voice: false,
                voice_request_limit: 100,
                voice_requests_used_since_last_refresh: 0,
                is_unlimited_codebase_indices: false,
                max_codebase_indices: 3,
                max_files_per_repo: 5000,
                embedding_generation_batch_size: 100,
            };
            assert_eq!(999999999, request_usage_model.request_limit());
            assert_eq!(39, request_usage_model.requests_used());
            assert_eq!(999999960, request_usage_model.requests_remaining());
        })
    });
}

#[test]
fn test_request_limit_info_past_refresh_time() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);
        request_usage_model.update(&mut app, |request_usage_model, _ctx| {
            request_usage_model.request_limit_info = RequestLimitInfo {
                limit: 200,
                num_requests_used_since_refresh: 39,
                next_refresh_time: ServerTimestamp::new(Utc::now() - Duration::seconds(1)),
                is_unlimited: false,
                request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
                is_unlimited_voice: false,
                voice_request_limit: 100,
                voice_requests_used_since_last_refresh: 0,
                is_unlimited_codebase_indices: false,
                max_codebase_indices: 3,
                max_files_per_repo: 5000,
                embedding_generation_batch_size: 100,
            };
            assert_eq!(200, request_usage_model.request_limit());
            assert_eq!(0, request_usage_model.requests_used());
            assert_eq!(200, request_usage_model.requests_remaining());
        })
    });
}

#[test]
fn test_request_limit_info_is_unlimited_true() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);
        request_usage_model.update(&mut app, |request_usage_model, _ctx| {
            request_usage_model.request_limit_info = RequestLimitInfo {
                limit: 999999999,
                num_requests_used_since_refresh: 39,
                next_refresh_time: ServerTimestamp::new(Utc::now() + Duration::minutes(1)),
                is_unlimited: true,
                request_limit_refresh_duration: RequestLimitRefreshDuration::Monthly,
                is_unlimited_voice: false,
                voice_request_limit: 100,
                voice_requests_used_since_last_refresh: 0,
                is_unlimited_codebase_indices: false,
                max_codebase_indices: 3,
                max_files_per_repo: 5000,
                embedding_generation_batch_size: 100,
            };
            assert_eq!(999999999, request_usage_model.request_limit());
            assert_eq!(39, request_usage_model.requests_used());
            assert_eq!(999999999, request_usage_model.requests_remaining());
        })
    });
}

#[test]
fn test_ambient_credits_banner_dismissal_is_persisted() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);

        request_usage_model.update(&mut app, |model, ctx| {
            assert!(!model.is_ambient_credits_banner_dismissed());
            model.dismiss_ambient_credits_banner(ctx);
            assert!(model.is_ambient_credits_banner_dismissed());
        });

        app.update(|ctx| {
            let stored_value = ctx
                .private_user_preferences()
                .read_value(AMBIENT_CREDITS_BANNER_DISMISSED_KEY)
                .unwrap();
            assert_eq!(stored_value, Some("true".to_owned()));
        });
    });
}

#[test]
fn test_ambient_credits_banner_dismissal_loads_from_preferences() {
    App::test((), |mut app| async move {
        register_user_preferences_for_tests(&mut app);
        app.update(|ctx| {
            ctx.private_user_preferences()
                .write_value(AMBIENT_CREDITS_BANNER_DISMISSED_KEY, "true".to_owned())
                .unwrap();
        });

        let request_usage_model = add_request_usage_model(&mut app);

        request_usage_model.update(&mut app, |model, _ctx| {
            assert!(model.is_ambient_credits_banner_dismissed());
        });
    });
}
#[test]
fn test_total_workspace_and_team_bonus_credits_counts_both_scopes() {
    App::test((), |mut app| async move {
        let (uid, workspace) = create_test_workspace();
        let other_uid = WorkspaceUid::from(crate::server::ids::ServerId::from(2_i64));
        add_user_workspaces_with_workspace(&mut app, workspace);
        let request_usage_model = add_request_usage_model(&mut app);

        request_usage_model.update(&mut app, |model, _ctx| {
            let make = |scope, remaining| BonusGrant {
                created_at: Utc::now(),
                cost_cents: 0,
                expiration: None,
                grant_type: BonusGrantType::Any,
                reason: "test".to_string(),
                user_facing_message: None,
                request_credits_granted: remaining,
                request_credits_remaining: remaining,
                scope,
            };
            model.bonus_grants = vec![
                make(BonusGrantScope::User, 5),
                make(BonusGrantScope::Team(uid), 7),
                make(BonusGrantScope::Workspace(uid), 11),
                make(BonusGrantScope::Team(other_uid), 13),
            ];

            assert_eq!(
                model.total_workspace_and_team_bonus_credits_remaining(uid),
                18
            );
        });
    });
}

/// The 30 tests this replaces each described a way to *earn* the right to make
/// an AI request — base quota, bonus grants, overages, pay-as-you-go, auto
/// reload, a BYO key. SimpleWarp does not meter requests, so there is nothing
/// to earn and nothing to deny.
#[test]
fn has_any_ai_remaining_is_true_with_no_workspace_no_credits_and_no_key() {
    App::test((), |mut app| async move {
        let request_usage_model = add_request_usage_model(&mut app);

        request_usage_model.update(&mut app, |model, ctx| {
            model.request_limit_info = RequestLimitInfo::new_for_test(0, 0);
            model.bonus_grants.clear();

            assert!(
                model.has_any_ai_remaining(ctx),
                "SimpleWarp allows every AI request; nothing is counted against a quota",
            );
        });
    });
}
