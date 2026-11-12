//! Finite development scroll recording through the ordinary native window path.
//!
//! This module is compiled only with the existing lock-metrics diagnostic feature,
//! and activated only by PUZZLE_SCROLL_PROBE=1. It owns no alternative renderer.
//! Fixture output enters Session, typed notches enter its normal wheel policy,
//! and the existing pacer/snapshot/Fifo surface path decides each submission.
//! Startup, glyph warming and idle phase calibration precede the recorded interval.
//! Notches retarget every 80 ms, keeping the 120-ms tween active for two seconds.
//! Every successful submission timestamp is retained; no slow sample is discarded.
//! Monitor metadata and actual intervals are reported separately: a 60-Hz display
//! cannot pass a 120-Hz requirement merely because the model has a fast clock.
//! The probe's deadline exists only while active; normal idle scheduling is unchanged.
//! It closes only its own harness window after writing the ignored target artifact.
//! A timestamp records the call that submits to Fifo, not a GPU-completion query.
//! This is the harness presentation evidence requested by M8.8; visual feel stays
//! a separate human review. All warmup submissions remain outside the interval.
//! The first and last timestamps bound edge gaps as well as interior misses.
//! Requiring the full expected sample count also catches a consistently slow clock
//! whose small individual delays never produce a single two-refresh gap.
//! Monitor refresh metadata is queried from the window that actually renders.
//! Unknown monitor rates are retained as null, never silently treated as 120 Hz.
//! Input deadlines use real Instant values; this is not the injected-clock test.
//! The finite history is deliberately much longer than the notch sequence.
//! Default multiplier three traverses fewer than 1,200 of its 5,000 rows,
//! so reaching the history boundary cannot shorten the animated workload.
//! The benchmark is run with a private empty config and a plain /bin/sh child;
//! it neither reads personal terminal settings nor types into a user's session.
//! No per-frame logging, output formatting or filesystem work enters the interval.
//! The development feature already observes owner/publication timing; the probe adds one
//! preallocated timestamp push per successful submission and no new allocation.
//! The ordinary 240×70 CPU probe measures assembly separately from Fifo waits.
use puzzle_term::{Origin, messages::Command, viewport::ScrollInput, vt::input::*};
use serde_json::json;
use std::time::{Duration, Instant};
/// Finite setup/input deadlines for the ordinary winit event loop.
mod driver;
/// CPU phase records are stored without formatting during the measured interval.
mod timing;

/// Setup and glyph warming finish before any submission is counted.
#[derive(PartialEq)]
enum Stage {
    /// Wait for startup output before installing the neutral history.
    Prepare,
    /// Give fonts, history parsing and idle calibration time to settle.
    Warm,
    /// Continuous notch retargeting owns the finite measured interval.
    Record,
}

/// Startup, warm fixture, then a finite sequence of real local scroll animations.
pub(super) struct ScrollProbe {
    /// Next setup or wheel deadline, used by ControlFlow::WaitUntil.
    due: Instant,
    /// Current setup or recording phase; only Record retains timestamps.
    stage: Stage,
    /// Actual monotonic start of the measured interval.
    pub(in crate::window) started: Option<Instant>,
    /// UTC origin pairs Record with actual main-thread CA assignment receipts.
    record_unix_ns: u128,
    /// Published grid and target display after the warm-up, rather than a size guess.
    geometry: serde_json::Value,
    /// Terminal-relative milliseconds after successful native GPU submissions.
    frames: Vec<f64>,
    /// Current OS-reported refresh rate, retained independently of cadence.
    millihertz: Option<u32>,
    /// Name of the monitor hosting the recorded window, never an inferred default.
    monitor_name: Option<String>,
    /// Raw monitor resolution distinguishes the LG 5K from the built-in display.
    monitor_size: Option<[u32; 2]>,
    /// Physical render target, independent of the full display resolution.
    surface_size: [u32; 2],
    /// Every measured frame includes its chosen deadline and CPU work.
    samples: Vec<timing::Sample>,
    /// Idle calibration attempts remain visible even if they happen between frames.
    calibrations: Vec<Instant>,
    /// A separate keyecho run measures input-to-submission using the same recorder.
    keyecho: bool,
    /// Authored input instants pair with keyecho's successive counter values.
    inputs: Vec<f64>,
    /// Native start and callback clocks distinguish idle activation from lost frames.
    pub(in crate::window) restart: super::restart::Journal,
}

