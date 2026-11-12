//! Per-thread CPU time distinguishes preemption from actual Session work.
//! Only instrumented feed probes or runtime-enabled input diagnostics read it.
//! Unsupported targets retain `None`, never a fabricated zero CPU sample.
//! CLOCK_THREAD_CPUTIME_ID includes this thread's user and kernel work, excluding
//! time it spends descheduled. Two reads bracket one owner operation without allocating.
use std::time::Duration;

/// Reads the calling thread only; timestamps must never move to another owner.
pub fn now() -> Option<Duration> {
    #[cfg(all(feature = "lock-metrics", unix))]
    {
        diagnostic_now()
    }
    #[cfg(not(all(feature = "lock-metrics", unix)))]
    {
        None
    }
}

/// Preserves unsupported clocks as absent and guards against an invalid reversed sample.
pub fn elapsed(started: Option<Duration>) -> Option<Duration> {
    now()?.checked_sub(started?)
}

#[cfg(all(test, feature = "lock-metrics", unix))]
/// Tests the essential distinction between executed work and sleeping wall time.
mod tests;

/// Runtime-enabled input diagnostics read only the calling thread CPU clock.
/// An unsupported platform returns None; wall time is never substituted for CPU.
pub(crate) fn diagnostic_now() -> Option<Duration> {
    #[cfg(unix)]
    {
        let time = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
        // Darwin's supported clock yields a nonnegative normalized timespec.
        // Checked conversion keeps diagnostic input from becoming a panic.
        let seconds = u64::try_from(time.tv_sec).ok()?;
        let nanos = u32::try_from(time.tv_nsec).ok()?;
        Some(Duration::new(seconds, nanos))
    }
    #[cfg(not(unix))]
    {
        None
    }
}
