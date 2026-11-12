//! Owner-local D-50 scheduling values, with D-33's actual output timestamps.
//!
//! Taking a notification does not acknowledge its output. Only publication
//! consumes that interval, so output during a render build wakes the next turn.
//! All state lives with Session; no renderer-side lock or atomic latch is needed.
use super::{FrameArrival, Instant, WakeRequests, Waker};

/// Coalesces desktop damage until the owner begins filling a free snapshot slot.
#[derive(Default)]
pub struct PublicationWaker {
    /// True until publication represents earlier requested damage.
    queued: bool,
    /// Suppressed requests still update silence detection for the following burst.
    last: Option<Instant>,
    /// Deferred values leave Session through its one host outbox.
    pending: WakeRequests,
}

impl PublicationWaker {
    /// Records actual output time; injected instants make the burst rule deterministic.
    pub fn request_at(&mut self, now: Instant) {
        // Retain every write's time, even when the display already has a request.
        let previous = self.last.replace(now);
        if self.queued {
            return;
        }
        self.queued = true;
        self.pending.frame = true;
        self.pending.arrival = Some(FrameArrival {
            first: now,
            previous,
        });
    }
}

impl Waker for PublicationWaker {
    /// Native output timestamps are measured at the request, never at UI delivery.
    fn request_frame(&mut self) {
        self.request_at(Instant::now());
    }
    /// Retain the earliest finite timer; idle taking produces no new timer.
    fn wake_at(&mut self, instant: Instant) {
        self.pending.deadline = Some(
            self.pending
                .deadline
                .map_or(instant, |old| old.min(instant)),
        );
    }
    /// The same Session owner releases damage before advancing the publication turn.
    fn acknowledge_frame(&mut self) {
        // Output arriving after this boundary gets its own scheduling value.
        self.queued = false;
    }
    /// Moves scheduling values without releasing outstanding snapshot ownership.
    fn take_requests(&mut self) -> WakeRequests {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
/// Injected output timing proves coalescing, final-write rearming and timer priority.
mod tests;
