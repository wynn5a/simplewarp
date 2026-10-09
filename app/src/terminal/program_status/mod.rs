//! Program status reported by programs through the OSC 7501 escape sequence.

pub mod protocol;
mod store;

use std::collections::HashMap;

use warpui::{Entity, EntityId, ModelContext, SingletonEntity};

pub use self::protocol::{BlockedKind, ProgramState, ProgramStatusReport};
use self::store::ProgramStatusStore;
pub use self::store::{ProgramStatusRecord, ReportSource};

pub enum ProgramStatusModelEvent {
    Changed { terminal_view_id: EntityId },
}

/// Tracks the program status records of every terminal view. All mutations happen on the UI
/// thread, driven by events from the terminal model.
#[derive(Default)]
pub struct ProgramStatusModel {
    stores: HashMap<EntityId, ProgramStatusStore>,
}

impl Entity for ProgramStatusModel {
    type Event = ProgramStatusModelEvent;
}

impl SingletonEntity for ProgramStatusModel {}

impl ProgramStatusModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// The root record of the terminal, which is what tab and pane indicators show.
    pub fn status(&self, terminal_view_id: EntityId) -> Option<&ProgramStatusRecord> {
        self.stores.get(&terminal_view_id)?.root()
    }

    pub fn apply_report(
        &mut self,
        terminal_view_id: EntityId,
        report: ProgramStatusReport,
        source: ReportSource,
        ctx: &mut ModelContext<Self>,
    ) {
        let store = self.stores.entry(terminal_view_id).or_default();
        let changed = store.apply(report, source);
        Self::finish_update(terminal_view_id, changed, ctx);
    }

    /// Drops `working`/`blocked` records: the process that reported them is gone.
    pub fn drop_running(&mut self, terminal_view_id: EntityId, ctx: &mut ModelContext<Self>) {
        self.update_existing(terminal_view_id, ProgramStatusStore::drop_running, ctx);
    }

    /// Drops `done`/`error` records, which stay visible until the user interacts.
    pub fn drop_finished(&mut self, terminal_view_id: EntityId, ctx: &mut ModelContext<Self>) {
        self.update_existing(terminal_view_id, ProgramStatusStore::drop_finished, ctx);
    }

    pub fn reset(&mut self, terminal_view_id: EntityId, ctx: &mut ModelContext<Self>) {
        self.update_existing(terminal_view_id, ProgramStatusStore::reset, ctx);
    }

    pub fn remove_terminal(&mut self, terminal_view_id: EntityId, ctx: &mut ModelContext<Self>) {
        if self.stores.remove(&terminal_view_id).is_some() {
            ctx.emit(ProgramStatusModelEvent::Changed { terminal_view_id });
        }
    }

    fn update_existing(
        &mut self,
        terminal_view_id: EntityId,
        update: impl FnOnce(&mut ProgramStatusStore) -> bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let changed = self.stores.get_mut(&terminal_view_id).is_some_and(update);
        Self::finish_update(terminal_view_id, changed, ctx);
    }

    fn finish_update(terminal_view_id: EntityId, changed: bool, ctx: &mut ModelContext<Self>) {
        if changed {
            ctx.emit(ProgramStatusModelEvent::Changed { terminal_view_id });
        }
    }
}
