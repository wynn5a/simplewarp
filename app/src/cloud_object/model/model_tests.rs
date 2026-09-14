use chrono::Utc;
use lazy_static::lazy_static;
use settings::{RespectUserSyncSetting, SyncToCloud};
use warpui::{App, ModelHandle};

use super::*;
use crate::cloud_object::folders::{CloudFolderModel, FolderId};
use crate::cloud_object::model::generic_string_model::GenericStringModel;
use crate::cloud_object::{
    CloudObjectMetadata, CloudObjectPermissions, CloudObjectStatuses, CloudObjectSyncStatus,
    NumInFlightRequests, ObjectIdType, Owner, ServerMetadata, ServerPermissions,
};
use crate::drive::DriveIndexVariant;
use crate::notebooks::{CloudNotebookModel, NotebookId};
use crate::server::ids::{ServerId, ServerIdAndType};
use crate::settings::Preference;
use crate::workflows::CloudWorkflowModel;
use crate::workspaces::team::Team;
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::workspaces::workspace::{Workspace, WorkspaceUid};

fn create_cloud_model(
    app: &mut App,
    objects: Vec<Box<dyn CloudObject>>,
) -> ModelHandle<CloudModel> {
    // Make sure to register the CloudModel singleton - some CloudObject methods
    // find it and other dependencies via the AppContext.
    app.add_singleton_model(|_ctx| CloudModel::new(None, objects, None))
}

lazy_static! {
    /// Mock the user being on _a_ team in tests, so that the team drive is available.
    /// Otherwise, any team objects will appear shared.
    static ref TEST_TEAM: Team = Team::from_local_cache(
        ServerId::from(1),
        "Test Team".to_string(),
        None,
        None,
        None,
    );

    static ref TEST_WORKSPACE: Workspace = Workspace::from_local_cache(
        WorkspaceUid::from(ServerId::from(1)),
        "Test Workspace".to_string(),
        Some(vec![TEST_TEAM.clone()]),
    );
}

fn mock_server_metadata() -> ServerMetadata {
    ServerMetadata {
        uid: ServerId::default(),
        revision: Revision::now(),
        metadata_last_updated_ts: Utc::now().into(),
        trashed_ts: None,
        folder_id: None,
        is_welcome_object: false,
        creator_uid: None,
        last_editor_uid: None,
        current_editor_uid: None,
    }
}

fn mock_server_permissions(owner: Owner) -> ServerPermissions {
    ServerPermissions {
        space: owner,
        guests: Vec::new(),
        permissions_last_updated_ts: Utc::now().into(),
        anyone_link_sharing: None,
    }
}

fn mock_permissions() -> CloudObjectPermissions {
    CloudObjectPermissions {
        owner: Owner::mock_current_user(),
        guests: Vec::new(),
        permissions_last_updated_ts: None,
        anyone_with_link: None,
    }
}

fn mock_server_workflows(
    start_id: i64,
    owner: Owner,
    number_of_workflows: i64,
) -> Vec<ServerWorkflow> {
    (0..number_of_workflows)
        .map(|idx| {
            ServerWorkflow::new(
                SyncId::ServerId((start_id + idx).into()),
                CloudWorkflowModel::new(Workflow::new(
                    format!("w{}", start_id + idx),
                    format!("c{}", start_id + idx),
                )),
                mock_server_metadata(),
                mock_server_permissions(owner),
            )
        })
        .collect()
}

