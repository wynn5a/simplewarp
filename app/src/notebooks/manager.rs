use std::collections::HashMap;
use std::collections::hash_map::Entry;

use warpui::{Entity, EntityId, ModelContext, SingletonEntity, WeakViewHandle, WindowId};

use super::CloudNotebook;
use super::notebook::NotebookView;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{OpenWarpDriveObjectSettings, Owner};
use crate::pane_group::{NotebookPane, PaneContent};
use crate::server::ids::SyncId;
use crate::workspace::PaneViewLocator;
use crate::{safe_debug, safe_warn};

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;

/// A singleton model tracking open notebooks.
///
/// This is tightly tied to the [workspace](crate::workspace::Workspace) and
/// [pane group](crate::pane_group::PaneGroup) views, as they contain all open notebook panes.
///
/// The overall flow is:
/// 1. A `Workspace` is asked to open a notebook (from the Warp Drive index, universal search, etc.).
/// 2. It checks the `NotebookManager` to see if the notebook is already open.
/// 3. If it is, the existing notebook pane is focused (this may be in another window).
/// 4. If not, the `Workspace` uses the `NotebookManager` to create a new notebook pane and
///    attaches it to the active tab.
/// 5. When the new pane is attached to a pane group, it registers itself with the `NotebookManager`.
///    This is because we need the pane group's ID in order to re-focus the pane.
/// 6. When the pane is closed, it de-registers itself from the `NotebookManager`.
///
/// During session restoration, notebook panes are created and attached by the `PaneGroup`.
///
pub struct NotebookManager {
    panes_by_hashed_id: HashMap<String, NotebookPaneData>,
}

/// Source for a new notebook pane.
#[derive(Debug, Clone)]
pub enum NotebookSource {
    Existing(SyncId),
    New {
        title: Option<String>,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
    },
}

impl NotebookManager {
    /// Create a new [`NotebookManager`] singleton.
    pub fn new(_cached_notebooks: Vec<CloudNotebook>, _ctx: &mut ModelContext<Self>) -> Self {
        Self {
            panes_by_hashed_id: HashMap::new(),
        }
    }

    /// Create a mock [`NotebookManager`] for use in tests.
    #[cfg(test)]
    pub fn mock(ctx: &mut ModelContext<Self>) -> Self {
        Self::new(Vec::new(), ctx)
    }

    /// If the notebook is already open in a pane, finds the location of that pane.
    pub fn find_pane(&self, source: &NotebookSource) -> Option<(WindowId, PaneViewLocator)> {
        match source {
            NotebookSource::Existing(notebook_id) => {
                let pane_data = self.panes_by_hashed_id.get(&notebook_id.uid())?;
                Some((pane_data.window_id, pane_data.locator))
            }
            NotebookSource::New { .. } => None,
        }
    }

    /// Unconditionally create a new notebook pane.
    pub fn create_pane(
        &mut self,
        source: &NotebookSource,
        settings: &OpenWarpDriveObjectSettings,
        window_id: WindowId,
        ctx: &mut ModelContext<Self>,
    ) -> NotebookPane {
        let view = ctx.add_typed_action_view(window_id, NotebookView::new);

        match source {
            NotebookSource::Existing(notebook_id) => {
                let notebook = CloudModel::as_ref(ctx).get_notebook(notebook_id).cloned();
                if let Some(notebook) = notebook {
                    view.update(ctx, |view, ctx| view.load(notebook, settings, ctx));
                } else {
                    // If the notebook doesn't exist yet, try waiting for initial load and check again
                    view.update(ctx, |view, ctx| {
                        view.wait_for_initial_load_then_load(*notebook_id, settings, window_id, ctx)
                    });
                }
            }
            NotebookSource::New {
                title,
                owner,
                initial_folder_id,
            } => view.update(ctx, |view, ctx| {
                view.open_new_notebook(title.clone(), *owner, *initial_folder_id, ctx);
            }),
        }

        NotebookPane::new(view, ctx)
    }

    /// Register an open notebook pane once it's bound to a pane group.
    pub fn register_pane(
        &mut self,
        pane: &NotebookPane,
        pane_group_id: EntityId,
        window_id: WindowId,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(notebook_id) = pane.notebook_view(ctx).as_ref(ctx).notebook_id(ctx) else {
            log::warn!("Notebook pane has no notebook ID");
            return;
        };

        let entry = self.panes_by_hashed_id.entry(notebook_id.uid());
        if let Entry::Vacant(entry) = entry {
            entry.insert(NotebookPaneData {
                notebook_id,
                window_id,
                locator: PaneViewLocator {
                    pane_group_id,
                    pane_id: pane.id(),
                },
                handle: pane.notebook_view(ctx).downgrade(),
            });
        } else {
            safe_warn!(
                safe: ("Ignoring duplicate notebook pane registration"),
                full: ("Ignoring duplicate notebook pane registration for {notebook_id}")
            );
        }
    }

    // De-register an open notebook pane when it's removed from a pane group.
    pub fn deregister_pane(&mut self, pane: &NotebookPane, ctx: &mut ModelContext<Self>) {
        let Some(notebook_id) = pane.notebook_view(ctx).as_ref(ctx).notebook_id(ctx) else {
            log::warn!("Notebook pane has no notebook ID");
            return;
        };

        // If a notebook pane is restored, the notebook may have been reopened in the meantime. In
        // that case, don't let closing the original pane clear out the new pane.
        if let Entry::Occupied(entry) = self.panes_by_hashed_id.entry(notebook_id.uid()) {
            if entry.get().locator.pane_id == pane.id() {
                entry.remove();
            } else {
                log::warn!(
                    "Ignoring duplicate registration of panes for {}",
                    notebook_id.uid()
                );
            }
        }
    }

    /// Swap the ID of the notebook open in a pane. This assumes the pane location and view are
    /// unchanged.
    pub(super) fn swap_notebook(&mut self, old_id: SyncId, new_id: SyncId) {
        if let Some(mut pane_data) = self.panes_by_hashed_id.remove(&old_id.uid()) {
            debug_assert_eq!(pane_data.notebook_id, old_id);
            pane_data.notebook_id = new_id;
            debug_assert!(
                self.panes_by_hashed_id
                    .insert(new_id.uid(), pane_data)
                    .is_none(),
                "New notebook was already open"
            );
        } else {
            log::warn!("Tried to swap notebooks, but the old one was not open");
        }
    }

    /// Close all open notebooks, saving any changes. This is called before the app terminates to
    /// prevent data loss, since notebooks are not saved immediately after every user edit.
    pub fn close_notebooks(&self, ctx: &mut ModelContext<Self>) {
        for pane in self.panes_by_hashed_id.values() {
            if let Some(notebook_view) = pane.handle.upgrade(ctx) {
                safe_debug!(
                    safe : ("Closing notebook on termination"),
                    full: ("Closing notebook {} on termination", pane.notebook_id)
                );
                notebook_view.update(ctx, |view, ctx| view.on_detach(ctx));
            }
        }
    }
}

struct NotebookPaneData {
    notebook_id: SyncId,
    window_id: WindowId,
    handle: WeakViewHandle<NotebookView>,
    locator: PaneViewLocator,
}

impl Entity for NotebookManager {
    type Event = ();
}

impl SingletonEntity for NotebookManager {}
