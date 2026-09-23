use std::ffi::OsStr;

use byte_unit::Byte;
use sysinfo::ProcessesToUpdate;
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::system::memory_footprint;

/// The threshold at which we emit a memory usage warning, in bytes.
const MEMORY_USAGE_WARNING_THRESHOLD_BYTES: u64 = Byte::GIGABYTE.as_u64() * 10;

/// The refresh interval for system information, in seconds.
const REFRESH_INTERVAL_S: usize = 5;
/// The refresh interval for system information.
const REFRESH_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(REFRESH_INTERVAL_S as u64);

pub enum SystemInfoEvent {
    /// There is new system info available for consumers to query.
    Refreshed,
    /// The application is using a large quantity of memory.
    MemoryUsageHigh,
}

pub struct SystemInfo {
    /// A structure we can use to efficiently query system information.
    system: sysinfo::System,
    /// Whether or not we've already emitted an event due to high memory usage.
    has_emitted_memory_warning_event: bool,
    /// Set to the memory footprint that crossed `MEMORY_USAGE_WARNING_THRESHOLD_BYTES` on the
    /// previous poll tick, while we wait for the next tick to confirm the spike is sustained rather
    /// than a transient blip.  `None` when there is no pending confirmation.
    pending_excessive_memory_footprint_bytes: Option<u64>,
}

impl SystemInfo {
    /// Creates a new [`SystemInfo`] model and begins periodic fetching of
    /// system information.
    ///
    /// Currently only retrieves and exposes memory usage information for the
    /// current process.
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let mut me = Self {
            system: sysinfo::System::new(),
            has_emitted_memory_warning_event: false,
            pending_excessive_memory_footprint_bytes: None,
        };

        // Initialize the underlying system info.  This is necessary in order
        // for our first read of CPU stats to be accurate, as they are computed
        // as a delta between the previous refresh and now.
        me.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Self::current_pid()]),
            false, /* refresh_dead_processes */
            Self::refresh_kind(),
        );

        // If we're doing automated heap usage tracking, set up periodic
        // refreshes of the memory usage data.
        Self::schedule_refresh(ctx);

        me
    }

    /// Returns the amount of memory being used by the current process, in
    /// bytes.
    pub fn used_memory(&self) -> Byte {
        self.system
            .process(Self::current_pid())
            .expect("current process should exist")
            .memory()
            .into()
    }

    /// Returns the full memory footprint of the current process, in bytes.
    ///
    /// Unlike [`used_memory`] (RSS), this includes memory that has been
    /// swapped out or compressed by the OS.  On macOS this matches the value
    /// shown by Activity Monitor.
    pub fn memory_footprint(&self) -> Byte {
        memory_footprint::memory_footprint_bytes().into()
    }

    fn schedule_refresh(ctx: &mut ModelContext<Self>) {
        ctx.spawn(
            async {
                warpui::r#async::Timer::after(REFRESH_INTERVAL).await;
            },
            |me, _, ctx| {
                me.refresh(ctx);
                Self::schedule_refresh(ctx);
            },
        );
    }

    fn refresh(&mut self, ctx: &mut ModelContext<Self>) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Self::current_pid()]),
            false, /* refresh_dead_processes */
            Self::refresh_kind(),
        );
        ctx.emit(SystemInfoEvent::Refreshed);

        let rss = self.used_memory();
        let footprint = self.memory_footprint();
        self.check_for_excessive_memory_usage(rss, footprint, ctx);
    }

    /// Checks for excessive memory usage.  This may send a telemetry event
    /// and trigger a Sentry heap profile dump if excessive usage is detected.
    ///
    /// The threshold check uses `memory_footprint` (which includes swapped
    /// and compressed pages) so we actually detect high memory situations.
    /// The Rudderstack telemetry event still reports `rss` so existing
    /// dashboards are unaffected.
    ///
    /// A crossing of the threshold is only reported once it's confirmed still excessive on the next
    /// poll tick, rather than on the tick that first observed it, so a short-lived spike that's
    /// freed moments later is skipped instead of producing a worthless Sentry event and heap
    /// profile.  A skip does not consume `has_emitted_memory_warning_event`, so an early transient
    /// spike doesn't silence the process for the rest of its lifetime.
    fn check_for_excessive_memory_usage(
        &mut self,
        rss: Byte,
        memory_footprint: Byte,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.has_emitted_memory_warning_event {
            return;
        }

        // Use footprint (not RSS) for the threshold so we catch memory
        // that has been swapped out or compressed by the OS.
        let footprint_bytes = memory_footprint.as_u64();
        let is_excessive = footprint_bytes >= MEMORY_USAGE_WARNING_THRESHOLD_BYTES;

        let Some(triggering_footprint_bytes) = self.pending_excessive_memory_footprint_bytes else {
            self.pending_excessive_memory_footprint_bytes = is_excessive.then_some(footprint_bytes);
            return;
        };
        self.pending_excessive_memory_footprint_bytes = None;

        if !is_excessive {
            log::info!(
                "Memory footprint returned to {footprint_bytes} bytes, back under the \
                 excessive-usage threshold, before confirming a spike that had crossed it at \
                 {triggering_footprint_bytes} bytes; skipping the excessive-memory-usage report \
                 for what looks like a transient spike."
            );
            return;
        }

        // Collect a detailed memory breakdown for diagnostics.
        let _memory_breakdown = memory_footprint::memory_breakdown();

        // If we're tracking heap usage and detect excessive memory usage,
        // dump and upload the current heap profiling data.
        #[cfg(feature = "heap_usage_tracking")]
        {
            let breakdown_for_sentry = memory_breakdown.clone();
            ctx.spawn(
                crate::profiling::dump_jemalloc_heap_profile(breakdown_for_sentry),
                |_, _, _| {},
            );
        }

        // Send a telemetry event indicating that memory usage is extreme.
        // Report RSS here to keep Rudderstack dashboards consistent.
        let _total_application_usage_bytes = rss.as_u64();

        ctx.emit(SystemInfoEvent::MemoryUsageHigh);
        self.has_emitted_memory_warning_event = true;
    }

    /// Returns the pid of the current process.
    fn current_pid() -> sysinfo::Pid {
        sysinfo::get_current_pid().expect("Platform should support process IDs")
    }

    /// Returns the [`sysinfo::ProcessRefreshKind`] that should be used when
    /// retrieving information about the current process.
    fn refresh_kind() -> sysinfo::ProcessRefreshKind {
        sysinfo::ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
    }

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
    type Event = SystemInfoEvent;
}

impl SingletonEntity for SystemInfo {}
