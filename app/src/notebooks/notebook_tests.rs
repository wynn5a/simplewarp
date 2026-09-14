use std::sync::Arc;

use chrono::{Duration, Utc};
use itertools::Itertools;
use warp_core::ui::appearance::Appearance;
use warp_editor::editor::EditorView;
use warpui::r#async::Timer;
use warpui::platform::WindowStyle;
use warpui::presenter::ChildView;
use warpui::telemetry::EventPayload;
use warpui::{
    AddSingletonModel, App, AppContext, Element, Entity, SingletonEntity, TypedActionView, View,
    ViewHandle, WindowId,
};

use super::{EDIT_WINDOW_DURATION, NotebookEvent, NotebookView, SAVE_PERIOD};
use crate::auth::auth_manager::AuthManager;
use crate::auth::user::{TEST_USER_EMAIL, TEST_USER_UID};
use crate::auth::{AuthStateProvider, UserUid};
use crate::cloud_object::model::actions::ObjectActions;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::model::view::{CloudViewModel, Editor, EditorState};
use crate::cloud_object::{
    OpenWarpDriveObjectSettings, Owner, Revision, ServerMetadata, ServerNotebook, ServerPermissions,
};
use crate::editor::{DisplayPoint, EditorAction, SelectAction};
use crate::network::NetworkStatus;
use crate::notebooks::active_notebook_data::Mode;
use crate::notebooks::editor::keys::NotebookKeybindings;
use crate::notebooks::editor::notebook_command::NotebookCommand;
use crate::notebooks::editor::view::EditorViewAction;
use crate::notebooks::notebook::FocusedComponent;
use crate::notebooks::{CloudNotebook, CloudNotebookModel, NotebookLocation};
use crate::pane_group::PaneEvent;
use crate::search::files::model::FileSearchModel;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::ids::ClientId;
use crate::server::ids::SyncId::ServerId;
use crate::server::server_api::ServerApiProvider;
use crate::server::telemetry::context_provider::AppTelemetryContextProvider;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::terminal::keys::TerminalKeybindings;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workflows::workflow::Workflow;
use crate::workflows::{WorkflowSource, WorkflowType};
use crate::workspace::ActiveSession;
use crate::workspaces::user_profiles::{UserProfileWithUID, UserProfiles};
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::{GlobalResourceHandles, GlobalResourceHandlesProvider, PrivacySettings};

fn initialize_app(app: &mut App) {
    initialize_settings_for_tests(app);

    let global_resources = GlobalResourceHandles::mock(app);
    app.add_singleton_model(|_| GlobalResourceHandlesProvider::new(global_resources));
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(|_| KeybindingChangedNotifier::new());
    app.add_singleton_model(|_| repo_metadata::repositories::DetectedRepositories::default());
    #[cfg(feature = "local_fs")]
    app.add_singleton_model(repo_metadata::RepoMetadataModel::new);
    app.add_singleton_model(FileSearchModel::new);
    app.add_singleton_model(NotebookKeybindings::new);
    app.add_singleton_model(TerminalKeybindings::new);
    app.add_singleton_model(PrivacySettings::mock);
    app.add_singleton_model(UserWorkspaces::default_mock);
    app.add_singleton_model(|_| UpdateManager::mock());
    app.add_singleton_model(CloudViewModel::mock);
    app.add_singleton_model(|_| UserProfiles::new(vec![]));
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| ActiveSession::default());
    app.add_singleton_model(|_| ObjectActions::new(Vec::new()));
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(AppTelemetryContextProvider::new_context_provider);
    app.add_singleton_model(AuthManager::new_for_test);
    #[cfg(feature = "voice_input")]
    app.add_singleton_model(voice_input::VoiceInput::new);
}

/// Container so that [`NotebookView`] can be registered as a typed action view.
struct Root {
    notebook: ViewHandle<NotebookView>,
    events: Vec<NotebookEvent>,
}

impl Entity for Root {
    type Event = ();
}

impl View for Root {
    fn ui_name() -> &'static str {
        "Root"
    }

    fn render(&self, _: &AppContext) -> Box<dyn Element> {
        ChildView::new(&self.notebook).finish()
    }
}

impl TypedActionView for Root {
    type Action = ();
}

fn create_notebook(app: &mut App) -> (WindowId, ViewHandle<NotebookView>, ViewHandle<Root>) {
    let (window, root) = app.add_window(WindowStyle::NotStealFocus, |ctx| {
        let notebook = ctx.add_typed_action_view(NotebookView::new);

        ctx.subscribe_to_view(&notebook, |me: &mut Root, _, event, _| {
            me.events.push(event.clone())
        });

        Root {
            notebook,
            events: Vec::new(),
        }
    });
    let notebook = app.read(|ctx| root.as_ref(ctx).notebook.clone());
    (window, notebook, root)
}

