use warpui::{App, EntityId, WindowId};

use super::*;

fn setup_model(app: &mut App) -> ModelHandle<ActiveAgentViewsModel> {
    app.add_singleton_model(|_| ActiveAgentViewsModel::new())
}

fn focused_terminal(model: &ActiveAgentViewsModel, window_id: WindowId) -> Option<EntityId> {
    model
        .focused_terminal_states
        .get(&window_id)
        .map(|state| state.focused_terminal_id)
}

#[test]
fn per_window_focused_state_is_independent() {
    App::test((), |mut app| async move {
        let model = setup_model(&mut app);
        let window_a = WindowId::new();
        let window_b = WindowId::new();
        let terminal_a = EntityId::new();
        let terminal_b = EntityId::new();

        model.update(&mut app, |model, ctx| {
            model.handle_pane_focus_change(window_a, Some(terminal_a), ctx);
            model.handle_pane_focus_change(window_b, Some(terminal_b), ctx);
        });

        model.read(&app, |model, _| {
            assert_eq!(focused_terminal(model, window_a), Some(terminal_a));
            assert_eq!(focused_terminal(model, window_b), Some(terminal_b));
        });
    });
}

#[test]
fn clearing_one_window_does_not_affect_other() {
    App::test((), |mut app| async move {
        let model = setup_model(&mut app);
        let window_a = WindowId::new();
        let window_b = WindowId::new();
        let terminal_a = EntityId::new();
        let terminal_b = EntityId::new();

        model.update(&mut app, |model, ctx| {
            model.handle_pane_focus_change(window_a, Some(terminal_a), ctx);
            model.handle_pane_focus_change(window_b, Some(terminal_b), ctx);
        });

        // Clear window A's focus by passing None for terminal_view_id.
        model.update(&mut app, |model, ctx| {
            model.handle_pane_focus_change(window_a, None, ctx);
        });

        model.read(&app, |model, _| {
            assert_eq!(focused_terminal(model, window_a), None);
            assert_eq!(focused_terminal(model, window_b), Some(terminal_b));
        });
    });
}

#[test]
fn unknown_window_returns_none() {
    App::test((), |mut app| async move {
        let model = setup_model(&mut app);
        let window_a = WindowId::new();
        let unknown_window = WindowId::new();
        let terminal = EntityId::new();

        model.update(&mut app, |model, ctx| {
            model.handle_pane_focus_change(window_a, Some(terminal), ctx);
        });

        model.read(&app, |model, _| {
            assert_eq!(model.get_focused_conversation(unknown_window), None);
            assert_eq!(focused_terminal(model, unknown_window), None);
        });
    });
}

#[test]
fn focus_change_without_agent_view_has_no_conversation() {
    App::test((), |mut app| async move {
        let model = setup_model(&mut app);
        let window = WindowId::new();
        let terminal = EntityId::new();

        // No agent view handles registered → active_conversation_id should be None.
        model.update(&mut app, |model, ctx| {
            model.handle_pane_focus_change(window, Some(terminal), ctx);
        });

        model.read(&app, |model, _| {
            assert_eq!(model.get_focused_conversation(window), None);
        });
    });
}

#[test]
fn remove_focused_state_for_window_cleans_up() {
    App::test((), |mut app| async move {
        let model = setup_model(&mut app);
        let window_a = WindowId::new();
        let window_b = WindowId::new();
        let terminal_a = EntityId::new();
        let terminal_b = EntityId::new();

        model.update(&mut app, |model, ctx| {
            model.handle_pane_focus_change(window_a, Some(terminal_a), ctx);
            model.handle_pane_focus_change(window_b, Some(terminal_b), ctx);
        });

        // Remove window A's state (simulating undo-close expiry).
        model.update(&mut app, |model, ctx| {
            model.remove_focused_state_for_window(window_a, ctx);
        });

        model.read(&app, |model, _| {
            assert_eq!(focused_terminal(model, window_a), None);
            assert_eq!(focused_terminal(model, window_b), Some(terminal_b));
        });

        // Removing again is a no-op.
        model.update(&mut app, |model, ctx| {
            model.remove_focused_state_for_window(window_a, ctx);
        });
    });
}

#[test]
fn overwriting_same_window_updates_state() {
    App::test((), |mut app| async move {
        let model = setup_model(&mut app);
        let window = WindowId::new();
        let terminal_1 = EntityId::new();
        let terminal_2 = EntityId::new();

        model.update(&mut app, |model, ctx| {
            model.handle_pane_focus_change(window, Some(terminal_1), ctx);
        });
        model.read(&app, |model, _| {
            assert_eq!(focused_terminal(model, window), Some(terminal_1));
        });

        model.update(&mut app, |model, ctx| {
            model.handle_pane_focus_change(window, Some(terminal_2), ctx);
        });
        model.read(&app, |model, _| {
            assert_eq!(focused_terminal(model, window), Some(terminal_2));
        });
    });
}