impl ScrollProbe {
    /// Allocates observation storage only for an explicitly requested dev run.
    pub(super) fn enabled() -> Option<Self> {
        (std::env::var("PUZZLE_SCROLL_PROBE").as_deref() == Ok("1")).then(|| Self {
            // Let the login shell's initial output finish before installing neutral history.
            due: Instant::now() + Duration::from_millis(600),
            stage: Stage::Prepare,
            started: None,
            record_unix_ns: 0,
            geometry: serde_json::Value::Null,
            frames: Vec::with_capacity(512),
            millihertz: None,
            monitor_name: None,
            monitor_size: None,
            surface_size: [0; 2],
            samples: Vec::with_capacity(512),
            calibrations: Vec::with_capacity(8),
            keyecho: std::env::var("PUZZLE_SCROLL_PROBE_KEYS").as_deref() == Ok("1"),
            inputs: Vec::with_capacity(32),
            restart: super::restart::Journal::default(),
        })
    }

    /// Records successful submissions and their phases without per-frame logging.
    pub(super) fn presented(
        &mut self,
        start: Instant,
        at: Instant,
        prediction: Option<(Instant, Instant, u64, &'static str)>,
        timing: &super::live::PresentTiming,
    ) {
        if self.stage == Stage::Record
            && let Some(started) = self.started
        {
            self.frames
                .push(at.duration_since(started).as_secs_f64() * 1000.0);
            self.samples
                .push(timing::Sample::new(start, at, prediction, timing));
        }
    }

    /// Retains calibration attempts to identify a blocking idle turn inside a gap.
    #[cfg(not(target_os = "macos"))]
    pub(super) fn calibrated(&mut self, now: Instant) {
        self.calibrations.push(now);
    }

    /// Records raw timestamps alongside the unchanged 120-Hz target.
    fn finish(&self, now: Instant, metrics: serde_json::Value) -> Result<(), String> {
        let elapsed = self
            .started
            .map_or(0.0, |at| now.duration_since(at).as_secs_f64() * 1000.0);
        // A missed 120-Hz slot creates about a 16.67-ms gap; allow half a period
        // for timestamp/wakeup jitter without admitting two-refresh intervals.
        let gaps: Vec<_> = self.frames.windows(2).map(|p| p[1] - p[0]).collect();
        let missed = gaps.iter().filter(|ms| **ms > 12.5).count();
        let report = json!({
            "keyecho":self.keyecho, "input_ms":self.inputs,
            "target_hz":120,"record_unix_ns":self.record_unix_ns,"geometry":self.geometry, "monitor_name":self.monitor_name, "monitor_millihertz":self.millihertz,
            "monitor_resolution_device_px":self.monitor_size, "surface_size_device_px":self.surface_size,
            "elapsed_ms":elapsed, "submission_ms":self.frames,
            "frames":self.started.map(|at| self.samples.iter().map(|s| s.json(at, &self.calibrations)).collect::<Vec<_>>()),
            "frame_gates":metrics,
            "display_link":self.started.map(|origin| self.restart.json(origin)),
            "missed_intervals":missed,
            "max_gap_ms":gaps.iter().copied().fold(0.0, f64::max),
            "pass":self.millihertz.is_some_and(|hz| hz >= 120_000)
                && (self.frames.len() + 1) as f64 >= (elapsed * 120.0 / 1000.0).floor()
                && missed == 0
                && self.frames.first().is_some_and(|ms| *ms <= 12.5)
                && self.frames.last().is_some_and(|ms| elapsed - ms <= 12.5)
        });
        // This is private generated evidence, never a checked-in recording or asset.
        std::fs::write("target/m8-desktop-scroll.json", report.to_string() + "\n")
            .map_err(|e| e.to_string())
    }
}

impl ScrollProbe {
    /// Keyecho inputs are sparse bursts; scroll notches keep the tween continuous.
    fn input_period(&self) -> Duration {
        Duration::from_millis(if self.keyecho { 100 } else { 80 })
    }

    /// Recorded native callbacks use the existing measured Ctrl+A sequence.
    fn input(&mut self, live: &mut super::live::LiveWindow, now: Instant) {
        if !self.keyecho {
            notch(live, now);
            return;
        }
        if let Some(started) = self.started {
            self.inputs
                .push(now.duration_since(started).as_secs_f64() * 1000.0);
        }
        match crate::keyboard::sequences::for_chord("ctrl+a") {
            Ok((events, _)) => {
                for event in events {
                    live.input(event);
                }
            }
            Err(error) => tracing::warn!(event="desktop.key_probe_failed", %error),
        }
    }
}

/// Typed notches reuse Session's local override and M8.2 continuous retargeting.
fn notch(live: &super::live::LiveWindow, _now: Instant) {
    let pointer = MouseEvent {
        kind: MouseKind::Wheel,
        button: 64,
        column: 0,
        row: 0,
        pixel_x: 0,
        pixel_y: 0,
        modifiers: Modifiers(Modifiers::SHIFT),
        button_held: false,
        click_count: 0,
    };
    // Shift policy and encoding run on the sole owner, after all earlier messages.
    live.send(puzzle_term::SessionMessage::Command(
        Command::Wheel {
            delta: ScrollInput::Rows(-16.0),
            pointer,
        },
        Origin::User,
    ));
}