/// Opens a notebook in the given view.
async fn open_notebook(app: &mut App, handle: &ViewHandle<NotebookView>, notebook: CloudNotebook) {
    handle.update(app, |view, ctx| {
        view.load(notebook, &OpenWarpDriveObjectSettings::default(), ctx);
    });
    // Pump the executor once so render effects settle: command block models are
    // built on LayoutUpdated, which is what the old baton-future await did.
    futures_lite::future::yield_now().await;
}

fn cloud_notebook(title: impl Into<String>, data: impl Into<String>) -> CloudNotebook {
    CloudNotebook::new_local(
        CloudNotebookModel {
            title: title.into(),
            data: data.into(),
            ai_document_id: None,
            conversation_id: None,
        },
        Owner::mock_current_user(),
        None,
        ClientId::new(),
    )
}

/// Mock a server notebook
fn mock_server_notebook(title: impl Into<String>, data: impl Into<String>) -> ServerNotebook {
    let metadata_ts = Utc::now().into();
    ServerNotebook::new(
        ServerId(123.into()),
        CloudNotebookModel {
            title: title.into(),
            data: data.into(),
            ai_document_id: None,
            conversation_id: None,
        },
        ServerMetadata {
            uid: 123.into(),
            revision: Revision::now(),
            metadata_last_updated_ts: metadata_ts,
            trashed_ts: None,
            folder_id: None,
            is_welcome_object: false,
            creator_uid: None,
            last_editor_uid: None,
            current_editor_uid: None,
        },
        ServerPermissions {
            space: Owner::mock_current_user(),
            guests: Vec::new(),
            anyone_link_sharing: None,
            permissions_last_updated_ts: metadata_ts,
        },
    )
}

/// Upsert server notebooks into the cloud model so that tests requiring
/// "up-to-date" notebooks can run.
async fn initial_load(app: &mut App, updated_notebooks: impl Into<Vec<ServerNotebook>>) {
    CloudModel::handle(app).update(app, |cloud_model, ctx| {
        for notebook in updated_notebooks.into() {
            cloud_model.upsert_from_server_notebook(notebook, ctx);
        }
    });
}

/// Wait for all edits to be saved.
async fn ensure_saved(app: &mut App, notebook_view: &ViewHandle<NotebookView>) {
    loop {
        let has_edits = notebook_view.read(app, |notebook, _| {
            notebook.content_is_dirty || notebook.title_is_dirty
        });
        if has_edits {
            Timer::after(SAVE_PERIOD).await;
        } else {
            break;
        }
    }

    // Ensure that any updates from the debounced save were processed.
    app.update(|_| ());
}

/// Test that command-block execution events are correctly translated into workflows.
#[test]
fn test_command_block_dispatches_event() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        initial_load(&mut app, []).await;

        let (window, notebook, root) = create_notebook(&mut app);
        open_notebook(
            &mut app,
            &notebook,
            cloud_notebook(
                "Test Notebook",
                r#"A command:
```
echo hello
```
"#,
            ),
        )
        .await;

        // First, make sure the editor is focused.
        notebook.update(&mut app, |notebook, ctx| {
            notebook.focus_input(ctx);
        });

        app.update(|ctx| {
            let input = &notebook.as_ref(ctx).input;
            let command = input
                .as_ref(ctx)
                .runnable_command_at(11.into(), ctx)
                .expect("Command should exist")
                .as_any()
                .downcast_ref::<NotebookCommand>()
                .expect("Should convert");

            // Use the command's own to_workflow implementation to use as much of the real code
            // path as possible.
            let workflow = command
                .to_workflow(ctx)
                .expect("Can't convert command to a workflow");

            ctx.dispatch_typed_action_for_view(
                window,
                input.id(),
                &EditorViewAction::RunWorkflow(workflow),
            );
        });

        app.read(|ctx| {
            let events = &root.as_ref(ctx).events;
            assert!(
                events.contains(&NotebookEvent::RunWorkflow {
                    workflow: Arc::new(WorkflowType::Notebook(Workflow::new(
                        "Command from Test Notebook",
                        "echo hello"
                    ))),
                    source: WorkflowSource::Notebook {
                        notebook_id: None,
                        team_uid: None,
                        location: NotebookLocation::PersonalCloud,
                    },
                }),
                "No RunWorkflow event in {events:#?}"
            );
        })
    });
}

