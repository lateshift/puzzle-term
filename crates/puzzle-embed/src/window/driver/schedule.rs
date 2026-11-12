//! Idle calibration, frame deadlines and PTY resize waits for winit.
//!
//! A redraw request names a future snapshot deadline. This module only
//! selects the earliest useful OS wake; it does not build terminal frames.
//! Dragged surfaces need an immediate GPU extent even though a child process
//! receives the latest grid at each throttle deadline, even during a drag.
//! An idle calibration present sends the retained image without copying VT
//! rows, because no terminal content changed while learning display phase.
//! A cold-frame comparison can map GPU memory for longer than one refresh;
//! it therefore gets an idle deadline and yields to terminal presentations.
use super::super::{live::LiveWindow, pacing::Pacer};
use super::RenderApp;
#[cfg(not(target_os = "macos"))]
use std::time::Duration;
use std::time::Instant;

impl RenderApp {
    /// Arm the first-event D-29 deadline; later arrivals cannot postpone it.
    pub(super) fn arm_pty_resize(&mut self, arrived: Instant) {
        // A mailbox can retain an earlier host arrival than the local resize
        // callback. Keep that earlier deadline too when rendering was busy.
        let due = arrived + std::time::Duration::from_millis(25);
        self.pty_resize_due = Some(self.pty_resize_due.map_or(due, |old| old.min(due)));
    }

    /// Sends the latest grid when the current first-event window expires.
    fn flush_resize_due(&mut self, now: Instant) {
        // The first event starts the window; new extents replace only its size.
        // Continuous dragging therefore cannot starve the child of SIGWINCH.
        if self.pty_resize_due.is_some_and(|due| now >= due) {
            // Clear before sending: the next event starts a fresh throttle window.
            self.pty_resize_due = None;
            if let Some(live) = self.live.as_mut() {
                if self.grid_pending {
                    // Apply the newest pending grid before its ordered PTY message;
                    // both the child and the next image use this same geometry.
                    self.grid_pending = false;
                    live.apply_grid_resize();
                    live.window.request_redraw();
                }
                // The child receives exactly the grid that the next image
                // will display, so tput and the visible columns agree.
                live.flush_pty_resize();
            }
        }
    }

    /// Samples retained-image FIFO acquisition while recording visibility evidence.
    #[cfg(not(target_os = "macos"))]
    fn calibrate_due(&mut self, now: Instant) -> Option<Instant> {
        // Retain the existing extent-only eligibility while visibility is measured.
        // Focus/frontmost are evidence, not gates for visible background terminals.
        let visible = self.live.as_ref().is_some_and(|live| {
            let size = live.window.inner_size();
            size.width > 0 && size.height > 0
        });
        let calibration_due = self
            .pacer
            .as_ref()
            .and_then(|pacer| pacer.calibration_due(now, visible));
        if calibration_due.is_some_and(|due| now >= due) {
            // Rendering only the retained image fills the configured FIFO depth:
            // either acquisition can identify an edge if a drawable was already queued.
            #[cfg(feature = "lock-metrics")]
            if let Some(observer) = &mut self.observer {
                observer.calibrated(now);
            }
            let sample = self.live.as_mut().and_then(LiveWindow::calibrate_phase);
            tracing::debug!(
                event = "desktop.calibration_result",
                wait_ms = sample
                    .as_ref()
                    .map(|timing| timing.blocked.as_secs_f64() * 1000.0),
                qualified = sample
                    .as_ref()
                    .is_some_and(|timing| timing.blocked > Duration::from_millis(1))
            );
            if let Some(pacer) = self.pacer.as_mut() {
                pacer.calibrated(
                    Instant::now(),
                    sample
                        // A quick acquire reused an available texture and
                        // says nothing about the monitor's visible phase.
                        .filter(|timing| timing.blocked > Duration::from_millis(1))
                        .map(|timing| timing.acquired),
                );
            }
        }
        // A no-sample attempt retries after a fresh 250-ms idle opportunity;
        // learned phase ages at five seconds. Both use the same OS idle wait.
        // Recompute so neither outcome depends on another PTY byte arriving.
        self.pacer
            .as_ref()
            .and_then(|pacer| pacer.calibration_due(Instant::now(), visible))
    }

    /// Turns queued deadlines into one OS wait rather than polling frames.
    pub fn schedule(&mut self) -> Option<Instant> {
        let now = Instant::now();
        // Harness workload deadlines use the ordinary surface path; no observer
        // exists in a normal embedding host or changes the refresh reservation.
        #[cfg(feature = "lock-metrics")]
        let observer_due = self.observer.as_mut().and_then(|observer| {
            let live = self.live.as_mut()?;
            observer.tick(now, live, self.pacer.as_ref())
        });
        self.flush_resize_due(now);
        if self.timer_due.is_some_and(|due| now >= due) {
            // A timer wake simply enters Session::frame, where blink and
            // synchronized-output state decide whether pixels changed.
            self.timer_due = None;
            if let Some(pacer) = self.pacer.as_mut() {
                let animation = self.live.as_ref().is_some_and(|live| live.animating);
                if animation {
                    pacer.request_animation(now);
                } else {
                    pacer.request(now, false);
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        let calibration_due = self.calibrate_due(now);
        // Calibration has its own returned idle deadline; a failed sample cannot
        // turn a past deadline into a busy event-loop retry.
        let verify_due = self.live.as_ref().and_then(LiveWindow::verify_deadline);
        // A queued terminal frame wins over slow diagnostic readback.
        // The next successful present replaces the obsolete witness.
        if verify_due.is_some_and(|due| now >= due)
            && self
                .pacer
                .as_ref()
                .is_none_or(|pacer| pacer.due().is_none())
            && let Some(live) = self.live.as_mut()
        {
            live.verify_idle(now);
        }
        let frame_due = self.pacer.as_ref().and_then(Pacer::due);
        if frame_due.is_some_and(|due| now >= due) && self.live.is_some() {
            // One queued redraw is enough. Waiting on a deadline already
            // in the past would turn the winit loop into a busy poll.
            self.draw_due();
        }
        let due = [
            // Keep the resize and blink timers in the same minimum: a long
            // idle-calibration wait must not hold an earlier PTY resize.
            self.pty_resize_due,
            self.timer_due,
            self.pacer.as_ref().and_then(Pacer::due),
            #[cfg(not(target_os = "macos"))]
            calibration_due.filter(|due| *due > now),
            verify_due.filter(|due| *due > now),
            #[cfg(feature = "lock-metrics")]
            observer_due,
        ]
        .into_iter()
        .flatten()
        .min();
        // `Wait` preserves the zero-idle-frame rule; `WaitUntil` exists
        // only while an explicit session or drag deadline is pending.
        // Poll only while unread PTY output remains; idle still sleeps (P2).
        due
    }
}

/// Injected arrivals exercise the production deadline without a GPU or sleeps.
#[cfg(test)]
mod tests;
