use warpui_core::{Entity, SingletonEntity};

/// Logs timing milestones for a multiple-stage process, one stage at a time.
pub struct IntervalTimer;

impl IntervalTimer {
    pub fn new() -> Self {
        Self
    }

    pub fn mark_interval_end(&mut self, name: impl Into<String>) {
        let name = name.into();
        tracing::info!(name);
    }
}

impl Default for IntervalTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for IntervalTimer {
    type Event = ();
}

impl SingletonEntity for IntervalTimer {}