#[test]
fn test_focus_tracking() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        initial_load(&mut app, []).await;

        let (window, notebook, root) = create_notebook(&mut app);
        open_notebook(
            &mut app,
            &notebook,
            cloud_notebook("Test Notebook", "This is a notebook"),
        )
        .await;

        let (title_view, input_view) = notebook.read(&app, |notebook, _| {
            (notebook.title.clone(), notebook.input.clone())
        });

        // Focus the title editor by selecting.
        app.update(|ctx| {
            ctx.dispatch_typed_action_for_view(
                window,
                title_view.id(),
                &EditorAction::Select(SelectAction::begin(DisplayPoint::new(0, 4))),
            );
        });
        app.read(|ctx| {
            assert_eq!(
                notebook.as_ref(ctx).last_focused_component,
                FocusedComponent::Title
            );

            let events = &root.as_ref(ctx).events;
            assert_eq!(
                events,
                &[
                    // This is from focusing the title editor.
                    NotebookEvent::Pane(PaneEvent::FocusSelf)
                ]
            );
        });

        // When blurring the notebook and restoring focus, focus should go to the title editor.
        root.update(&mut app, |_, ctx| ctx.focus_self());
        notebook.update(&mut app, |notebook, ctx| notebook.focus(ctx));
        assert_eq!(app.focused_view_id(window), Some(title_view.id()));
        app.read(|ctx| {
            let events = &root.as_ref(ctx).events;
            assert_eq!(
                events,
                &[
                    // This is the prior focus event.
                    NotebookEvent::Pane(PaneEvent::FocusSelf),
                    // This is from focusing the title editor again.
                    NotebookEvent::Pane(PaneEvent::FocusSelf)
                ]
            );
        });

        // Focus the input view, which should emit a focused event.
        input_view.update(&mut app, |view, ctx| view.focus(ctx));
        app.read(|ctx| {
            assert_eq!(
                notebook.as_ref(ctx).last_focused_component,
                FocusedComponent::Input
            );

            let events = &root.as_ref(ctx).events;
            assert_eq!(
                events,
                &[
                    // These are prior events.
                    NotebookEvent::Pane(PaneEvent::FocusSelf),
                    NotebookEvent::Pane(PaneEvent::FocusSelf),
                    // This is from focusing the input editor.
                    NotebookEvent::Pane(PaneEvent::FocusSelf),
                ]
            );
        });

        // Now, focus should be restored to the input editor.
        root.update(&mut app, |_, ctx| ctx.focus_self());
        notebook.update(&mut app, |notebook, ctx| notebook.focus(ctx));
        assert_eq!(app.focused_view_id(window), Some(input_view.id()));
    });
}

#[test]
#[ignore]
fn test_edit_telemetry() {
    fn edit_events() -> Vec<serde_json::Value> {
        warpui::telemetry::flush_events()
            .into_iter()
            .filter_map(|event| match event.payload {
                EventPayload::NamedEvent { name, value, .. } if name == "Notebook Edited" => value,
                _ => None,
            })
            .collect_vec()
    }

    App::test((), |mut app| async move {
        initialize_app(&mut app);
        initial_load(&mut app, []).await;

        let (_, notebook, _) = create_notebook(&mut app);
        open_notebook(
            &mut app,
            &notebook,
            cloud_notebook("Test Notebook", "This is a notebook"),
        )
        .await;
        let input_view = notebook.read(&app, |notebook, _| notebook.input.clone());

        // The notebook should show in edit mode, with telemetry recording.
        notebook.update(&mut app, |notebook, ctx| {
            notebook.grab_edit_access(ctx);
            assert_eq!(
                notebook.active_notebook_data.as_ref(ctx).mode,
                Mode::Editing
            );
            assert!(notebook.edit_telemetry_handle.is_some());
            notebook.focus_input(ctx);
        });

        // With no edits, there are no events.
        ensure_saved(&mut app, &notebook).await;
        Timer::after(2 * EDIT_WINDOW_DURATION).await;
        assert!(edit_events().is_empty());

        // Make a small edit, which should get reported as non-meaningful.
        input_view.update(&mut app, |input, ctx| {
            input.user_typed("Hi", ctx);
        });

        ensure_saved(&mut app, &notebook).await;
        Timer::after(2 * EDIT_WINDOW_DURATION).await;
        assert_eq!(
            edit_events(),
            vec![serde_json::json!({
                "notebook_id": None::<()>,
                "meaningful_change": false,
            })]
        );

        // If we switch to view mode, we stop recording elemetry.
        notebook.update(&mut app, |notebook, ctx| {
            notebook.switch_to_view(ctx);
            assert!(notebook.edit_telemetry_handle.is_none());
        });

        // Telemetry resumes when we switch to editing.
        notebook.update(&mut app, |notebook, ctx| {
            notebook.switch_to_edit(ctx);
            notebook.focus_input(ctx);
            assert!(notebook.edit_telemetry_handle.is_some());
        });

        // Finally, a meaningful edit is recorded as such.
        input_view.update(&mut app, |input, ctx| {
            input.user_typed(
                "This is a very very very very long edit. This is a heavy notebook user.",
                ctx,
            );
        });

        ensure_saved(&mut app, &notebook).await;
        Timer::after(2 * EDIT_WINDOW_DURATION).await;
        assert_eq!(
            edit_events(),
            vec![serde_json::json!({
                "notebook_id": None::<()>,
                "meaningful_change": true,
            })]
        );
    });
}

