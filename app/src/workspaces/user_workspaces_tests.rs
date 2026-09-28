use settings::{PrivatePreferences, PublicPreferences};
use warpui::{AddSingletonModel, App, WindowId};
use warpui_extras::user_preferences;

use super::*;
use crate::auth::auth_manager::AuthManager;
use crate::cloud_object::model::persistence::CloudModel;
use crate::features::FeatureFlag;
use crate::network::NetworkStatus;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::server_api::ServerApiProvider;
use crate::settings::{AISettings, CodeSettings, PrivacySettings};
use crate::system::SystemStats;
use crate::workspaces::team::Team;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::workspaces::workspace::Workspace;

#[derive(Default)]
struct CachedResources {
    workspaces: Vec<Workspace>,
}

fn initialize_app(app: &mut App, resources: CachedResources) {
    initialize_app_with_auth(app, resources, AuthStateProvider::new_for_test());
}

fn initialize_app_with_auth(
    app: &mut App,
    resources: CachedResources,
    auth_state_provider: AuthStateProvider,
) {
    // Add the necessary singleton models to the App
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| SystemStats::new());
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|ctx| UserWorkspaces::mock(resources.workspaces, ctx));
    app.add_singleton_model(|_| UpdateManager::mock());
    app.add_singleton_model(PrivacySettings::mock);
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| auth_state_provider);
    app.add_singleton_model(AuthManager::new_for_test);
    app.add_singleton_model(|_| {
        PublicPreferences::new(Box::<user_preferences::in_memory::InMemoryPreferences>::default())
    });
    app.add_singleton_model(|_| {
        PrivatePreferences::new(Box::<user_preferences::in_memory::InMemoryPreferences>::default())
    });

    app.add_singleton_model(CodeSettings::new_with_defaults);
    app.add_singleton_model(AISettings::new_with_defaults);
}

fn initialize_window_team_test_app(app: &mut App, workspaces: Vec<Workspace>) {
    app.add_singleton_model(PrivacySettings::mock);
    app.add_singleton_model(|ctx| UserWorkspaces::mock(workspaces, ctx));
}

#[test]
fn test_loading_all_spaces_after_switching_from_offline() {
    let _flag = FeatureFlag::KnowledgeSidebar.override_enabled(true);

    let workspace = workspace_for_test(&team_for_test());

    App::test((), |mut app| async move {
        // With no workspaces cached, UserWorkspaces stores no teams.
        initialize_app(&mut app, CachedResources { workspaces: vec![] });

        UserWorkspaces::handle(&app).read(&app, |teams, _| {
            assert!(!teams.has_teams());
        });

        // Load the team into the (previously server-fed) workspace list, then
        // select it — the production fetch flow sets the current workspace
        // after the list arrives.
        UserWorkspaces::handle(&app).update(&mut app, |teams, ctx| {
            teams.update_workspaces(vec![workspace.clone()], ctx);
            teams.set_current_workspace_uid(workspace.uid, ctx);
        });

        // We also ensure that UserWorkspaces stores a team
        UserWorkspaces::handle(&app).read(&app, |teams, _| {
            assert!(teams.has_teams());
        });
    })
}

#[test]
fn test_codebase_context_enabled_with_no_workspace() {
    App::test((), |mut app| async move {
        initialize_app(&mut app, CachedResources { workspaces: vec![] });

        app.read(|ctx| {
            let codebase_context_enabled =
                UserWorkspaces::as_ref(ctx).is_codebase_context_enabled(ctx);
            assert!(
                codebase_context_enabled,
                "codebase context should be on by default"
            );
        });
    })
}

fn team_for_test() -> Team {
    Team {
        uid: 123.into(),
        name: "test".to_string(),
        color: None,
        members: vec![],
        billing_metadata: Default::default(),
    }
}

