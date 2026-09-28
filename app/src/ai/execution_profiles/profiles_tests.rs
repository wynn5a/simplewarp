use settings::Setting as _;
use warp_core::features::FeatureFlag;
use warpui::{App, SingletonEntity};

use crate::LaunchMode;
use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::ai::execution_profiles::{
    AIExecutionProfile, ActionPermission, CloudAIExecutionProfile, CloudAIExecutionProfileModel,
    ExecutionProfileId,
};
use crate::ai::llms::LLMId;
use crate::ai::mcp::TemplatableMCPServerManager;
use crate::auth::AuthStateProvider;
use crate::cloud_object::model::actions::ObjectActions;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{
    CloudObjectMetadata, CloudObjectPermissions, CloudObjectStatuses, CloudObjectSyncStatus, Owner,
    Revision,
};
use crate::network::NetworkStatus;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::ids::{ServerId, SyncId};
use crate::settings::{AISettings, PrivacySettings};
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workspaces::user_profiles::UserProfiles;
use crate::workspaces::user_workspaces::UserWorkspaces;

fn mock_cloud_metadata() -> CloudObjectMetadata {
    CloudObjectMetadata {
        pending_changes_statuses: CloudObjectStatuses {
            content_sync_status: CloudObjectSyncStatus::NoLocalChanges,
            has_pending_metadata_change: false,
            has_pending_permissions_change: false,
            pending_untrash: false,
            pending_delete: false,
        },
        folder_id: None,
        revision: Some(Revision::now()),
        metadata_last_updated_ts: None,
        current_editor_uid: None,
        trashed_ts: None,
        is_welcome_object: false,
        creator_uid: None,
        last_editor_uid: None,
        last_task_run_ts: None,
    }
}

fn mock_cloud_permissions() -> CloudObjectPermissions {
    CloudObjectPermissions {
        owner: Owner::mock_current_user(),
        guests: Vec::new(),
        permissions_last_updated_ts: None,
        anyone_with_link: None,
    }
}

fn owned_legacy_profile(sync_id: SyncId, profile: AIExecutionProfile) -> CloudAIExecutionProfile {
    CloudAIExecutionProfile::new(
        sync_id,
        CloudAIExecutionProfileModel::new(profile),
        mock_cloud_metadata(),
        mock_cloud_permissions(),
    )
}

/// Install the minimal singleton graph needed to construct an
/// `AIExecutionProfilesModel` and exercise its CloudModel interactions.
fn install_singletons(app: &mut App, auth_state: AuthStateProvider) {
    initialize_settings_for_tests(app);
    app.add_singleton_model(|_| auth_state);
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| UpdateManager::mock());
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| ObjectActions::new(Vec::new()));
    app.add_singleton_model(|_| TemplatableMCPServerManager::default());
    app.add_singleton_model(PrivacySettings::mock);
    app.add_singleton_model(|_| UserProfiles::new(Vec::new()));
    app.add_singleton_model(UserWorkspaces::default_mock);
}

#[test]
fn gui_default_execute_commands_remains_always_ask() {
    let _guard = FeatureFlag::FileBackedExecutionProfiles.override_enabled(false);

    App::test((), |mut app| async move {
        install_singletons(&mut app, AuthStateProvider::new_for_test());
        let profile_model = app.add_singleton_model(|ctx| {
            AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx)
        });

        profile_model.read(&app, |model, ctx| {
            assert_eq!(
                model.default_profile(ctx).data().execute_commands,
                ActionPermission::AlwaysAsk,
                "the GUI/legacy default must remain conservative"
            );
        });
    })
}