/// Opening a notebook stays in view mode: the eager baton grab waited on the
/// server initial load, which can no longer complete, so there is nothing to
/// wait for and nothing to grab. Entering edit mode is an explicit toggle.
#[test]
fn test_no_eager_baton_grab_without_initial_load() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        // Seed the cloud model the way the old initial-load helper did; it no
        // longer resolves any load gate.
        initial_load(&mut app, vec![]).await;

        let (_, notebook_view, _) = create_notebook(&mut app);
        let mut cloud_notebook = cloud_notebook("Test Notebook", r#"A notebook"#);

        // Set the current editor of the notebook to be the test notebook
        cloud_notebook.metadata.current_editor_uid = Some(TEST_USER_UID.to_string().clone());

        // Add the notebook to cloud model
        CloudModel::handle(&app).update(&mut app, |model, _| {
            model.add_object(cloud_notebook.id, cloud_notebook.clone())
        });

        // Open the notebook
        open_notebook(&mut app, &notebook_view, cloud_notebook).await;

        // The recorded editor is still reported as the current user ...
        notebook_view.update(&mut app, |notebook, ctx| {
            assert_eq!(
                notebook
                    .active_notebook_data
                    .as_ref(ctx)
                    .current_editor(ctx),
                Some(Editor {
                    state: EditorState::CurrentUser,
                    email: Some(TEST_USER_EMAIL.to_string())
                })
            )
        });

        // ... but opening does not enter edit mode on its own.
        let mode = notebook_view.read(&app, |notebook, ctx| notebook.mode(ctx));
        assert_eq!(mode, Mode::View);
    });
}

/// Test to make sure we do not eagerly enter edit mode when there is another editor
#[test]
fn test_not_eager_baton_grab_different_editor() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        // Seed the cloud model the way the old initial-load helper did.
        initial_load(&mut app, vec![]).await;

        let uid = "ian@warp.dev".to_string();
        let email = "ian@warp.dev".to_string();

        let (_, notebook_view, _) = create_notebook(&mut app);
        let mut cloud_notebook = cloud_notebook("Test Notebook", r#"A notebook"#);

        // Set the current editor of the notebook to be another email
        cloud_notebook.metadata.current_editor_uid = Some(uid.clone());
        UserProfiles::handle(&app).update(&mut app, |user_profiles, _| {
            user_profiles.insert_profiles(&vec![UserProfileWithUID {
                firebase_uid: UserUid::new(&uid),
                display_name: Some(email.clone()),
                email: email.clone(),
                photo_url: "".to_string(),
            }]);
        });

        // Add the notebook to cloud model
        CloudModel::handle(&app).update(&mut app, |model, _| {
            model.add_object(cloud_notebook.id, cloud_notebook.clone())
        });

        // Open the notebook
        open_notebook(&mut app, &notebook_view, cloud_notebook).await;

        // Assert that the editor is the other email
        notebook_view.update(&mut app, |notebook, ctx| {
            assert_eq!(
                notebook
                    .active_notebook_data
                    .as_ref(ctx)
                    .current_editor(ctx),
                Some(Editor {
                    state: EditorState::OtherUserActive,
                    email: Some(email)
                })
            )
        });

        let mode = notebook_view.read(&app, |notebook, ctx| notebook.mode(ctx));

        // Assert that we are in view mode open since there is another editor
        assert_eq!(mode, Mode::View);
    });
}

/// Test to make sure we do not eagerly enter edit mode when another editor took the baton
/// while Warp was closed.
#[test]
fn test_untitled_notebook() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let (_, notebook, _) = create_notebook(&mut app);

        notebook.update(&mut app, |notebook, ctx| {
            notebook.open_new_notebook(None, Owner::mock_current_user(), None, ctx);
        });

        notebook.read(&app, |notebook, ctx| {
            assert_eq!(notebook.title(ctx), "Untitled");
        });

        notebook.update(&mut app, |notebook, ctx| {
            notebook.switch_to_edit(ctx);
            notebook.focus_title(ctx);
            notebook.title.update(ctx, |title, ctx| {
                title.user_insert("My Notebook", ctx);
            });
            assert_eq!(notebook.title(ctx), "My Notebook");
        });
    });
}
