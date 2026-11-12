//! Immediate resize and paced redraw callbacks for the live window.
//!
//! A physical surface follows dragging now; PTY SIGWINCH takes the latest
//! grid at each first-event 25-ms throttle deadline. Resize frames bypass pacing
//! so the backing store never lags the OS window. Ordinary VT damage waits for
//! the display turn.
//!
//! Glyph misses finish synchronously on this render thread (D-41).
//! Surface acquisition touches no Session; its sole owner keeps encoding keys.

use super::super::pacing::Pacer;
use super::RenderApp;
use crate::render_thread::surface::Extent;
#[cfg(feature = "lock-metrics")]
use std::time::Duration;
use std::time::Instant;

impl RenderApp {
    /// Starts the display clock after the OS window has a monitor.
    pub fn init_pacer(&mut self) {
        // The monitor is unknown before `resumed`; creating this state in
        // WindowApp::new would always assume 60 Hz on a 120 Hz display.
        let Some(live) = &self.live else {
            return;
        };
        // ProMotion may change its active rate; the highest advertised
        // mode keeps input echo within the shortest display period.
        // A detached or virtual display may expose no mode at all. The
        // 60 Hz fallback still gives a finite deadline without polling.
        let reset_reason = if self.pacer.is_none() {
            // Startup differs from five-second staleness: no phase was learned yet.
            "startup"
        } else {
            // Reconfiguration is visible in calibration's separate configure age.
            "monitor_or_dpr_change"
        };
        let rate = live.window.info.millihertz;
        #[cfg(not(target_os = "macos"))]
        let mut pacer = Pacer::new(rate, self.render_settle);
        #[cfg(target_os = "macos")]
        let mut pacer = Pacer::new_linked(rate, self.render_settle);
        pacer.reset_reason(reset_reason);
        self.pacer = Some(pacer);
        // The very first clear/text frame is exempt from the predicted
        // phase because acquisition has not yet supplied a phase sample.
        if let Some(pacer) = self.pacer.as_mut() {
            pacer.request(Instant::now(), false);
        }
        live.window.request_redraw();
    }

    /// Reconfigures the backing immediately and schedules one PTY resize.
    pub fn resize(&mut self, size: Extent) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        tracing::debug!(target: "puzzle_desktop::input", event = "Resized", width = size.width, height = size.height);
        // The recording needs the physical extent before a corresponding
        // frame entry, so a private replay can reproduce its exact grid.
        live.resize_surface(size);
        // GPU attachments follow the drag immediately, whereas VT reflow
        // waits until draw_due and uses the most recent of many OS sizes.
        self.grid_pending = true;
        // A stale bitmap stretched during a live drag is more distracting
        // than one exceptional unpaced redraw at the new physical extent.
        if let Some(pacer) = self.pacer.as_mut() {
            pacer.request(Instant::now(), true);
        }
        live.window.request_redraw();
        // A running throttle window keeps its first deadline during continuous drag.
        self.arm_pty_resize(Instant::now());
    }

    /// Updates device-cell geometry after a monitor scale change.
    pub fn dpr(&mut self, scale_factor: f64) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        tracing::debug!(target: "puzzle_desktop::input", event = "ScaleFactorChanged", scale_factor);
        // Hinted outlines and device-cell dimensions both depend on DPR;
        // changing the surface alone would blur or misplace glyphs.
        live.change_dpr(scale_factor);
        // A monitor crossing can produce both scale and size callbacks.
        // The pending flag collapses them into one latest-extent reflow.
        self.grid_pending = true;
        self.arm_pty_resize(Instant::now());
        // The monitor crossing may also change its refresh rate. Relearn
        // the phase from the new surface rather than reusing the old clock.
        self.init_pacer();
    }

    /// Takes the latest Session snapshot only at its chosen display deadline.
    pub fn draw_due(&mut self) {
        #[cfg(feature = "lock-metrics")]
        let prediction = self.pacer.as_ref().and_then(Pacer::prediction);
        let deadline = self
            .pacer
            .as_ref()
            .map_or(Instant::now(), |pacer| pacer.build_deadline(Instant::now()));
        #[cfg(target_os = "macos")]
        let deadline = self.geometry_deadline.take().unwrap_or(deadline);
        // Winit may generate a redraw callback for expose events before
        // our deadline. It must not snapshot a transient VT update early.
        if !self
            .pacer
            .as_mut()
            .is_some_and(|pacer| pacer.take(Instant::now()))
        {
            #[cfg(feature = "lock-metrics")]
            if let Some(live) = self.live.as_mut() {
                live.frame_metrics.denied();
            }
            return;
        }
        let Some(live) = self.live.as_mut() else {
            return;
        };
        // The grid uses the latest physical extent immediately before
        // copying rows, while PTY SIGWINCH follows its independent throttle timer.
        if self.grid_pending {
            self.grid_pending = false;
            live.apply_grid_resize();
        }
        let started = Instant::now();
        #[cfg(feature = "lock-metrics")]
        // D-33 edge evidence remains available without offering glyph-work slack.
        live.frame_metrics
            .prediction(prediction.map(|(_, at, _, _)| at));
        #[cfg(feature = "lock-metrics")]
        live.frame_metrics.clock_plan(
            prediction,
            self.pacer
                .as_ref()
                .map_or([Duration::ZERO; 2], Pacer::clock_costs),
        );
        // A no-damage redraw does not teach frame cost or display phase;
        // only a successful swapchain submission contributes a sample.
        if let Some(timing) = live.redraw(deadline) {
            let ended = Instant::now();
            self.perf.presented(ended);
            #[cfg(feature = "lock-metrics")]
            if let Some(observer) = &mut self.observer {
                // Observers inspect only this submitted image, after the real present.
                observer.presented(started, ended, prediction, &timing, live);
            }
            // Acquisition waits longer than 1 ms identify the compositor
            // boundary; short acquisitions retain the prior phase sample.
            if let Some(pacer) = self.pacer.as_mut() {
                pacer.snapshot_received(live.snapshot_done, live.snapshot_wait_and_latency);
                pacer.presented(started, timing.acquired, timing.blocked, ended);
            }
        }
    }
}

impl RenderApp {
    /// Host clocks take over before interpreting the first independent refresh index.
    pub(super) fn use_host_clock(&mut self, host_driven: bool) {
        if host_driven && let Some(pacer) = &mut self.pacer {
            pacer.use_host_refresh();
        }
    }
    /// A changed-geometry CA callback has priority over settling ordinary output.
    #[cfg(target_os = "macos")]
    pub(super) fn geometry_opportunity(&mut self, deadline: Option<Instant>, now: Instant) {
        if let Some(deadline) = deadline {
            // Apply after ordinary damage/refresh decisions: neither a burst
            // settle window nor a tick may postpone this bounded CA opportunity.
            // The normal display-link policy resumes after this exceptional frame.
            self.geometry_deadline = Some(deadline);
            if let Some(pacer) = &mut self.pacer {
                pacer.request(now, true);
            }
        }
    }
}
