//! First-event PTY throttle shared by normal expiry and final fixture flushing.
//!
//! A slow frame can cross the throttle deadline in the same turn that Close was
//! received. Keeping the pending notification here lets both normal expiry and
//! final flushing consume it exactly once; finalization cannot send it again.
use std::time::{Duration, Instant};

/// Latest host size and its first-event deadline, independent of grid reflow.
#[derive(Default)]
pub(super) struct QuietResize {
    /// The geometry travels with its deadline, even if no frame applied it yet.
    pending: Option<([u32; 2], Instant)>,
}

impl QuietResize {
    /// Replace only the size after the first event arms the D-29 timer.
    pub(super) fn update(&mut self, size: [u32; 2], now: Instant) {
        // The clock belongs to the first event; a continuous drag cannot delay it.
        let due = self
            .pending
            .map_or(now + Duration::from_millis(25), |(_, due)| due);
        self.pending = Some((size, due));
    }

    /// Consumes a due notification, or flushes it once when the controller closes.
    pub(super) fn take(&mut self, now: Instant, closing: bool) -> Option<[u32; 2]> {
        // Both callers share this ownership transfer. An expired notification
        // leaves no pending value for finalization to duplicate after a slow frame.
        if self
            .pending
            .is_some_and(|(_, deadline)| closing || now >= deadline)
        {
            self.pending.take().map(|(size, _)| size)
        } else {
            None
        }
    }
}

#[cfg(test)]
/// Injected time crosses the old duplicate edge without sleeping or a GPU.
mod tests;
