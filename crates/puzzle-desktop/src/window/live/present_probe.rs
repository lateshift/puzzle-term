//! Reviewer-requested 300-frame windowed FIFO/coalescing experiment (M8.8).
//!
//! Each distinct counter goes through the real Session snapshot, frame builder,
//! renderer and surface. Only this opt-in development probe bypasses Pacer: a
//! scheduled burst could never reveal whether FIFO itself limits submissions.
//! All text/storage is prepared before timing. Readback and JSON/file work happen
//! afterward, so no fence, sleep or logging accidentally supplies display pacing.
//! Submission timestamps do not identify which images the compositor displays.
//! Its explicit owner query is diagnostic only, compiled with the measurement feature.
//! macOS omits retained-image calibration even here: its native link owns the clock.
use super::LiveWindow;
use serde_json::json;
use std::time::Instant;

/// Harness-only observation of the engine-owned surface.
pub(in crate::window) trait PresentProbe {
    /// Runs the authored measurement using retained resources.
    fn present_probe(&mut self) -> Result<(), String>;
    /// Counter feed uses the ordinary owner message and actual submission.
    fn probe_counter(&mut self, text: &str) -> Result<super::PresentTiming, String>;
}

impl PresentProbe for LiveWindow {
    /// Submits 300 distinct counters as quickly as the actual FIFO surface allows.
    /// Returns an error instead of retaining partial evidence after a failed draw.
    fn present_probe(&mut self) -> Result<(), String> {
        let visibility_start = self.visibility_evidence("burst_start");
        warm_counter(self)?;
        // Identical-counter control distinguishes content from the call path.
        let distinct = std::env::var("PUZZLE_PRESENT_PROBE_STATIC").as_deref() != Ok("1");
        let counters: Vec<_> = (1..=300)
            .map(|n| format!("\x1b[H\x1b[2Kframe {:03}", if distinct { n } else { 0 }))
            .collect();
        let mut samples = Vec::with_capacity(counters.len());
        // No event-loop deadline or phase calibration runs inside this loop.
        let started = Instant::now();
        for text in &counters {
            let requested = Instant::now();
            let timing = self.probe_counter(text)?;
            samples.push((requested, timing));
        }
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        let visibility_end = self.visibility_evidence("burst_end");
        // A final retained-frame readback proves real counter ink exists, but
        // happens outside elapsed and cannot make the timed burst block.
        if let Some(verifier) = self.verifier.as_mut() {
            verifier.save_present_probe(&self.renderer, &mut self.frame)?;
        }
        let monitor = self.window.current_monitor();
        let millihertz = monitor.as_ref().and_then(|m| m.refresh_rate_millihertz());
        let report = json!({
            "frame_count":samples.len(), "elapsed_ms":elapsed, "present_mode":"Fifo", "distinct":distinct,
            "maximum_frame_latency":self.renderer.maximum_frame_latency(),
            "visibility_start":visibility_start, "visibility_end":visibility_end,
            "monitor_name":monitor.and_then(|m| m.name()), "monitor_millihertz":millihertz,
            "expected_fifo_ms":millihertz.map(|hz| 300.0 * 1_000_000.0 / f64::from(hz)),
            "surface_size":self.frame.target_size, "windowed":self.window.fullscreen().is_none(),
            "frames":samples.iter().enumerate().map(|(n, (request, timing))| json!({
                "counter":n+1, "request_ms":request.duration_since(started).as_secs_f64()*1000.0,
                "acquire_start_ms":(timing.acquired-timing.blocked).duration_since(started).as_secs_f64()*1000.0,
                "acquired_ms":timing.acquired.duration_since(started).as_secs_f64()*1000.0,
                "submitted_ms":timing.submitted.duration_since(started).as_secs_f64()*1000.0,
                "presented_ms":timing.presented.duration_since(started).as_secs_f64()*1000.0,
                "acquire_wait_ms":timing.blocked.as_secs_f64()*1000.0,
            })).collect::<Vec<_>>(),
        });
        // Ignored evidence belongs to this owned process, never shipped assets.
        std::fs::write("target/m8-fast-present.json", report.to_string() + "\n")
            .map_err(|e| e.to_string())
    }

    /// Feeds one short VT counter and submits its ordinary damaged snapshot.
    fn probe_counter(&mut self, text: &str) -> Result<super::PresentTiming, String> {
        let period = std::time::Duration::from_secs_f64(
            1000.0 / f64::from(self.window.info.millihertz.max(1)),
        );
        let deadline = Instant::now() + period;
        let bytes = text.as_bytes().to_vec();
        self.session.probe(deadline, move |session| {
            session.feed(&bytes, Instant::now());
        })?;
        // A missing snapshot/acquisition invalidates the experiment; do not
        // count requests that never reached the real surface as presentations.
        self.redraw(deadline)
            .ok_or_else(|| "counter frame was not submitted".into())
    }
}

/// Warms the same FIFO counter before either distinct or static measurements.
fn warm_counter(live: &mut LiveWindow) -> Result<(), String> {
    // Retained-history and warmed-counter controls share this same window,
    // keeping monitor, visibility and drawable ownership fixed for comparison.
    #[cfg(not(target_os = "macos"))]
    let compare = std::env::var("PUZZLE_CALIBRATION_REPEAT").as_deref() == Ok("1");
    #[cfg(not(target_os = "macos"))]
    if compare {
        // This retained history frame has been idle for the whole warmup.
        live.calibrate_phase();
    }
    // Reset the scroll fixture to a simple, visible primary-screen counter.
    // Warm every digit before timing; a cold glyph is not compositor pacing.
    live.probe_counter("\x1b[?25l\x1b[2J\x1b[Hframe 0123456789")?;
    // Compare the identical calibration code after a real counter frame.
    // Repeated diagnostic attempts reveal whether returning from the loop
    // changes drawable retention; the production bound remains unchanged.
    #[cfg(not(target_os = "macos"))]
    if compare {
        for _ in 0..3 {
            live.calibrate_phase();
        }
    }
    Ok(())
}
