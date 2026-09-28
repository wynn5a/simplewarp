use std::sync::Arc;

use warp_core::ui::appearance::Appearance;
use warp_editor::editor::EditorView;
use warpui::platform::WindowStyle;
use warpui::presenter::ChildView;
use warpui::{
    AddSingletonModel, App, AppContext, Element, Entity, SingletonEntity, TypedActionView, View,
    ViewHandle, WindowId,
};

use super::{NotebookEvent, NotebookView};
use crate::auth::AuthStateProvider;
use crate::auth::auth_manager::AuthManager;
use crate::cloud_object::Owner;
use crate::cloud_object::model::actions::ObjectActions;
use crate::cloud_object::model::persistence::CloudModel;
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
use crate::server::server_api::ServerApiProvider;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::terminal::keys::TerminalKeybindings;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workflows::workflow::Workflow;
use crate::workflows::{WorkflowSource, WorkflowType};
use crate::workspace::ActiveSession;
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
    app.add_singleton_model(|_| UpdateManager::mock());
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| ActiveSession::default());
    app.add_singleton_model(|_| ObjectActions::new(Vec::new()));
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
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
        view.load(notebook, ctx);
    });
    // Command block models are built on LayoutUpdated, which is emitted by the render
    // model after its layout actions round-trip through a background thread, so wait
    // for layout completion instead of pumping the executor a fixed number of times.
    let render_state = handle.read(app, |view, ctx| {
        view.input
            .as_ref(ctx)
            .model()
            .as_ref(ctx)
            .render_state()
            .clone()
    });
    app.read(|ctx| render_state.as_ref(ctx).layout_complete())
        .await;
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

/// Test that command-block execution events are correctly translated into workflows.
#[test]
fn test_command_block_dispatches_event() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

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
/// Opening a notebook stays in view mode; entering edit mode is an explicit toggle.
#[test]
fn test_open_notebook_starts_in_view_mode() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let (_, notebook_view, _) = create_notebook(&mut app);
        let cloud_notebook = cloud_notebook("Test Notebook", r#"A notebook"#);

        CloudModel::handle(&app).update(&mut app, |model, _| {
            model.add_object(cloud_notebook.id, cloud_notebook.clone())
        });

        open_notebook(&mut app, &notebook_view, cloud_notebook).await;

        let mode = notebook_view.read(&app, |notebook, ctx| notebook.mode(ctx));
        assert_eq!(mode, Mode::View);
    });
}

/// A stale editor recorded in upstream metadata does not block entering edit mode.
#[test]
fn test_stale_recorded_editor_does_not_block_editing() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        let (_, notebook_view, _) = create_notebook(&mut app);
        let mut cloud_notebook = cloud_notebook("Test Notebook", r#"A notebook"#);
        cloud_notebook.metadata.current_editor_uid = Some("ian@warp.dev".to_string());

        CloudModel::handle(&app).update(&mut app, |model, _| {
            model.add_object(cloud_notebook.id, cloud_notebook.clone())
        });

        open_notebook(&mut app, &notebook_view, cloud_notebook).await;
        assert_eq!(
            notebook_view.read(&app, |notebook, ctx| notebook.mode(ctx)),
            Mode::View
        );

        notebook_view.update(&mut app, |notebook, ctx| notebook.toggle_mode(ctx));
        assert_eq!(
            notebook_view.read(&app, |notebook, ctx| notebook.mode(ctx)),
            Mode::Editing
        );
    });
}

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