fn mock_server_notebooks() -> Vec<ServerNotebook> {
    let owner = Owner::mock_current_user();
    vec![
        ServerNotebook::new(
            SyncId::ServerId(1.into()),
            CloudNotebookModel {
                title: "t1".to_string(),
                data: "d1".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            mock_server_metadata(),
            mock_server_permissions(owner),
        ),
        ServerNotebook::new(
            SyncId::ServerId(2.into()),
            CloudNotebookModel {
                title: "t2".to_string(),
                data: "d2".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            mock_server_metadata(),
            mock_server_permissions(owner),
        ),
        ServerNotebook::new(
            SyncId::ServerId(3.into()),
            CloudNotebookModel {
                title: "t3".to_string(),
                data: "d3".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            mock_server_metadata(),
            mock_server_permissions(owner),
        ),
        ServerNotebook::new(
            SyncId::ServerId(4.into()),
            CloudNotebookModel {
                title: "t4".to_string(),
                data: "d4".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            mock_server_metadata(),
            mock_server_permissions(owner),
        ),
    ]
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

fn folder_from_cloud_model(model: &CloudModel, id: SyncId) -> &CloudFolder {
    model.get_folder_by_uid(&id.uid()).expect("is a folder")
}

#[test]
fn test_update_with_deleted_objects() {
    let workflows = mock_server_workflows(
        5,
        Owner::Team {
            team_uid: ServerId::from(1),
        },
        3,
    );
    let notebooks = mock_server_notebooks();

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(
            &mut app,
            workflows
                .iter()
                .map(|workflow| CloudWorkflow::new_from_server(workflow.clone()))
                .map(|o| Box::new(o) as Box<dyn CloudObject>)
                .collect(),
        );
        cloud_model.update(&mut app, |model, ctx| {
            for notebook in notebooks.clone() {
                model.upsert_from_server_notebook(notebook, ctx);
            }
        });

        // Validate there's some notebooks and workflows in memory
        cloud_model.read(&app, |cloud_model, _| {
            assert_eq!(
                3,
                cloud_model.get_all_active_and_inactive_workflows().count()
            );
            assert_eq!(
                4,
                cloud_model.get_all_active_and_inactive_notebooks().count()
            );
            assert_eq!(7, cloud_model.as_cloud_objects().count());
        });

        // Apply the "update from server"
        cloud_model.update(&mut app, |cloud_model, ctx| {
            // Set 3rd notebook to have pending changes. This should keep it in memory,
            // even though it's not returned from the server.
            let notebook_id: SyncId = SyncId::ServerId(3.into());
            if let Some(object) = cloud_model.get_notebook_mut(&notebook_id) {
                object.set_pending_content_changes_status(CloudObjectSyncStatus::InFlight(
                    NumInFlightRequests(1),
                ));
            }
            cloud_model.update_objects(notebooks.into_iter().take(2), ctx);
            cloud_model.update_objects(workflows.into_iter().take(2), ctx);
        });

        cloud_model.read(&app, |cloud_model, _| {
            // expected: 3rd workflow was removed on the server, and so we don't want it in
            // memory
            assert_eq!(
                2,
                cloud_model.get_all_active_and_inactive_workflows().count()
            );
            // expected: 3rd notebook has local changes, so we want to keep it, but 4th
            // doesn't and also wasn't returned from the server, so we want to remove it.
            assert_eq!(
                3,
                cloud_model.get_all_active_and_inactive_notebooks().count()
            );
            assert_eq!(5, cloud_model.as_cloud_objects().count());
        });
    })
}

#[test]
fn test_update_object_server_id_for_notebook() {
    let client_id = ClientId::new();
    let server_id: NotebookId = 1.into();
    let notebooks: Vec<Box<dyn CloudObject>> = vec![Box::new(CloudNotebook::new(
        SyncId::ClientId(client_id),
        CloudNotebookModel {
            title: "t1".to_string(),
            data: "d1".to_string(),
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
    ))];

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, notebooks);
        cloud_model.update(&mut app, |model, ctx| {
            model.update_object_after_server_creation(
                client_id,
                ServerCreationInfo {
                    creator_uid: None,
                    permissions: ServerPermissions::mock_personal(),
                    server_id_and_type: ServerIdAndType {
                        id: server_id.to_server_id(),
                        id_type: ObjectIdType::Notebook,
                    },
                },
                ctx,
            )
        });

        cloud_model.read(&app, |model, _| {
            let notebook = model
                .get_notebook(&SyncId::ServerId(server_id.into()))
                .unwrap();
            assert_eq!(notebook.id, SyncId::ServerId(server_id.into()));
        });
    })
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
                SyncToCloud::Globally(RespectUserSyncSetting::Yes),
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

#[test]
fn test_update_object_server_id_for_workflow() {
    let client_id = ClientId::new();
    let server_id: ServerId = 1.into();
    let workflows: Vec<Box<dyn CloudObject>> = vec![Box::new(CloudWorkflow::new(
        SyncId::ServerId(1.into()),
        CloudWorkflowModel::new(Workflow::new("w1", "c1")),
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
    ))];
    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, workflows);
        cloud_model.update(&mut app, |model, ctx| {
            model.update_object_after_server_creation(
                client_id,
                ServerCreationInfo {
                    creator_uid: None,
                    permissions: ServerPermissions::mock_personal(),
                    server_id_and_type: ServerIdAndType {
                        id: server_id,
                        id_type: ObjectIdType::Workflow,
                    },
                },
                ctx,
            )
        });

        cloud_model.read(&app, |model, _| {
            let workflow = model.get_workflow(&SyncId::ServerId(server_id)).unwrap();
            assert_eq!(workflow.id, SyncId::ServerId(server_id));
        });
    })
}

