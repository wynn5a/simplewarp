use cloud_objects::time::ServerTimestamp;
use warpui::{App, ModelHandle};

use super::*;
use crate::cloud_object::folders::CloudFolderModel;
use crate::cloud_object::model::generic_string_model::GenericStringModel;
use crate::cloud_object::{
    CloudObjectMetadata, CloudObjectPermissions, CloudObjectStatuses, CloudObjectSyncStatus, Owner,
};
use crate::notebooks::CloudNotebookModel;
use crate::server::ids::ClientId;
use crate::settings::{CloudPreference, Platform, Preference};

fn create_cloud_model(
    app: &mut App,
    objects: Vec<Box<dyn CloudObject>>,
) -> ModelHandle<CloudModel> {
    // Make sure to register the CloudModel singleton - some CloudObject methods
    // find it and other dependencies via the AppContext.
    app.add_singleton_model(|_ctx| CloudModel::new(None, objects))
}

fn mock_permissions() -> CloudObjectPermissions {
    CloudObjectPermissions {
        owner: Owner::mock_current_user(),
        guests: Vec::new(),
        permissions_last_updated_ts: None,
        anyone_with_link: None,
    }
}

fn mock_cloud_folder(id: SyncId, name: String, folder_id: Option<SyncId>) -> CloudFolder {
    CloudFolder::new(
        id,
        CloudFolderModel {
            name,
            is_open: true,
            is_warp_pack: false,
        },
        CloudObjectMetadata {
            pending_changes_statuses: CloudObjectStatuses {
                content_sync_status: CloudObjectSyncStatus::NoLocalChanges,
                has_pending_metadata_change: false,
                has_pending_permissions_change: false,
                pending_untrash: false,
                pending_delete: false,
            },
            folder_id,
            revision: Default::default(),
            metadata_last_updated_ts: Default::default(),
            current_editor_uid: Default::default(),
            trashed_ts: Default::default(),
            is_welcome_object: false,
            creator_uid: None,
            last_editor_uid: None,
            last_task_run_ts: None,
        },
        mock_permissions(),
    )
}

fn mock_cloud_notebook(id: SyncId, title: String, folder_id: Option<SyncId>) -> CloudNotebook {
    CloudNotebook::new(
        id,
        CloudNotebookModel {
            title,
            data: "test".into(),
            ai_document_id: None,
            conversation_id: None,
        },
        CloudObjectMetadata {
            pending_changes_statuses: CloudObjectStatuses {
                content_sync_status: CloudObjectSyncStatus::NoLocalChanges,
                has_pending_metadata_change: false,
                has_pending_permissions_change: false,
                pending_untrash: false,
                pending_delete: false,
            },
            folder_id,
            revision: Default::default(),
            metadata_last_updated_ts: Default::default(),
            current_editor_uid: Default::default(),
            trashed_ts: Default::default(),
            is_welcome_object: false,
            creator_uid: None,
            last_editor_uid: None,
            last_task_run_ts: None,
        },
        mock_permissions(),
    )
}

fn mock_trashed_cloud_folder(id: SyncId, name: String, folder_id: Option<SyncId>) -> CloudFolder {
    let mut folder = mock_cloud_folder(id, name, folder_id);
    folder.metadata.trashed_ts = Some(ServerTimestamp::from_unix_timestamp_micros(10).unwrap());
    folder
}

#[test]
fn test_create_json_object() {
    let client_id = ClientId::default();
    let id = SyncId::ClientId(client_id);
    let json_object: Box<dyn CloudObject> = Box::new(CloudPreference::new(
        id,
        GenericStringModel::new(
            Preference::new(
                "test_storage_key".to_owned(),
                "{\"test_key\": \"test_value\"}",
                Platform::Global,
            )
            .expect("error creating preference"),
        ),
        CloudObjectMetadata {
            pending_changes_statuses: CloudObjectStatuses {
                content_sync_status: CloudObjectSyncStatus::NoLocalChanges,
                has_pending_metadata_change: false,
                has_pending_permissions_change: false,
                pending_untrash: false,
                pending_delete: false,
            },
            folder_id: Default::default(),
            revision: Default::default(),
            metadata_last_updated_ts: Default::default(),
            current_editor_uid: Default::default(),
            trashed_ts: Default::default(),
            is_welcome_object: false,
            creator_uid: None,
            last_editor_uid: None,
            last_task_run_ts: None,
        },
        mock_permissions(),
    ));

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, vec![json_object]);
        cloud_model.read(&app, |model, _| {
            let json_object: &CloudPreference =
                model.get_object_of_type(&id).expect("model should exist");
            assert_eq!(
                json_object.model().string_model.storage_key,
                "test_storage_key".to_owned()
            );
        });
    })
}

