//! Finite measurement lifecycle on the ordinary native window (M8.8).
//!
//! Prepare waits for the shell prompt, feeds neutral history and records hardware.
//! Warm gives glyph upload and idle calibration time before measurements start.
//! Record produces ordinary input requests; Pacer still chooses every snapshot.
//! The recorder never treats a requested frame as a successful presentation.
//! A keyecho run leaves its own alternate-screen program in charge of output.
//! A scroll run uses local notches and a much longer history than the input sweep.
//! Monitor resolution and surface size are separately expressed in device pixels.
//! This distinction matters for the external LG 5K and the built-in Retina display.
//! An explicit 300-frame experiment exits Warm into its separate unpaced loop.
//! Only that requested experiment bypasses the scheduler to observe FIFO itself.
//! Nothing in a default desktop build invokes this development lifecycle.
//! Deadlines return to WaitUntil, so these stages do not introduce polling.
//! Every run saves its complete interval before closing its owned process.
//! Timed intervals omit setup, glyph warming, JSON encoding and filesystem work.
//! The OS refresh metadata remains evidence rather than a guarantee of scanout.
//! ProMotion's adaptive actual rate must be checked against measured cadence.
//! Warm-up volume is an explicit glyph-loss bisect input, not a timing tuning
//! parameter. Default measurements retain all 5,000 rows, so the same notch
//! sweep cannot hit the history boundary and accidentally end animation early.
//! Startup placement and config alternatives are exercised only by the separate
//! retained-atlas ink guard; the four latency groups use identical fixtures.
use super::super::{live::LiveWindow, observer::Observer};
use super::{ScrollProbe, Stage};
use crate::window::live::{GlyphBisect, PresentProbe};
use std::time::{Duration, Instant};

impl Observer {
    /// Supplies finite input deadlines without modifying normal presentation decisions.
    pub(in crate::window) fn scroll_probe_tick(
        &mut self,
        now: Instant,
        live: &mut LiveWindow,
    ) -> Option<Instant> {
        let probe = self.scroll_probe.as_mut()?;
        if now < probe.due {
            return Some(probe.due);
        }
        if probe.stage == Stage::Prepare {
            probe.prepare(live, now);
        } else if probe.stage == Stage::Warm {
            // Read the live executor's existing atlas before any diagnostic
            // counter can introduce replacement glyphs or change the fixture.
            if std::env::var("PUZZLE_GLYPH_BISECT").as_deref() == Ok("1") {
                if let Err(error) = live.glyph_bisect() {
                    tracing::warn!(event = "desktop.glyph_bisect_failed", %error);
                }
                live.window.close();
                return None;
            }
            // The reviewer requests an unpaced burst to test FIFO itself. This
            // owned diagnostic exits here; ordinary scrolling still uses Pacer.
            if std::env::var("PUZZLE_PRESENT_PROBE").as_deref() == Ok("300") {
                if let Err(error) = live.present_probe() {
                    tracing::warn!(event = "desktop.present_probe_failed", %error);
                }
                live.window.close();
                return None;
            }
            // Pair worker submissions with actual main-thread layer assignments.
            probe.record_unix_ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                // The caller's monotonic interval began before this callback.
                // Remove its age rather than pairing UTC with a later origin.
                .map_or(0, |time| {
                    time.as_nanos().saturating_sub(now.elapsed().as_nanos())
                });
            probe.geometry = super::super::measurement_display::witness(live);
            probe.started = Some(now);
            // The bounded HUD diagnosis needs native ticks and publication clocks
            // on the same origin as scroll frames; observation changes no policy.
            live.frame_metrics.start_at(now);
            probe.stage = Stage::Record;
            probe.input(live, now);
            probe.due = now + probe.input_period();
        } else if probe
            .started
            .is_some_and(|at| now.duration_since(at) >= Duration::from_secs(2))
        {
            if let Err(error) = probe.finish(now, live.frame_metrics.report()) {
                tracing::warn!(event="desktop.scroll_probe_failed", %error);
            } else {
                tracing::info!(
                    event = "desktop.scroll_probe_complete",
                    "saved actual scroll submission timestamps"
                );
            }
            // The benchmark owns this process, so normal user windows are unaffected.
            live.window.close();
            return None;
        } else {
            probe.input(live, now);
            probe.due = now + probe.input_period();
        }
        Some(probe.due)
    }
}

impl ScrollProbe {
    /// Records actual placement and feeds the finite neutral warm-up history.
    fn prepare(&mut self, live: &super::super::live::LiveWindow, now: Instant) {
        // Query the window after any startup placement/DPR event. Recording a
        // requested monitor would hide a failed or delayed move to the built-in.
        self.monitor_name = live.window.current_monitor().and_then(|m| m.name());
        self.monitor_size = live
            .window
            .current_monitor()
            .map(|m| [m.size().width, m.size().height]);
        let size = live.window.inner_size();
        self.surface_size = [size.width, size.height];
        self.millihertz = live
            .window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz());
        // Real VT output warms all letters; no blank or separate toy frame is counted.
        if !self.keyecho {
            // The reviewer-requested bisect changes only setup volume; one
            // alphabet row remains available for identical per-cell readback.
            let rows = if std::env::var("PUZZLE_GLYPH_HISTORY").as_deref() == Ok("0") {
                1
            } else {
                5_000
            };
            let bytes =
                b"scroll ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz\r\n".repeat(rows);
            if let Err(error) =
                live.session
                    .probe(now + Duration::from_millis(750), move |session| {
                        session.feed(&bytes, now);
                    })
            {
                tracing::warn!(event = "desktop.scroll_probe_failed", %error);
            }
        }
        self.stage = Stage::Warm;
        self.due = now + Duration::from_millis(750);
    }
}