/// Regression test for the onboarding autonomy bug where
/// `edit_profile_internal` would silently drop edits made to an `Unsynced`
/// default profile whenever `personal_drive` returned `None` (logged-out
/// users). `apply_agent_settings` calls `set_*` on the default profile the
/// moment onboarding completes, which can happen before the user logs in
/// (e.g. `LoginSlideEvent::LoginLaterConfirmed`), so those edits must
/// persist on the local `Unsynced` state rather than being dropped.
#[test]
fn edits_persist_on_unsynced_default_profile_when_logged_out() {
    App::test((), |mut app| async move {
        install_singletons(&mut app, AuthStateProvider::new_logged_out_for_test());
        let profile_model = app.add_singleton_model(|ctx| {
            AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx)
        });

        let default_profile_id = profile_model.read(&app, |model, _ctx| model.default_profile_id());

        // Sanity-check the precondition: the baseline `apply_code_diffs`
        // on a fresh default profile is the enum default (`AgentDecides`).
        profile_model.read(&app, |model, ctx| {
            assert!(
                matches!(
                    model.default_profile(ctx).data().apply_code_diffs,
                    ActionPermission::AgentDecides
                ),
                "unexpected baseline apply_code_diffs"
            );
        });

        // Apply the edit that onboarding would make for the Full autonomy
        // preset. Before the fix, this call no-ops because
        // `personal_drive` is `None` while the profile is `Unsynced` — the
        // `set_apply_code_diffs` value was cloned, mutated, then dropped
        // without being written back to `default_profile_state`.
        profile_model.update(&mut app, |model, ctx| {
            model.set_apply_code_diffs(&default_profile_id, &ActionPermission::AlwaysAllow, ctx);
        });

        profile_model.read(&app, |model, ctx| {
            assert_eq!(
                model.default_profile(ctx).data().apply_code_diffs,
                ActionPermission::AlwaysAllow,
                "edit was dropped: default profile still has the baseline \
                 apply_code_diffs value after an edit made while logged out",
            );
        });
    })
}

#[test]
fn feature_disabled_keeps_legacy_backend_behavior() {
    let _guard = FeatureFlag::FileBackedExecutionProfiles.override_enabled(false);

    App::test((), |mut app| async move {
        install_singletons(&mut app, AuthStateProvider::new_for_test());
        let server_id = ServerId::from(511);
        let legacy_model = LLMId::from("gpt-5-6-sol-high");
        let legacy_default = owned_legacy_profile(
            SyncId::ServerId(server_id),
            AIExecutionProfile {
                name: "Legacy default".to_string(),
                is_default_profile: true,
                base_model: Some(legacy_model.clone()),
                ..Default::default()
            },
        );
        CloudModel::handle(&app).update(&mut app, |cloud_model, _| {
            cloud_model.add_object(legacy_default.id, legacy_default);
        });

        let profile_model = app.add_singleton_model(|ctx| {
            AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx)
        });
        profile_model.read(&app, |model, ctx| {
            let default_profile = model.default_profile(ctx);
            assert_ne!(default_profile.id(), &ExecutionProfileId::default_profile());
            assert_eq!(default_profile.data().base_model, Some(legacy_model));
            assert_eq!(default_profile.sync_id(), Some(SyncId::ServerId(server_id)));
            assert_eq!(
                model.get_profile_id_by_sync_id(&SyncId::ServerId(server_id), ctx),
                Some(default_profile.id().clone())
            );
        });
        app.read(|ctx| {
            assert!(
                !AISettings::as_ref(ctx)
                    .execution_profiles
                    .is_value_explicitly_set()
            );
        });
    });
}

#[test]
fn pre_login_edit_materializes_the_pending_collection() {
    let _guard = FeatureFlag::FileBackedExecutionProfiles.override_enabled(true);

    App::test((), |mut app| async move {
        install_singletons(&mut app, AuthStateProvider::new_logged_out_for_test());
        let profile_model = app.add_singleton_model(|ctx| {
            AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx)
        });
        let default_profile_id = profile_model.read(&app, |model, _| model.default_profile_id());

        profile_model.update(&mut app, |model, ctx| {
            model.set_apply_code_diffs(&default_profile_id, &ActionPermission::AlwaysAllow, ctx);
        });

        app.read(|ctx| {
            let settings = AISettings::as_ref(ctx);
            assert!(settings.execution_profiles.is_value_explicitly_set());
            assert_eq!(
                settings
                    .execution_profiles
                    .value()
                    .profile(&ExecutionProfileId::default_profile())
                    .map(|profile| profile.apply_code_diffs),
                Some(ActionPermission::AlwaysAllow)
            );
        });
        let restored_model = app
            .add_model(|ctx| AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx));
        restored_model.read(&app, |model, ctx| {
            assert_eq!(
                model.default_profile(ctx).data().apply_code_diffs,
                ActionPermission::AlwaysAllow
            );
        });
    });
}

