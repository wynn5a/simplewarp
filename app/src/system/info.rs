use std::ffi::OsStr;

use sysinfo::ProcessesToUpdate;
use warpui::{Entity, SingletonEntity};

#[derive(Default)]
pub struct SystemInfo {
    /// A structure we can use to efficiently query system information.
    system: sysinfo::System,
}

impl SystemInfo {
    /// Returns the [`sysinfo::ProcessRefreshKind`] that should be used when enumerating the entire
    /// process table.
    ///
    /// This samples neither CPU nor memory: on Windows each per-process CPU sample issues an
    /// `NtQueryInformationProcess(ProcessCycleTime)` call, which forces a
    /// `KeFlushProcessWriteBuffers` inter-processor interrupt across every logical core. Across the
    /// whole process table that can pin all cores at `DISPATCH_LEVEL` long enough to trip the DPC
    /// watchdog and bugcheck high-core-count machines.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn all_processes_refresh_kind() -> sysinfo::ProcessRefreshKind {
        sysinfo::ProcessRefreshKind::nothing()
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn refresh_all_processes(&mut self) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true, /* remove_dead_processes */
            Self::all_processes_refresh_kind(),
        );
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn processes_by_name<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a sysinfo::Process> {
        self.system.processes_by_name(OsStr::new(name))
    }
}

impl Entity for SystemInfo {
    type Event = ();
}

impl SingletonEntity for SystemInfo {}
