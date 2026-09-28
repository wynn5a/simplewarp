use settings::Setting as _;
use warpui::{App, SingletonEntity};

use crate::LaunchMode;
use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::ai::execution_profiles::{
    ActionPermission, ExecutionProfileId, ExecutionProfilesConfig,
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
        let cli_model = app.add_model(|ctx| {
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
        });
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