#[test]
fn profile_sources_preserve_state_across_migration_and_rollout() {
    App::test((), |mut app| async move {
        install_singletons(&mut app, AuthStateProvider::new_for_test());
        app.update(|ctx| {
            let mut profiles = crate::ai::execution_profiles::ExecutionProfilesConfig::default();
            profiles
                .profile_mut(&ExecutionProfileId::default_profile())
                .unwrap()
                .name = "Settings default".to_string();
            AISettings::handle(ctx).update(ctx, |settings, ctx| {
                settings
                    .execution_profiles
                    .set_value(profiles, ctx)
                    .unwrap();
            });
        });

        let server_id = ServerId::from(506);
        let legacy_default = owned_legacy_profile(
            SyncId::ServerId(server_id),
            AIExecutionProfile {
                name: "Legacy default".to_string(),
                is_default_profile: true,
                ..Default::default()
            },
        );
        CloudModel::handle(&app).update(&mut app, |cloud_model, _| {
            cloud_model.add_object(legacy_default.id, legacy_default);
        });

        let settings_model = {
            let _guard = FeatureFlag::FileBackedExecutionProfiles.override_enabled(true);
            app.add_model(|ctx| {
                AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx)
            })
        };
        settings_model.read(&app, |model, ctx| {
            assert_eq!(model.default_profile(ctx).data().name, "Settings default");
        });
        let created_profile_id = settings_model
            .update(&mut app, |model, ctx| model.create_profile(ctx))
            .unwrap();
        settings_model.update(&mut app, |model, ctx| {
            model.set_profile_name(&created_profile_id, "Edited", ctx);
        });
        app.read(|ctx| {
            assert_eq!(
                AISettings::as_ref(ctx)
                    .execution_profiles
                    .value()
                    .profile(&created_profile_id)
                    .map(|profile| profile.name.as_str()),
                Some("Edited")
            );
        });
        settings_model.update(&mut app, |model, ctx| {
            model.delete_profile(&created_profile_id, ctx);
        });

        let legacy_model = {
            let _guard = FeatureFlag::FileBackedExecutionProfiles.override_enabled(false);
            app.add_model(|ctx| {
                AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx)
            })
        };
        legacy_model.read(&app, |model, ctx| {
            assert_eq!(model.default_profile(ctx).data().name, "Legacy default");
        });

        let restored_settings_model = {
            let _guard = FeatureFlag::FileBackedExecutionProfiles.override_enabled(true);
            app.add_model(|ctx| {
                AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx)
            })
        };
        restored_settings_model.read(&app, |model, ctx| {
            assert_eq!(model.default_profile(ctx).data().name, "Settings default");
        });
        restored_settings_model.update(&mut app, |model, _| model.reset(true));
        restored_settings_model.read(&app, |model, ctx| {
            assert_eq!(model.default_profile(ctx).data().name, "Settings default");
        });

        let cli_model = {
            let _guard = FeatureFlag::FileBackedExecutionProfiles.override_enabled(true);
            app.add_model(|ctx| {
                AIExecutionProfilesModel::new(
                    &LaunchMode::CommandLine {
                        command: warp_cli::CliCommand::Model(warp_cli::model::ModelCommand::List),
                        global_options: warp_cli::GlobalOptions::default(),
                        debug: false,
                        is_sandboxed: true,
                        computer_use_override: None,
                    },
                    ctx,
                )
            })
        };
        cli_model.read(&app, |model, ctx| {
            assert_ne!(model.default_profile(ctx).data().name, "Settings default");
            assert!(model.default_profile(ctx).sync_id().is_none());
        });
    });
}

