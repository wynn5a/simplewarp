use warp_core::ui::appearance::Appearance;
use warpui::platform::WindowStyle;
use warpui::{AddSingletonModel, App, SingletonEntity, TypedActionView, ViewHandle};

use super::{DriveIndex, DriveIndexAction, SharedObjectLimitBannerKind};
use crate::ASSETS;
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::auth::AuthStateProvider;
use crate::auth::auth_manager::AuthManager;
use crate::cloud_object::model::actions::ObjectActions;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::model::view::CloudViewModel;
use crate::cloud_object::{ObjectType, Owner, WarpDriveItemId};
use crate::drive::CloudObjectTypeAndId;
use crate::menu::MenuItem;
use crate::network::NetworkStatus;
use crate::notebooks::{CloudNotebook, CloudNotebookModel};
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::ids::{ClientId, SyncId};
use crate::server::server_api::ServerApiProvider;
use crate::server::telemetry::context_provider::AppTelemetryContextProvider;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workflows::workflow::Workflow;
use crate::workflows::{CloudWorkflow, CloudWorkflowModel};
use crate::workspaces::user_profiles::UserProfiles;
use crate::workspaces::user_workspaces::UserWorkspaces;

fn initialize_app(app: &mut App) {
    initialize_settings_for_tests(app);

    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(UserWorkspaces::default_mock);
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(AppTelemetryContextProvider::new_context_provider);
    app.add_singleton_model(AuthManager::new_for_test);
    app.add_singleton_model(|_| UpdateManager::mock());
    app.add_singleton_model(CloudViewModel::mock);
    app.add_singleton_model(|_| ObjectActions::new(Vec::new()));
    app.add_singleton_model(|_| UserProfiles::new(Vec::new()));
    app.add_singleton_model(|_| KeybindingChangedNotifier::mock());
    app.add_singleton_model(|_| BlocklistAIHistoryModel::new_for_test());
    #[cfg(feature = "voice_input")]
    app.add_singleton_model(voice_input::VoiceInput::new);
}

fn create_index(app: &mut App) -> ViewHandle<DriveIndex> {
    let (_, index) = app.add_window(WindowStyle::NotStealFocus, DriveIndex::new);
    index
}

fn create_workflow(app: &mut App) -> SyncId {
    CloudModel::handle(app).update(app, |cloud_model, ctx| {
        let client_id = ClientId::new();
        let sync_id = SyncId::ClientId(client_id);
        let workflow = Workflow::new("my workflow", "my command");
        cloud_model.create_object(
            sync_id,
            CloudWorkflow::new_local(
                CloudWorkflowModel::new(workflow),
                Owner::mock_current_user(),
                None,
                client_id,
            ),
            ctx,
        );
        sync_id
    })
}

fn create_notebook(app: &mut App) -> SyncId {
    CloudModel::handle(app).update(app, |cloud_model, ctx| {
        let client_id = ClientId::new();
        let sync_id = SyncId::ClientId(client_id);
        cloud_model.create_object(
            sync_id,
            CloudNotebook::new_local(
                CloudNotebookModel::default(),
                Owner::mock_current_user(),
                None,
                client_id,
            ),
            ctx,
        );
        sync_id
    })
}
fn label_for_menu_item(item: &MenuItem<DriveIndexAction>) -> &str {
    if let MenuItem::Item(item) = item {
        item.label()
    } else {
        panic!("item provided wasn't of type MenuItem::Item")
    }
}

#[test]
fn test_warp_drive_navigation_states() {
    use crate::drive::index::DriveIndexAction;
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let index = create_index(&mut app);
        let sync_id = create_notebook(&mut app);
        let cloud_object_type_and_id: CloudObjectTypeAndId =
            CloudObjectTypeAndId::from_id_and_type(sync_id, ObjectType::Notebook);

        index.read(&app, |index, _| {
            assert_eq!(index.selected, None, "Expect selected to be None");
            assert_eq!(
                index.focused_index,
                Some(0),
                "Expect focused_index to be initialized"
            );
        });

        index.update(&mut app, |index, ctx| {
            index.handle_action(&DriveIndexAction::OpenObject(cloud_object_type_and_id), ctx);
        });

        index.read(&app, |index, _| {
            assert_eq!(
                index.selected,
                Some(WarpDriveItemId::Object(cloud_object_type_and_id)),
                "Expect selected to have correct value"
            );
        });
    });
}

#[test]
fn test_shared_object_limit_banner_dismissal_persists_per_type() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let index = create_index(&mut app);

        // Neither banner is dismissed by default.
        index.read(&app, |_index, cx| {
            assert!(!DriveIndex::is_object_limit_banner_dismissed(
                SharedObjectLimitBannerKind::Notebook,
                cx,
            ));
            assert!(!DriveIndex::is_object_limit_banner_dismissed(
                SharedObjectLimitBannerKind::Workflow,
                cx,
            ));
        });

        // Dismissing the notebook banner is remembered.
        index.update(&mut app, |index, ctx| {
            index.handle_action(
                &DriveIndexAction::DismissObjectLimitBanner {
                    banner_kind: SharedObjectLimitBannerKind::Notebook,
                },
                ctx,
            );
        });

        // The notebook banner stays dismissed, and the workflow banner is
        // unaffected — dismissal is tracked per object type.
        index.read(&app, |_index, cx| {
            assert!(DriveIndex::is_object_limit_banner_dismissed(
                SharedObjectLimitBannerKind::Notebook,
                cx,
            ));
            assert!(!DriveIndex::is_object_limit_banner_dismissed(
                SharedObjectLimitBannerKind::Workflow,
                cx,
            ));
        });
    });
}