#[test]
fn test_update_object_server_id_for_folder() {
    let client_id = ClientId::new();
    let server_id: FolderId = 1.into();
    let folders: Vec<Box<dyn CloudObject>> = vec![Box::new(CloudFolder::new(
        SyncId::ServerId(1.into()),
        CloudFolderModel::new("test", false),
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
    ))];
    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, folders);
        cloud_model.update(&mut app, |model, ctx| {
            model.update_object_after_server_creation(
                client_id,
                ServerCreationInfo {
                    creator_uid: None,
                    permissions: ServerPermissions::mock_personal(),
                    server_id_and_type: ServerIdAndType {
                        id: server_id.to_server_id(),
                        id_type: ObjectIdType::Folder,
                    },
                },
                ctx,
            )
        });

        cloud_model.read(&app, |model, _| {
            let folder = model
                .get_folder_by_uid(&SyncId::ServerId(server_id.into()).uid())
                .unwrap();
            assert_eq!(folder.id, SyncId::ServerId(server_id.into()));
        });
    })
}

#[test]
fn test_collapse_all_in_location() {
    /*
       the folder structure looks like:

       test1
        ↳ test 4
         ↳ test 5
       test 2
        ↳ test 6
         ↳ test 7
       test 3

    */
    let folder_1_id: SyncId = SyncId::ServerId(1.into());
    let folder_2_id: SyncId = SyncId::ServerId(2.into());
    let folder_3_id: SyncId = SyncId::ServerId(3.into());
    let folder_4_id: SyncId = SyncId::ServerId(4.into());
    let folder_5_id: SyncId = SyncId::ServerId(5.into());
    let folder_6_id: SyncId = SyncId::ServerId(6.into());
    let folder_7_id: SyncId = SyncId::ServerId(7.into());

    let folders = vec![
        mock_cloud_folder(folder_1_id, "test1".to_string(), None),
        mock_cloud_folder(folder_2_id, "test2".to_string(), None),
        mock_cloud_folder(folder_3_id, "test3".to_string(), None),
        mock_cloud_folder(folder_4_id, "test4".to_string(), Some(folder_1_id)),
        mock_cloud_folder(folder_5_id, "test5".to_string(), Some(folder_4_id)),
        mock_cloud_folder(folder_6_id, "test6".to_string(), Some(folder_2_id)),
        mock_cloud_folder(folder_7_id, "test7".to_string(), Some(folder_6_id)),
    ]
    .into_iter()
    .map(|o| Box::new(o) as Box<dyn CloudObject>)
    .collect();

    App::test((), |mut app| async move {
        app.add_singleton_model(UserWorkspaces::default_mock);
        let cloud_model = create_cloud_model(&mut app, folders);

        cloud_model.update(&mut app, |model, ctx| {
            // first, collapse all folders in folder 1
            model.collapse_all_in_location(
                CloudObjectLocation::Folder(folder_1_id),
                DriveIndexVariant::MainIndex,
                ctx,
            );

            // folders 1, 4, and 5 should be collapsed
            let folder_1 = folder_from_cloud_model(model, folder_1_id);
            let folder_4 = folder_from_cloud_model(model, folder_4_id);
            let folder_5 = folder_from_cloud_model(model, folder_5_id);
            assert!(!folder_1.model().is_open);
            assert!(!folder_4.model().is_open);
            assert!(!folder_5.model().is_open);
            // but the others are still open
            let folder_2 = folder_from_cloud_model(model, folder_2_id);
            let folder_3 = folder_from_cloud_model(model, folder_3_id);
            let folder_6 = folder_from_cloud_model(model, folder_6_id);
            let folder_7 = folder_from_cloud_model(model, folder_7_id);
            assert!(folder_2.model().is_open);
            assert!(folder_3.model().is_open);
            assert!(folder_6.model().is_open);
            assert!(folder_7.model().is_open);

            model.collapse_all_in_location(
                CloudObjectLocation::Space(Default::default()),
                DriveIndexVariant::MainIndex,
                ctx,
            );
            // now all folders in this space are collapsed
            let folder_1 = folder_from_cloud_model(model, folder_1_id);
            let folder_2 = folder_from_cloud_model(model, folder_2_id);
            let folder_3 = folder_from_cloud_model(model, folder_3_id);
            let folder_4 = folder_from_cloud_model(model, folder_4_id);
            let folder_5 = folder_from_cloud_model(model, folder_5_id);
            let folder_6 = folder_from_cloud_model(model, folder_6_id);
            let folder_7 = folder_from_cloud_model(model, folder_7_id);
            assert!(!folder_1.model().is_open);
            assert!(!folder_2.model().is_open);
            assert!(!folder_3.model().is_open);
            assert!(!folder_4.model().is_open);
            assert!(!folder_5.model().is_open);
            assert!(!folder_6.model().is_open);
            assert!(!folder_7.model().is_open);
        });
    })
}

