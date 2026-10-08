use warpui::{App, SingletonEntity};

use super::{ObjectOperation, UpdateManager};
use crate::cloud_object::model::actions::ObjectActions;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{CloudObjectTypeAndId, Owner};
use crate::notebooks::{CloudNotebook, CloudNotebookModel};
use crate::server::ids::{ClientId, SyncId};

fn initialize_app(app: &mut App) {
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| UpdateManager::mock());
    app.add_singleton_model(|_| ObjectActions::new(Vec::new()));
}

/// Adds a locally created notebook, which is always keyed by a `ClientId`.
fn add_local_notebook(app: &mut App) -> CloudObjectTypeAndId {
    let client_id = ClientId::new();
    let notebook = CloudNotebook::new_local(
        CloudNotebookModel {
            title: "local".to_owned(),
            data: String::new(),
            ai_document_id: None,
            conversation_id: None,
        },
        Owner::mock_current_user(),
        None,
        client_id,
    );
    let id = SyncId::ClientId(client_id);
    CloudModel::handle(app).update(app, |model, ctx| model.create_object(id, notebook, ctx));
    CloudObjectTypeAndId::Notebook(id)
}

fn is_trashed(app: &App, id: CloudObjectTypeAndId) -> Option<bool> {
    CloudModel::handle(app).read(app, |model, _| {
        model
            .get_by_uid(&id.uid())
            .map(|object| object.metadata().trashed_ts.is_some())
    })
}

#[test]
fn locally_created_objects_can_be_trashed_untrashed_and_deleted() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let id = add_local_notebook(&mut app);
        assert_eq!(is_trashed(&app, id), Some(false));

        UpdateManager::handle(&app).update(&mut app, |manager, ctx| manager.trash_object(id, ctx));
        assert_eq!(is_trashed(&app, id), Some(true));

        UpdateManager::handle(&app)
            .update(&mut app, |manager, ctx| manager.untrash_object(id, ctx));
        assert_eq!(is_trashed(&app, id), Some(false));

        UpdateManager::handle(&app).update(&mut app, |manager, ctx| {
            manager.delete_object_by_user(id, ctx)
        });
        assert_eq!(is_trashed(&app, id), None);
    })
}

#[test]
fn operation_results_carry_the_client_id_for_local_objects() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let id = add_local_notebook(&mut app);
        let SyncId::ClientId(client_id) = id.sync_id() else {
            panic!("expected a client id");
        };

        let manager = UpdateManager::handle(&app);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        app.update(|ctx| {
            ctx.subscribe_to_model(&manager, move |_, event, _| {
                let super::UpdateManagerEvent::ObjectOperationComplete { result } = event;
                sink.lock().unwrap().push((
                    matches!(result.operation, ObjectOperation::Trash),
                    result.client_id,
                    result.server_id,
                ));
            });
        });

        manager.update(&mut app, |manager, ctx| manager.trash_object(id, ctx));

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], (true, Some(client_id), None));
    })
}
