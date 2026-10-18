use warpui::async_assert_eq;
use warpui::integration::{AssertionCallback, AssertionOutcome};

use crate::integration_testing::view_getters::pane_group_view;

pub fn assert_focused_pane_index(tab_index: usize, pane_index: usize) -> AssertionCallback {
    Box::new(move |app, window_id| {
        let pane_group = pane_group_view(app, window_id, tab_index);
        pane_group.read(app, |view, ctx| {
            let Some(pane_id) = view.pane_id_from_index(pane_index) else {
                return AssertionOutcome::failure(format!("no pane at pane_index {pane_index}"));
            };
            async_assert_eq!(view.focused_pane_id(ctx), pane_id)
        })
    })
}
