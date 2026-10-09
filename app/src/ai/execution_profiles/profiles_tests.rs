use settings::Setting as _;
use warpui::{App, SingletonEntity};

use crate::LaunchMode;
use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::ai::execution_profiles::{
    AIExecutionProfile, ActionPermission, ExecutionProfileId, ExecutionProfilesConfig,
};
use crate::ai::mcp::TemplatableMCPServerManager;
use crate::settings::AISettings;
use crate::test_util::settings::initialize_settings_for_tests;

/// Install the minimal singleton graph needed to construct an `AIExecutionProfilesModel`.
fn install_singletons(app: &mut App) {
    initialize_settings_for_tests(app);
    app.add_singleton_model(|_| TemplatableMCPServerManager::default());
}

#[test]
fn gui_default_execute_commands_remains_always_ask() {
    App::test((), |mut app| async move {
        install_singletons(&mut app);
        let profile_model = app.add_singleton_model(|ctx| {
            AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx)
        });

        profile_model.read(&app, |model, ctx| {
            assert_eq!(
                model.default_profile(ctx).data().execute_commands,
                ActionPermission::AlwaysAsk,
                "the GUI default must remain conservative"
            );
        });
    })
}

#[test]
fn first_edit_materializes_the_pending_collection() {
    App::test((), |mut app| async move {
        install_singletons(&mut app);
        let profile_model = app.add_singleton_model(|ctx| {
            AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx)
        });
        let default_profile_id = profile_model.read(&app, |model, _| model.default_profile_id());

        // A fresh default profile starts with the enum default (`AgentDecides`).
        profile_model.read(&app, |model, ctx| {
            assert_eq!(
                model.default_profile(ctx).data().apply_code_diffs,
                ActionPermission::AgentDecides
            );
        });

        profile_model.update(&mut app, |model, ctx| {
            model.set_apply_code_diffs(&default_profile_id, &ActionPermission::AlwaysAllow, ctx);
        });

        profile_model.read(&app, |model, ctx| {
            assert_eq!(
                model.default_profile(ctx).data().apply_code_diffs,
                ActionPermission::AlwaysAllow
            );
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
fn settings_collection_backs_create_edit_and_delete() {
    App::test((), |mut app| async move {
        install_singletons(&mut app);
        app.update(|ctx| {
            let mut profiles = ExecutionProfilesConfig::default();
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

        let settings_model = app
            .add_model(|ctx| AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx));
        settings_model.read(&app, |model, ctx| {
            assert_eq!(model.default_profile(ctx).data().name, "Settings default");
            assert!(!model.has_multiple_profiles());
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
        settings_model.read(&app, |model, _| {
            assert!(model.has_multiple_profiles());
        });
        settings_model.update(&mut app, |model, ctx| {
            model.delete_profile(&created_profile_id, ctx);
        });
        app.read(|ctx| {
            assert!(
                AISettings::as_ref(ctx)
                    .execution_profiles
                    .value()
                    .profile(&created_profile_id)
                    .is_none()
            );
        });

        let restored_model = app
            .add_model(|ctx| AIExecutionProfilesModel::new(&LaunchMode::new_for_unit_test(), ctx));
        restored_model.read(&app, |model, ctx| {
            assert_eq!(model.default_profile(ctx).data().name, "Settings default");
        });
    });
}

#[test]
fn cli_uses_its_own_default_profile() {
    App::test((), |mut app| async move {
        install_singletons(&mut app);
        let cli_model = app.add_model(|ctx| AIExecutionProfilesModel::new(&cli_launch_mode(), ctx));
        cli_model.read(&app, |model, ctx| {
            assert_ne!(
                model.default_profile_id(),
                ExecutionProfileId::default_profile()
            );
            assert_eq!(
                model.default_profile(ctx).data().execute_commands,
                ActionPermission::AlwaysAllow
            );
        });
        assert!(
            cli_model
                .update(&mut app, |model, ctx| model.create_profile(ctx))
                .is_none()
        );
    });
}

fn cli_launch_mode() -> LaunchMode {
    LaunchMode::CommandLine {
        command: warp_cli::CliCommand::Model(warp_cli::model::ModelCommand::List),
        global_options: warp_cli::GlobalOptions::default(),
        debug: false,
        is_sandboxed: true,
    }
}

#[test]
fn cli_can_select_a_stored_local_profile() {
    App::test((), |mut app| async move {
        install_singletons(&mut app);
        let reviewer_id = ExecutionProfileId::parse("profile-reviewer").unwrap();
        app.update(|ctx| {
            let mut profiles = ExecutionProfilesConfig::default();
            profiles.insert(
                reviewer_id.clone(),
                AIExecutionProfile {
                    name: "Reviewer".to_string(),
                    execute_commands: ActionPermission::AlwaysAsk,
                    ..Default::default()
                },
            );
            AISettings::handle(ctx).update(ctx, |settings, ctx| {
                settings
                    .execution_profiles
                    .set_value(profiles, ctx)
                    .unwrap();
            });
        });

        let cli_model = app.add_model(|ctx| AIExecutionProfilesModel::new(&cli_launch_mode(), ctx));
        let terminal_view_id = warpui::EntityId::new();
        cli_model.update(&mut app, |model, ctx| {
            let local_profiles = model.local_profiles(ctx);
            assert_eq!(
                local_profiles.profile_ids().cloned().collect::<Vec<_>>(),
                vec![ExecutionProfileId::default_profile(), reviewer_id.clone()]
            );
            let profile_id = local_profiles.resolve("reviewer").unwrap();
            model.set_active_profile(terminal_view_id, profile_id, ctx);
        });
        cli_model.read(&app, |model, ctx| {
            let active = model.active_profile(Some(terminal_view_id), ctx);
            assert_eq!(active.id(), &reviewer_id);
            assert_eq!(active.data().name, "Reviewer");
            // Other terminals keep the CLI default.
            assert_eq!(
                model.active_profile(None, ctx).data().execute_commands,
                ActionPermission::AlwaysAllow
            );
        });
    });
}

#[test]
fn cli_lists_the_implicit_default_before_any_profile_is_stored() {
    App::test((), |mut app| async move {
        install_singletons(&mut app);
        let cli_model = app.add_model(|ctx| AIExecutionProfilesModel::new(&cli_launch_mode(), ctx));
        cli_model.read(&app, |model, ctx| {
            let local_profiles = model.local_profiles(ctx);
            assert_eq!(
                local_profiles.profile_ids().cloned().collect::<Vec<_>>(),
                vec![ExecutionProfileId::default_profile()]
            );
            assert_eq!(
                local_profiles.resolve("default"),
                Ok(ExecutionProfileId::default_profile())
            );
        });
    });
}
