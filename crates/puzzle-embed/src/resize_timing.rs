//! Paired wall/thread-CPU witnesses for resize stages, without changing policy.
//! A trace-disabled run does not read either clock or allocate observer storage.
use std::time::Instant;

/// One owner's stage guard; CPU timestamps never cross threads.
pub struct Stage {
    /// Closed diagnostic stage catalogue supplied by the calling operation.
    name: &'static str,
    /// Absent when the trace target is disabled.
    started: Option<Instant>,
    /// Calling thread's CPU clock at entry; unsupported clocks remain absent.
    cpu: Option<std::time::Duration>,
}
impl Stage {
    /// Starts an explicitly enabled observation before the stage does any work.
    pub fn new(name: &'static str) -> Self {
        // Dynamic filtering joins the static trace gate; ordinary windows pay no clock cost.
        let enabled = tracing::enabled!(target:"puzzle_desktop::resize", tracing::Level::TRACE);
        Self {
            name,
            started: enabled.then(Instant::now),
            // Read on this thread, so descheduling can be distinguished from actual work.
            cpu: if enabled {
                crate::thread_cpu::now()
            } else {
                None
            },
        }
    }
}
impl Drop for Stage {
    /// Log after the operation, retaining wall preemption separately from CPU work.
    fn drop(&mut self) {
        if let Some(started) = self.started {
            // Wall time includes OS waits; thread CPU excludes time spent descheduled.
            let wall_ms = started.elapsed().as_secs_f64() * 1000.;
            let cpu_ms = crate::thread_cpu::elapsed(self.cpu).map(|n| n.as_secs_f64() * 1000.);
            // Unsupported CPU clocks remain None rather than inventing a wall-based estimate.
            tracing::trace!(target:"puzzle_desktop::resize", stage=self.name, wall_ms, ?cpu_ms);
        }
    }
}
