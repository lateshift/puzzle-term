//! Diagnostic clocks around the actor's existing blocking receive.
//!
//! The wait policy and deadline are unchanged. Paired wall/CPU timestamps
//! separate sleep and wake delivery from work performed after the worker wakes.
use std::{sync::mpsc, time::Instant};

/// One completed worker wait; copied values never schedule a frame.
#[cfg(feature = "lock-metrics")]
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// Wait entry and return use the same process monotonic clock as frames.
    pub bounds: [Instant; 2],
    /// The actual actor deadline can be earlier than a surface snapshot deadline.
    pub deadline: Option<Instant>,
    /// Executed wait bookkeeping excludes time sleeping or descheduled.
    pub cpu: Option<std::time::Duration>,
    /// A mailbox notification differs from expiration of the requested timeout.
    pub reason: &'static str,
}

/// Reports disconnection without introducing another wake or a recurring timer.
pub(super) struct Outcome {
    /// A closed actor inbox ends the loop when it would otherwise sleep forever.
    pub(super) connected: bool,
    /// Instrumented builds retain scalar evidence; normal builds keep no sample.
    #[cfg(feature = "lock-metrics")]
    pub(super) sample: Sample,
}

/// Sleeps on the same inbox and the same absolute deadline as the actor loop.
pub(super) fn receive(wakes: &mpsc::Receiver<()>, due: Option<Instant>) -> Outcome {
    #[cfg(feature = "lock-metrics")]
    let cpu = crate::thread_cpu::now();
    let started = Instant::now();
    // Untimed idle waits remain untimed; no polling or scheduler hint is added.
    let result = match due {
        Some(due) => wakes.recv_timeout(due.saturating_duration_since(started)),
        None => wakes
            .recv()
            .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
    };
    Outcome {
        // Preserve the actor's original disconnect handling: a timed wait
        // re-enters lifecycle draining; an untimed closed inbox ends the loop.
        connected: due.is_some() || result != Err(mpsc::RecvTimeoutError::Disconnected),
        #[cfg(feature = "lock-metrics")]
        sample: Sample {
            bounds: [started, Instant::now()],
            deadline: due,
            cpu: crate::thread_cpu::elapsed(cpu),
            reason: match result {
                Ok(()) => "message",
                Err(mpsc::RecvTimeoutError::Timeout) => "timeout",
                Err(mpsc::RecvTimeoutError::Disconnected) => "disconnected",
            },
        },
    }
}
