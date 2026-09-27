use warpui::integration::TestStep;
use warpui::{SingletonEntity, async_assert, async_assert_eq};

use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{CloudObjectLocation, Space};
use crate::network::{NetworkStatus, NetworkStatusKind};
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::ids::ClientId;
use crate::util::bindings::keybinding_name_to_display_string;
use crate::workflows::workflow::Workflow;
use crate::workspaces::user_workspaces::UserWorkspaces;

fn set_and_assert_network_status(status: NetworkStatusKind) -> TestStep {
    TestStep::new("Set and assert network status")
        .with_action(move |app, _, _| {
            NetworkStatus::handle(app).update(app, |network_status, ctx| {
                if matches!(status, NetworkStatusKind::Online) {
                    network_status.reachability_changed(true, ctx);
                } else {
                    network_status.reachability_changed(false, ctx);
                }
            });
        })
        .add_assertion(move |app, _| {
            NetworkStatus::handle(app).read(app, |network_status, _| {
                async_assert!(
                    network_status.status() == status,
                    "network status is correct"
                )
            })
        })
}

pub fn go_offline() -> TestStep {
    set_and_assert_network_status(NetworkStatusKind::Offline)
}

pub fn go_online() -> TestStep {
    set_and_assert_network_status(NetworkStatusKind::Online)
}

pub fn create_a_personal_workflow() -> TestStep {
    TestStep::new("Create a personal workflow")
        .with_action(move |app, _, _| {
            UpdateManager::handle(app).update(app, |update_manager, ctx| {
                update_manager.create_workflow(
                    Workflow::new("My first workflow", "ls"),
                    UserWorkspaces::as_ref(ctx)
                        .personal_drive(ctx)
                        .expect("User UID must be set in tests"),
                    None,
                    ClientId::default(),
                    true,
                    ctx,
                )
            })
        })
        .add_assertion(move |app, _| {
            CloudModel::handle(app).read(app, |cloud_model, ctx| {
                async_assert!(
                    cloud_model
                        .active_cloud_objects_in_location_without_descendents(
                            CloudObjectLocation::Space(Space::Personal),
                            ctx,
                        )
                        .count()
                        > 0,
                    "cloud objects exist"
                )
            })
        })
}

pub fn assert_binding_display_string(
    binding: &'static str,
    display_string: Option<&'static str>,
) -> TestStep {
    TestStep::new("Assert a binding's display string").add_named_assertion(
        format!("Binding {binding} should have display string {display_string:?}"),
        move |app, _| {
            app.update(|ctx| {
                async_assert_eq!(
                    keybinding_name_to_display_string(binding, ctx).as_deref(),
                    display_string
                )
            })
        },
    )
}