/// Regression test for the "log in to an existing user after onboarding"
/// bug. Cloud objects arriving via the initial bulk load are inserted into
/// `CloudModel` *without* firing per-object `ObjectCreated` events, so the
/// bulk-load reconciliation must adopt them afterward. Without it, the
/// existing user's default profile sits in `CloudModel` but
/// `AIExecutionProfilesModel` stays in `Unsynced`, so a subsequent
/// onboarding edit creates a duplicate cloud default profile instead of
/// editing the existing one. This test drives that sequence and asserts
/// the model adopts the cloud profile's sync id.
#[test]
fn reconciles_unsynced_default_profile_with_cloud_after_initial_load() {
    App::test((), |mut app| async move {
        install_singletons(&mut app, AuthStateProvider::new_for_test());
        let profile_model = app.add_singleton_model(|ctx| {
            AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx)
        });

        // Baseline: CloudModel is empty, so the model starts Unsynced and
        // `sync_id` is `None`.
        profile_model.read(&app, |model, ctx| {
            assert!(
                model.default_profile(ctx).sync_id().is_none(),
                "default profile should be Unsynced at startup"
            );
        });

        // Simulate the user's existing cloud default profile arriving via
        // initial bulk load. We construct the existing profile with
        // `apply_code_diffs = AlwaysAllow` so we can verify the model is
        // reading that cloud object after reconciliation.
        let cloud_uid = ServerId::from(42);
        let cloud_sync_id = SyncId::ServerId(cloud_uid);
        let cloud_profile = AIExecutionProfile {
            name: "Default".to_string(),
            is_default_profile: true,
            apply_code_diffs: ActionPermission::AlwaysAllow,
            ..Default::default()
        };
        let existing_profile = CloudAIExecutionProfile::new(
            cloud_sync_id,
            CloudAIExecutionProfileModel::new(cloud_profile),
            mock_cloud_metadata(),
            mock_cloud_permissions(),
        );

        // Insert the object into CloudModel without per-object events and then
        // run the bulk-load reconciliation by hand.
        CloudModel::handle(&app).update(&mut app, move |cloud_model, _| {
            cloud_model.add_object(existing_profile.id, existing_profile);
        });
        profile_model.update(&mut app, |model, ctx| {
            model.reconcile_with_cloud_state_after_initial_load(ctx);
        });

        // The model should now be Synced with the cloud profile's sync_id,
        // and `default_profile` should read values from the existing cloud
        // object (proving we're not backed by a fresh client-side default).
        profile_model.read(&app, |model, ctx| {
            let info = model.default_profile(ctx);
            assert_eq!(
                info.sync_id(),
                Some(cloud_sync_id),
                "model did not adopt the existing cloud default profile's sync_id"
            );
            assert_eq!(
                info.data().apply_code_diffs,
                ActionPermission::AlwaysAllow,
                "default profile should now surface the existing cloud value"
            );
        });

        // Further edits should now target the existing cloud profile in
        // place, rather than falling through the `Unsynced` branch and
        // creating a duplicate.
        let default_profile_id = profile_model.read(&app, |model, _ctx| model.default_profile_id());
        profile_model.update(&mut app, |model, ctx| {
            model.set_apply_code_diffs(&default_profile_id, &ActionPermission::AlwaysAsk, ctx);
        });
        profile_model.read(&app, |model, ctx| {
            let info = model.default_profile(ctx);
            assert_eq!(
                info.sync_id(),
                Some(cloud_sync_id),
                "edit should target the same cloud sync_id, not create a duplicate"
            );
            assert_eq!(
                info.data().apply_code_diffs,
                ActionPermission::AlwaysAsk,
                "edit should be reflected on the existing cloud profile"
            );
        });
    })
}
