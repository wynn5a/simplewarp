use warpui::WindowId;

use super::{QuitScope, UnsavedStateSummary};

fn window_summary(tabs: usize, long_running_commands: usize) -> UnsavedStateSummary<'static> {
    UnsavedStateSummary {
        scope: QuitScope::Window(WindowId::new()),
        total_long_running_commands: long_running_commands,
        windows_with_long_running_commands: usize::from(long_running_commands > 0),
        tabs_with_long_running_commands: usize::from(long_running_commands > 0),
        terminal_sessions: Vec::new(),
        unsaved_code_changes: false,
        tabs_closing_with_window: tabs,
    }
}

#[test]
fn single_tab_window_does_not_count_as_closing_multiple_tabs() {
    let summary = window_summary(1, 0);
    assert!(!summary.closes_multiple_tabs());
    assert_eq!(summary.warning_text(), "");
}

#[test]
fn multi_tab_window_warns_with_the_tab_count() {
    let summary = window_summary(3, 0);
    assert!(summary.closes_multiple_tabs());
    assert_eq!(summary.warning_text(), "This window has 3 tabs open.");
}

#[test]
fn multi_tab_warning_precedes_running_processes() {
    assert_eq!(
        window_summary(2, 1).warning_text(),
        "This window has 2 tabs open.\nYou have 1 process running in this window."
    );
}