/// Helper: compute active UIDs using the naive (non-memoized) is_trashed approach.
fn naive_active_object_uids(model: &CloudModel) -> HashSet<String> {
    model
        .as_cloud_objects()
        .filter(|obj| !obj.is_trashed(model))
        .map(|obj| obj.uid())
        .collect()
}

#[test]
fn active_object_uids_matches_naive_with_no_trashed_objects() {
    let folder_id = SyncId::ServerId(1.into());
    let objects: Vec<Box<dyn CloudObject>> = vec![
        Box::new(mock_cloud_folder(folder_id, "Folder".into(), None)),
        Box::new(mock_cloud_notebook(
            SyncId::ServerId(2.into()),
            "Notebook".into(),
            Some(folder_id),
        )),
    ];

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, objects);
        cloud_model.read(&app, |model, _| {
            assert_eq!(model.active_object_uids(), naive_active_object_uids(model));
            assert_eq!(model.active_object_uids().len(), 2);
        });
    });
}

#[test]
fn active_object_uids_matches_naive_with_directly_trashed_object() {
    let trashed_folder_id = SyncId::ServerId(1.into());
    let active_notebook_id = SyncId::ServerId(2.into());
    let objects: Vec<Box<dyn CloudObject>> = vec![
        Box::new(mock_trashed_cloud_folder(
            trashed_folder_id,
            "Trashed Folder".into(),
            None,
        )),
        Box::new(mock_cloud_notebook(
            active_notebook_id,
            "Active Notebook".into(),
            None,
        )),
    ];

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, objects);
        cloud_model.read(&app, |model, _| {
            let active = model.active_object_uids();
            assert_eq!(active, naive_active_object_uids(model));
            assert_eq!(active.len(), 1);
            assert!(active.contains(&active_notebook_id.uid()));
            assert!(!active.contains(&trashed_folder_id.uid()));
        });
    });
}

#[test]
fn active_object_uids_matches_naive_with_indirectly_trashed_children() {
    // A trashed folder with a non-trashed notebook inside it.
    // The notebook should be considered trashed (indirectly) by both approaches.
    let trashed_folder_id = SyncId::ServerId(1.into());
    let child_notebook_id = SyncId::ServerId(2.into());
    let active_notebook_id = SyncId::ServerId(3.into());
    let objects: Vec<Box<dyn CloudObject>> = vec![
        Box::new(mock_trashed_cloud_folder(
            trashed_folder_id,
            "Trashed Folder".into(),
            None,
        )),
        Box::new(mock_cloud_notebook(
            child_notebook_id,
            "Child in Trashed Folder".into(),
            Some(trashed_folder_id),
        )),
        Box::new(mock_cloud_notebook(
            active_notebook_id,
            "Top-level Notebook".into(),
            None,
        )),
    ];

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, objects);
        cloud_model.read(&app, |model, _| {
            let active = model.active_object_uids();
            assert_eq!(active, naive_active_object_uids(model));
            assert_eq!(active.len(), 1);
            assert!(active.contains(&active_notebook_id.uid()));
        });
    });
}

#[test]
fn active_object_uids_matches_naive_with_nested_trashed_folder() {
    // folder_a (trashed) -> folder_b (not trashed) -> notebook (not trashed)
    // Both folder_b and notebook should be indirectly trashed.
    let folder_a_id = SyncId::ServerId(1.into());
    let folder_b_id = SyncId::ServerId(2.into());
    let notebook_id = SyncId::ServerId(3.into());
    let active_notebook_id = SyncId::ServerId(4.into());
    let objects: Vec<Box<dyn CloudObject>> = vec![
        Box::new(mock_trashed_cloud_folder(
            folder_a_id,
            "Folder A (trashed)".into(),
            None,
        )),
        Box::new(mock_cloud_folder(
            folder_b_id,
            "Folder B".into(),
            Some(folder_a_id),
        )),
        Box::new(mock_cloud_notebook(
            notebook_id,
            "Deeply nested".into(),
            Some(folder_b_id),
        )),
        Box::new(mock_cloud_notebook(
            active_notebook_id,
            "Active".into(),
            None,
        )),
    ];

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, objects);
        cloud_model.read(&app, |model, _| {
            let active = model.active_object_uids();
            assert_eq!(active, naive_active_object_uids(model));
            assert_eq!(active.len(), 1);
            assert!(active.contains(&active_notebook_id.uid()));
        });
    });
}

#[test]
fn active_object_uids_matches_naive_with_empty_model() {
    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, vec![]);
        cloud_model.read(&app, |model, _| {
            let active = model.active_object_uids();
            assert_eq!(active, naive_active_object_uids(model));
            assert!(active.is_empty());
        });
    });
}
