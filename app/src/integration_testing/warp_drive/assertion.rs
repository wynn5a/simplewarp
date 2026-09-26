use warpui::async_assert;
use warpui::integration::AssertionCallback;

use crate::integration_testing::view_getters::workspace_view;

pub fn assert_is_left_panel_open() -> AssertionCallback {
    Box::new(move |app, window_id| {
        let workspace = workspace_view(app, window_id);

        workspace.read(app, |workspace, ctx| {
            async_assert!(
                workspace.is_left_panel_open(ctx),
                "Expected left panel to be open, but it was closed"
            )
        })
    })
}
