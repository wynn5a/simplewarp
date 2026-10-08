use warpui::integration::AssertionWithDataCallback;
use warpui::{App, async_assert_eq};

use crate::integration_testing::view_getters::workflow_view;
use crate::server::ids::SyncId;
use crate::workflows::workflow_view::WorkflowView;

/// Asserts that a pane has the given workflow open.
pub fn assert_workflow_id(
    tab_index: usize,
    pane_index: usize,
    expected_id_key: impl Into<String>,
) -> AssertionWithDataCallback {
    let expected_id_key = expected_id_key.into();
    Box::new(move |app, window_id, data| {
        let expected_id = data.get(&expected_id_key).expect("No saved workflow ID");

        let workflow = workflow_view(app, window_id, tab_index, pane_index);
        workflow.read(app, |workflow, _ctx| {
            let id = workflow.workflow_id();
            async_assert_eq!(
                id, *expected_id,
                "Expected window_id={window_id}, tab_index={tab_index}, pane_index={pane_index} to contain {expected_id:?}, but got {id:?}")
        })
    })
}

/// Find number of workflows that are open by id
pub fn open_workflow_count(app: &App, id: SyncId) -> usize {
    app.window_ids()
        .into_iter()
        .flat_map(|window_id| app.views_of_type::<WorkflowView>(window_id))
        .flatten()
        .filter(move |view| view.read(app, |view, _ctx| view.workflow_id()) == id)
        .count()
}
