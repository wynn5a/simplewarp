use std::collections::BTreeMap;
use std::fmt;

use super::protocol::{
    BlockedKind, MAX_RECORDS, ProgramState, ProgramStatusReport, ProgramStatusUpdate, RecordPath,
};

/// Where a report came from. `Osc94` reports are the ConEmu progress bridge and are ignored once
/// the terminal has produced a real `Osc7501` report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportSource {
    Osc7501,
    Osc94,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProgramStatusRecord {
    pub state: ProgramState,
    pub kind: Option<BlockedKind>,
    pub progress: Option<u8>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
}

impl From<ProgramStatusUpdate> for ProgramStatusRecord {
    fn from(update: ProgramStatusUpdate) -> Self {
        let ProgramStatusUpdate {
            id: _,
            state,
            kind,
            progress,
            app,
            title,
            message,
        } = update;
        Self {
            state,
            kind,
            progress,
            app,
            title,
            message,
        }
    }
}

// `title` and `message` are untrusted text from any process on the PTY and must not reach logs.
impl fmt::Debug for ProgramStatusRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgramStatusRecord")
            .field("state", &self.state)
            .field("kind", &self.kind)
            .field("progress", &self.progress)
            .finish_non_exhaustive()
    }
}

/// The records reported by the programs running in one terminal.
#[derive(Debug, Default)]
pub struct ProgramStatusStore {
    records: BTreeMap<RecordPath, ProgramStatusRecord>,
    saw_program_status: bool,
}

impl ProgramStatusStore {
    /// The root record, which is what tab and pane indicators show.
    pub fn root(&self) -> Option<&ProgramStatusRecord> {
        self.records.get(&RecordPath::root())
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Applies a report and returns whether any record changed.
    pub fn apply(&mut self, report: ProgramStatusReport, source: ReportSource) -> bool {
        match source {
            ReportSource::Osc7501 => self.saw_program_status = true,
            ReportSource::Osc94 if self.saw_program_status => return false,
            ReportSource::Osc94 => (),
        }
        match report {
            ProgramStatusReport::Set(update) => {
                let id = update.id.clone();
                let record = ProgramStatusRecord::from(update);
                if self.records.get(&id) == Some(&record) {
                    return false;
                }
                if !self.records.contains_key(&id) && self.records.len() >= MAX_RECORDS {
                    return false;
                }
                self.records.insert(id, record);
                true
            }
            ProgramStatusReport::Clear { id } => {
                let before = self.records.len();
                self.records.retain(|path, _| !path.is_within(&id));
                self.records.len() != before
            }
        }
    }

    /// Drops the records that only make sense while a process is running.
    pub fn drop_running(&mut self) -> bool {
        self.drop_where(|state| matches!(state, ProgramState::Working | ProgramState::Blocked))
    }

    /// Drops the records that stay visible until the user interacts with the terminal.
    pub fn drop_finished(&mut self) -> bool {
        self.drop_where(|state| matches!(state, ProgramState::Done | ProgramState::Error))
    }

    /// Full terminal reset.
    pub fn reset(&mut self) -> bool {
        let changed = !self.records.is_empty();
        *self = Self::default();
        changed
    }

    fn drop_where(&mut self, should_drop: impl Fn(ProgramState) -> bool) -> bool {
        let before = self.records.len();
        self.records.retain(|_, record| !should_drop(record.state));
        self.records.len() != before
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
