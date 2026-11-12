//! D-60 matching-image admission and one absolute geometry-change deadline.
//!
//! These rules have no native layer dependency: tests can inject the clock and
//! exercise a closed mailbox or spurious wake without creating an AppKit view.
//! Only a ready image matching both physical size and scale can satisfy display.
use super::{Geometry, Shared};
use std::time::{Duration, Instant};

/// Obsolete geometry releases ownership independently of waiting admission.
impl super::State {
    /// A host update and a delayed GPU callback use exactly the same retirement rule.
    pub(super) fn retire_obsolete(&mut self) {
        if self
            .ready
            .as_ref()
            .is_some_and(|ready| self.desired.is_some_and(|wanted| wanted != ready.geometry))
            && let Some(ready) = self.ready.take()
        {
            // Return metadata too; neither the layer nor its hit regions adopted it.
            self.retired.push_back(ready.serial);
        }
    }
    /// Completion cannot resurrect an old size after its last main-thread discard.
    pub(super) fn publish(&mut self, ready: super::Ready, ordinal: u64, succeeded: bool) {
        if self.closed {
            // Close already recycled the host ledger and forbids a new wake.
            return;
        }
        // Failed and superseded commands still settle their request. Waiting
        // credit describes completion, not permission to display obsolete ink.
        self.requests.complete(ordinal);
        // Retaining ready also excludes its texture from pool acquisition.
        // Check geometry before taking that ownership, including delayed fences.
        if succeeded
            && ready.slot.valid.load(std::sync::atomic::Ordering::Acquire)
            && self.desired.is_none_or(|wanted| wanted == ready.geometry)
            && self
                .ready
                .as_ref()
                .is_none_or(|old| old.serial < ready.serial)
        {
            if let Some(old) = self.ready.replace(ready) {
                // Only the newest accepted image can install its region table.
                self.retired.push_back(old.serial);
            }
        } else {
            // The render owner returns this unpresented table without a UI callback.
            self.retired.push_back(ready.serial);
        }
    }
}

/// A display with no rate observation uses the scheduler's existing 60-Hz fallback.
fn period(millihertz: u32) -> Duration {
    // Host display rates are in millihertz; seconds per tick are 1000/rate.
    // The unknown-rate fallback is shared with the existing desktop scheduler.
    // Using its period bounds waiting without a new user-visible tuned margin.
    Duration::from_secs_f64(1000.0 / f64::from(if millihertz == 0 { 60_000 } else { millihertz }))
}
/// Never hold the mailbox while requesting rendering or touching Core Animation.
pub(super) fn take_ready(
    shared: &Shared,
    geometry: Geometry,
    deadline: Option<Instant>,
) -> (Option<super::Ready>, bool) {
    // Condvar atomically releases this guard while sleeping; GPU completion
    // therefore never waits for the main thread to finish its bounded wait.
    let mut state = shared.lock();
    loop {
        if state.closed {
            return (None, false);
        }
        // Completion may have raced a newer-size transaction. Discarding an old
        // ready value releases only that image, never the currently displayed one.
        if let Some(ready) = state.ready.take() {
            // An earlier GPU completion can race a later main-thread adoption.
            // Monotonic ordinals reject that image without consulting the parser
            // or any display-link index, which belongs to another namespace.
            let newer = state.current.as_ref().is_none_or(|current| {
                ready.serial > current.serial.load(std::sync::atomic::Ordering::Acquire)
            });
            if newer && ready.slot.valid.load(std::sync::atomic::Ordering::Acquire) {
                if ready.geometry == geometry {
                    return (Some(ready), false);
                }
                // A GPU wake can precede CA layout for the new host bounds.
                // Keep the image until that transaction, rather than throwing
                // away finished future geometry and rendering it a second time.
                state.ready = Some(ready);
            } else {
                // Invalid or already presented images cannot retain metadata either.
                state.retired.push_back(ready.serial);
            }
        }
        let Some(deadline) = deadline else {
            return (None, false);
        };
        // Recompute remaining time after spurious wakes; never restart the bound.
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return (None, true);
        }
        let (next, _) = shared
            .ready
            .wait_timeout(state, left)
            .unwrap_or_else(|e| e.into_inner());
        // A spurious notification carries no image. Loop with the original
        // absolute deadline and inspect closed/ready again under the same lock.
        state = next;
    }
}

/// The sole admission rule for a main-thread wait; callers inject the clock.
pub(super) fn wait_deadline(
    committed: Option<Geometry>,
    requested: Geometry,
    display: bool,
    now: Instant,
    rate: u32,
) -> Option<Instant> {
    (display && committed != Some(requested)).then(|| now + period(rate))
}