fn workspace_for_test(team: &Team) -> Workspace {
    Workspace {
        uid: "workspace_uid123456789".to_string().into(),
        teams: vec![team.clone()],
        billing_metadata: team.billing_metadata.clone(),
    }
}

#[test]
fn test_window_team_assignment_reconciles_when_current_workspace_changes() {
    let first_team = team_for_test();
    let first_workspace = workspace_for_test(&first_team);
    let mut second_team = team_for_test();
    second_team.uid = 456.into();
    let mut second_workspace = workspace_for_test(&second_team);
    second_workspace.uid = "workspace_uid987654321".to_string().into();
    let second_workspace_uid = second_workspace.uid;

    App::test((), |mut app| async move {
        initialize_window_team_test_app(&mut app, vec![first_workspace, second_workspace]);

        let window_id = WindowId::new();
        UserWorkspaces::handle(&app).update(&mut app, |user_workspaces, ctx| {
            user_workspaces.register_window(window_id, None, ctx);
            user_workspaces.set_current_workspace_uid(second_workspace_uid, ctx);
        });

        app.read(|ctx| {
            assert_eq!(
                UserWorkspaces::as_ref(ctx).team_uid_for_window(window_id),
                Some(second_team.uid)
            );
        });
    })
}

#[test]
fn test_unassigned_window_is_initialized_after_workspace_metadata_loads() {
    let team = team_for_test();
    let workspace = workspace_for_test(&team);
    let workspace_uid = workspace.uid;

    App::test((), |mut app| async move {
        initialize_window_team_test_app(&mut app, vec![]);

        let window_id = WindowId::new();
        UserWorkspaces::handle(&app).update(&mut app, |user_workspaces, ctx| {
            user_workspaces.register_window(window_id, None, ctx);
        });
        app.read(|ctx| {
            assert_eq!(
                UserWorkspaces::as_ref(ctx).team_uid_for_window(window_id),
                None
            );
        });

        UserWorkspaces::handle(&app).update(&mut app, |user_workspaces, ctx| {
            user_workspaces.update_workspaces(vec![workspace], ctx);
            user_workspaces.set_current_workspace_uid(workspace_uid, ctx);
        });

        app.read(|ctx| {
            assert_eq!(
                UserWorkspaces::as_ref(ctx).team_uid_for_window(window_id),
                Some(team.uid)
            );
        });
    })
}

#[test]
fn test_team_switcher_hidden_with_zero_teams() {
    // When the user is in no workspace / no teams, `can_switch_teams` must return
    // false so the pill does not render.
    App::test((), |mut app| async move {
        initialize_window_team_test_app(&mut app, vec![]);
        app.read(|ctx| {
            assert!(
                !UserWorkspaces::as_ref(ctx).can_switch_teams(),
                "0 teams: switcher should be hidden"
            );
        });
    })
}

#[test]
fn test_team_switcher_hidden_with_single_team() {
    // With exactly 1 team, `can_switch_teams` must return false.
    let team = team_for_test();
    let workspace = workspace_for_test(&team);
    App::test((), |mut app| async move {
        initialize_window_team_test_app(&mut app, vec![workspace]);
        app.read(|ctx| {
            assert!(
                !UserWorkspaces::as_ref(ctx).can_switch_teams(),
                "1 team: switcher should be hidden"
            );
        });
    })
}

#[test]
fn test_team_switcher_visible_with_multiple_teams() {
    // With 2+ teams, `can_switch_teams` must return true so the pill is shown.
    let team1 = team_for_test();
    let mut team2 = team_for_test();
    team2.uid = 456.into();
    team2.name = "Second Team".to_string();
    let mut workspace = workspace_for_test(&team1);
    workspace.teams.push(team2);

    App::test((), |mut app| async move {
        initialize_window_team_test_app(&mut app, vec![workspace]);
        app.read(|ctx| {
            assert!(
                UserWorkspaces::as_ref(ctx).can_switch_teams(),
                "2 teams: switcher should be visible"
            );
        });
    })
}