#[test]
fn test_collapse_all_in_trash() {
    /*
       the folder structure looks like:

       test1 -- trashed by user
        ↳ test 4
         ↳ test 5 -- trashed by user
       test 2 -- trashed by user
        ↳ test 6
         ↳ test 7
       test 3 -- trashed by user

       the structure in the trash index looks like:

       test1 -- trashed by user
        ↳ test 4
       test 5 -- trashed by user
       test 2 -- trashed by user
        ↳ test 6
         ↳ test 7
       test 3 -- trashed by user

    */
    let folder_1_id: SyncId = SyncId::ServerId(1.into());
    let folder_2_id: SyncId = SyncId::ServerId(2.into());
    let folder_3_id: SyncId = SyncId::ServerId(3.into());
    let folder_4_id: SyncId = SyncId::ServerId(4.into());
    let folder_5_id: SyncId = SyncId::ServerId(5.into());
    let folder_6_id: SyncId = SyncId::ServerId(6.into());
    let folder_7_id: SyncId = SyncId::ServerId(7.into());

    let folders = vec![
        mock_trashed_cloud_folder(folder_1_id, "test1".to_string(), None),
        mock_trashed_cloud_folder(folder_2_id, "test2".to_string(), None),
        mock_trashed_cloud_folder(folder_3_id, "test3".to_string(), None),
        mock_cloud_folder(folder_4_id, "test4".to_string(), Some(folder_1_id)),
        mock_trashed_cloud_folder(folder_5_id, "test5".to_string(), Some(folder_4_id)),
        mock_cloud_folder(folder_6_id, "test6".to_string(), Some(folder_2_id)),
        mock_cloud_folder(folder_7_id, "test7".to_string(), Some(folder_6_id)),
    ]
    .into_iter()
    .map(|o| Box::new(o) as Box<dyn CloudObject>)
    .collect();

    App::test((), |mut app| async move {
        app.add_singleton_model(UserWorkspaces::default_mock);
        let cloud_model = create_cloud_model(&mut app, folders);

        cloud_model.update(&mut app, |model, ctx| {
            // first, collapse all folders in folder 1
            model.collapse_all_in_location(
                CloudObjectLocation::Folder(folder_1_id),
                DriveIndexVariant::Trash,
                ctx,
            );

            // folders 1, 4 should be collapsed
            let folder_1 = folder_from_cloud_model(model, folder_1_id);
            let folder_4 = folder_from_cloud_model(model, folder_4_id);
            assert!(!folder_1.model().is_open);
            assert!(!folder_4.model().is_open);
            // but the others, including folder 5, are still open
            let folder_2 = folder_from_cloud_model(model, folder_2_id);
            let folder_3 = folder_from_cloud_model(model, folder_3_id);
            let folder_5 = folder_from_cloud_model(model, folder_5_id);
            let folder_6 = folder_from_cloud_model(model, folder_6_id);
            let folder_7 = folder_from_cloud_model(model, folder_7_id);
            assert!(folder_2.model().is_open);
            assert!(folder_3.model().is_open);
            assert!(folder_5.model().is_open);
            assert!(folder_6.model().is_open);
            assert!(folder_7.model().is_open);

            model.collapse_all_in_location(
                CloudObjectLocation::Space(Default::default()),
                DriveIndexVariant::Trash,
                ctx,
            );
            // now all folders in this space are collapsed
            let folder_1 = folder_from_cloud_model(model, folder_1_id);
            let folder_2 = folder_from_cloud_model(model, folder_2_id);
            let folder_3 = folder_from_cloud_model(model, folder_3_id);
            let folder_4 = folder_from_cloud_model(model, folder_4_id);
            let folder_5 = folder_from_cloud_model(model, folder_5_id);
            let folder_6 = folder_from_cloud_model(model, folder_6_id);
            let folder_7 = folder_from_cloud_model(model, folder_7_id);
            assert!(!folder_1.model().is_open);
            assert!(!folder_2.model().is_open);
            assert!(!folder_3.model().is_open);
            assert!(!folder_4.model().is_open);
            assert!(!folder_5.model().is_open);
            assert!(!folder_6.model().is_open);
            assert!(!folder_7.model().is_open);
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
