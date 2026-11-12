//! Finite Unicode-flood evidence uses the ordinary winit, parser and PTY paths.
//!
//! An explicitly activated development probe starts a neutral child, counts
//! successful surface submissions during its six-second flood, and injects
//! one recorded native key. The child records actual receipt independently.
//! No normal window has this probe, its deadlines or observation allocations.
//!
//! The raw child concurrently produces UTF-8 and reads its PTY input. UTC clocks
//! compare key injection with receipt on the same machine; presentation times
//! use monotonic instants. The first second stays cold before the recorded key.
//! Every successful flood submission remains in the report, including startup
//! delays, so the denominator cannot exclude slow discovery or placeholder work.
//! A provisional first frame and later bitmap ink establish the asynchronous
//! transition independently of the final parser summary or a redraw request.
//! This observer schedules setup/input/check deadlines beside the ordinary pacer.
//! It never supplies replacement refresh times or enables slow frame verification.
//! Generated files remain under ignored target/, and the launch command uses
//! a neutral shell rather than modifying any user's dotfiles or live sessions.
//!
//! D-39 additionally selects a long pressure mode through a separate environment
//! flag. The ordinary six-second criterion and its existing bounds are unchanged.
//! The long producer invokes the actual vtebench executable for thirty seconds,
//! then switches to an authored Hangul screen without more application output.
//! Its frozen title supplies the boundary between flood metrics and ink recovery.
//! Thirty seconds later the retained executor is read without reapplying uploads.
//! Final source handles distinguish covered ink from nonblank provisional boxes.
//! The report includes every source cell, every submission and redraw duration.
//! Page evictions are counted across the full flood interval; the fixed page
//! bounds come from packing limits rather than a workload-specific time window.
//! Charged vector bytes include allocated reserve rows in both texture formats.
//! They are checked against the same 48 MiB limit used by ordinary terminals.
//! Failure to handshake, receive a key or produce a frozen image leaves no pass.
//! Both observer modes remain opt-in behind the existing lock-metrics feature.
//!
//! The pressure producer path is made absolute because login shells start at home.
//! Each probe reserves a private receipt directory before launching its child.
//! No user recordings or other applications enter this measurement lifecycle.
//! A ready title precedes release so index construction state is observable.
//! Process memory is sampled before release and after the completed interval.
//! Mapped file ownership is recorded separately from resident process pages.
//! The child independently timestamps receipt rather than relying on queue enqueue.
//! A reversed receipt timestamp rejects the fixture instead of clamping to zero.
//! Frozen recovery retains every presentation and its final ink readiness count.
//! Both Slug vectors and CPU-oracle overflow bitmaps qualify as covered ink.
//! The watchdog is a timeout for broken fixtures, never a glyph latency budget.
//! The report retains worst frame cost alongside all raw ordered frame samples.
//! Frame gate evidence locates acquire stalls separately from admitted glyph work.
//! Page size and allocated reserve bytes remain visible in the final report.
//! Fixture-generated source and local man pages remain under ignored target/.
//! The finite observer never injects alternative display-phase samples.

use super::{live::LiveWindow, observer::Observer, pacing::Pacer};
use crate::window::live::UnicodePressure;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
/// Process RSS and virtual storage are observed separately from font mappings.
mod memory;
/// Thirty-second pressure and frozen-screen phases are separate from the short gate.
mod pressure;
/// Run-owned journals and cancellable diagnostic input.
mod receipts;
/// Independent input deadlines and retained-store GPU sampling.
mod sampling;
/// The observer advances only at finite setup, input and completion deadlines.
pub(super) struct Probe {
    /// No later probe may clear this run's receipt evidence or inherit its timer.
    receipts: receipts::Receipts,
    /// Absolute neutral script selected by xtask, never a user's live shell.
    script: String,
    /// Empty until the producer acknowledges startup through its OSC title.
    started: Option<Instant>,
    /// The actual successful submissions, not redraw requests.
    frames: Vec<f64>,
    /// UTC receipt clocks share the child's system clock on this machine.
    issued_ns: Arc<AtomicU64>,
    /// Independent timer timestamps pair one-to-one with child receipt samples.
    issued_samples: Arc<std::sync::Mutex<Vec<u128>>>,
    /// A bounded observer deadline does not change native presentation pacing.
    due: Instant,
    /// A broken producer handshake must fail acceptance instead of leaving a window open.
    expires: Instant,
    /// The launch command is sent exactly once.
    launched: bool,
    /// Every sampled image comes from the executor which presented this frame.
    gpu_samples: Vec<serde_json::Value>,
    /// Limit readback frequency without selecting only favorable frames.
    last_gpu_sample: Option<Instant>,
    /// Explicit timing-only cohort omits intrusive reads and cannot pass ink acceptance.
    cadence_only: bool,
    /// Long diagnostic mode observes capacity recovery after the corpus reaches the limit.
    pressure: Option<pressure::Pressure>,
    /// Before the producer starts, index construction may already be complete.
    memory_before: serde_json::Value,
    /// An explicit diagnostic can place its recorded key inside cold glyph warming.
    key_delay: Duration,
}
impl Probe {
    /// Allocates only for an explicit development environment variable.
    pub(super) fn enabled() -> Option<Self> {
        std::env::var("PUZZLE_UNICODE_PROBE")
            .ok()
            .and_then(|script| {
                Some(Self {
                    receipts: receipts::Receipts::new(std::path::Path::new(
                        "target/m11-unicode/receipts",
                    ))
                    .map_err(|error| tracing::error!(event="desktop.unicode_probe_failed", %error))
                    .ok()?,
                    script,
                    started: None,
                    frames: Vec::with_capacity(512),
                    issued_ns: Arc::new(AtomicU64::new(0)),
                    issued_samples: Arc::new(std::sync::Mutex::new(Vec::new())),
                    due: Instant::now() + Duration::from_millis(250),
                    expires: Instant::now()
                        + Duration::from_secs(
                            if std::env::var_os("PUZZLE_UNICODE_PRESSURE").is_some() {
                                85
                            } else {
                                30
                            },
                        ),
                    launched: false,
                    gpu_samples: Vec::new(),
                    last_gpu_sample: None,
                    cadence_only: std::env::var_os("PUZZLE_UNICODE_CADENCE_ONLY").is_some(),
                    pressure: pressure::Pressure::enabled(),
                    memory_before: serde_json::Value::Null,
                    key_delay: Duration::from_millis(
                        std::env::var("PUZZLE_UNICODE_KEY_DELAY_MS")
                            .ok()
                            .and_then(|value| value.parse().ok())
                            .unwrap_or_else(|| {
                                if std::env::var_os("PUZZLE_UNICODE_PRESSURE").is_some() {
                                    12_000
                                } else {
                                    1_000
                                }
                            }),
                    ),
                })
            })
    }
    /// Receives only successful submissions from the ordinary surface path.
    pub(super) fn presented(
        &mut self,
        started: Instant,
        now: Instant,
        live: &mut super::live::LiveWindow,
    ) {
        if let Some(pressure) = &mut self.pressure {
            pressure.presented(now.duration_since(started).as_secs_f64() * 1000.0, live);
            if pressure.frozen.is_some() {
                if pressure.frozen_frames.len() == 1 {
                    // Independently read the first snapshot carrying the frozen title.
                    // A later settled screen cannot substitute for its first submitted ink.
                    self.last_gpu_sample = None;
                    self.sample_gpu(live, now);
                }
                return;
            }
        }
        // Setup frames do not belong to the producer interval; its OSC handshake
        // establishes the exact monotonic start before any Unicode output is released.
        if let Some(start) = self.started {
            self.frames
                .push(now.duration_since(start).as_secs_f64() * 1000.0);
            self.sample_gpu(live, now);
        }
    }
    /// Retains every sample and lets xtask enforce the user's unchanged bounds.
    fn finish(
        &self,
        now: Instant,
        live: &mut super::live::LiveWindow,
        period: Duration,
    ) -> Result<(), String> {
        let boundary = self.pressure.as_ref().and_then(|p| p.frozen).unwrap_or(now);
        let elapsed = boundary
            .duration_since(self.started.ok_or("Unicode start missing")?)
            .as_secs_f64();
        // Independent UTC samples include UI callback delivery and actual child receipt.
        let keys = self.key_latencies()?;
        let key_ms = keys
            .iter()
            .copied()
            .reduce(f64::max)
            .ok_or("no key latency samples")?;
        // The complete interval includes any missed submissions during cold discovery.
        // No outlier removal or warmed-only slice can turn a stall into a pass.
        let fps = self.frames.len() as f64 / elapsed;
        // Retain raw monotonic submission offsets alongside the aggregate, allowing
        // the reviewer to inspect long gaps rather than trusting only average fps.
        let mut report = serde_json::json!({
            "seconds": elapsed, "fps": fps, "key_to_child_ms": key_ms, "key_samples_ms": keys,
            "submission_ms": self.frames, "gpu_samples": self.gpu_samples,
            "memory_before_glyphs": self.memory_before,
            "memory_after_glyphs": memory::sample(),
            "pass": fps >= 30.0 && key_ms <= 50.0 && self.complete_ink(live, elapsed),
        });
        if let Some(pressure) = &self.pressure {
            let frozen = live.frozen_unicode()?;
            report["pass"] = serde_json::json!(
                fps >= 30.0
                    && key_ms <= 50.0
                    && pressure.worst_ms <= period.as_secs_f64() * 1000.0
                    && pressure.bytes <= 48 * 1024 * 1024
                    && frozen["pass"] == true
                    && self.complete_ink(live, elapsed)
            );
            report["frozen"] = frozen;
            report["frozen_ink_ms"] = serde_json::json!(pressure.ink_ms);
            report["frozen_presentations"] = serde_json::json!(pressure.frozen_frames);
            report["font_preparation_at_freeze"] = pressure.at_freeze.clone();
            report["font_preparation_final"] = live.glyph_timing();
            report["evictions"] = pressure.evictions.into();
            report["evictions_per_second"] = (pressure.evictions as f64 / elapsed).into();
            report["worst_frame_ms"] = pressure.worst_ms.into();
            report["frame_ms"] = serde_json::json!(pressure.frame_ms);
            report["vector_bytes"] = pressure.bytes.into();
        }
        report["complete_frames_pass"] =
            serde_json::json!(fps >= 30.0 && key_ms <= 50.0 && self.complete_ink(live, elapsed));
        self.diagnostics(&mut report, elapsed, live, period)?;
        // Write the private observation first. The predictable path is only the
        // latest gate summary; it must never be the sole copy of a failed run.
        let bytes = report.to_string();
        std::fs::write(self.receipts.path.with_file_name("native.json"), &bytes)
            .map_err(|e| e.to_string())?;
        std::fs::write("target/m11-unicode/native.json", bytes).map_err(|e| e.to_string())
    }
    /// Adds ordinary-store evidence after the timed interval, without preparing replacement ink.
    fn diagnostics(
        &self,
        report: &mut serde_json::Value,
        elapsed: f64,
        live: &mut super::live::LiveWindow,
        period: Duration,
    ) -> Result<(), String> {
        report["geometry"] = super::measurement_display::witness(live);
        report["cadence_only"] = self.cadence_only.into();
        report["ink_appearances"] = live.ink_observer.report(elapsed);
        report["frame_gates"] = live.frame_metrics.report();
        report["pipeline_stages"] = live.session.stage_report();
        // Retain every owner answer, including late and unchanged slots. A frame
        // which missed its build deadline must not disappear from the evidence.
        report["session_publications"] = live
            .publications
            .iter()
            .map(|timing| timing.report())
            .collect::<Vec<_>>()
            .into();
        // D-45 separates Unicode stress from reusable-glyph Flood's numerical fps floor.
        // Keep the count visible even when covered ink in successful frames is complete.
        // A later successful recovery cannot erase a failed earlier preparation turn.
        report["aborted_builds"] = live.frame_metrics.aborted_builds().into();
        report["unicode_stress_pass"] = serde_json::json!(
            self.complete_ink(live, elapsed)
                && live.frame_metrics.aborted_builds() == 0
                && report["key_to_child_ms"]
                    .as_f64()
                    .is_some_and(|ms| ms <= 50.0)
        );
        report["font_preparation_final"] = live.glyph_timing();
        let publication = live.text_context.completion_metrics().unwrap_or_default();
        report["publication"] = serde_json::json!({
            "turns":publication.turns,"ink":publication.ink,
            "milliseconds":publication.nanos as f64 / 1e6,
            "worst_ms":publication.worst_nanos as f64 / 1e6,
        });
        let (evictions, bytes) = live.text_context.vector_pressure();
        report["atlas_final"] = serde_json::json!({"evictions":evictions,"bytes":bytes});
        report["atlas_page_rows"] = puzzle_term::slug::atlas::PAGE_ROWS.into();
        report["atlas_page_pair_bytes"] = (puzzle_term::slug::atlas::PAGE_TEXELS * 20).into();
        report["refresh_budget_ms"] = (period.as_secs_f64() * 1000.0).into();
        if std::env::var_os("PUZZLE_WORKLOAD").is_some() {
            // Real command workloads also inspect retained GPU ink after their timed interval.
            report["retained_gpu"] = live.frozen_unicode()?;
        }
        Ok(())
    }
}
impl Probe {
    /// Chooses the original short completion deadline or the explicit frozen-screen mode.
    fn complete(&mut self, live: &super::live::LiveWindow, now: Instant) -> bool {
        if let Some(pressure) = &mut self.pressure {
            pressure.complete(live, now)
        } else {
            // Longer authored move replays retain the unchanged ordinary gate by default.
            // Their duration is observation input, never a scheduler or acceptance margin.
            self.started.is_some_and(|at| {
                now.duration_since(at)
                    >= Duration::from_millis(
                        std::env::var("PUZZLE_UNICODE_OBSERVE_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(5900),
                    )
            })
        }
    }
    /// Starts the fixture once through the ordinary Session text command and PTY writer.
    fn launch(&mut self, live: &mut super::live::LiveWindow, _now: Instant) {
        // Directory reservation made this journal exclusive. Never delete another
        // run's evidence, including a rejected count from an interrupted window.
        let Some(dir) = std::env::current_dir().ok() else {
            return;
        };
        // The child shell starts at home; resolve fixture paths in the host's
        // checkout before quoting, including paths containing an apostrophe.
        let script = dir
            .join(&self.script)
            .to_string_lossy()
            .replace('\'', "'\"'\"'");
        let key = dir
            .join(&self.receipts.path)
            .to_string_lossy()
            .replace('\'', "'\"'\"'");
        live.send(puzzle_term::SessionMessage::Command(
            puzzle_term::messages::Command::Text(format!(
                "stty raw -echo; python3 -u '{script}' '{key}'; stty sane\r"
            )),
            puzzle_term::Origin::User,
        ));
        // Subsequent deadlines observe the handshake instead of launching another child.
        self.launched = true;
    }
}

/// Measurement deadlines are independent of the native frame pacer.
impl Observer {
    /// Setup and key deadlines run beside the existing native presentation scheduler.
    pub(super) fn unicode_tick(
        &mut self,
        now: Instant,
        live: &mut LiveWindow,
        pacer: Option<&Pacer>,
    ) -> Option<Instant> {
        // An ordinary window has no observer and contributes no probe deadlines.
        // This branch does not request or build a frame as part of measurement setup.
        let probe = self.unicode_probe.as_mut()?;
        if now >= probe.expires {
            tracing::error!(
                event = "desktop.unicode_probe_failed",
                "Unicode fixture exceeded its finite watchdog"
            );
            live.window.close();
            return None;
        }
        // Observe the executor's actual display period; the fixture supplies no replacement clock.
        let period = pacer?.refresh_period();
        if now < probe.due {
            return Some(probe.due);
        }
        if !probe.launched {
            probe.launch(live, now);
        } else if probe.started.is_none() {
            if live.title == "unicode-ready" {
                // The process is known: this records only our application's memory.
                // Sampling precedes the timed producer release, so subprocess overhead
                // cannot become part of glyph preparation or presentation measurements.
                probe.memory_before = memory::sample();
                live.ink_observer.start();
                live.frame_metrics.start();
                // Release the waiting producer only after measurement starts, preserving
                // cold first-frame and source-discovery work in the timed interval.
                probe.started = Some(now);
                probe.schedule_key(live);
                if std::env::var_os("PUZZLE_LIFECYCLE_AT_UNICODE").is_some() {
                    live.window
                        .replay(crate::render_thread::host::HostAction::StartReplay);
                }
                live.send(puzzle_term::SessionMessage::Command(
                    puzzle_term::messages::Command::Text("g".into()),
                    puzzle_term::Origin::User,
                ));
            }
        } else if probe.complete(live, now) {
            // Finish before the producer exits or stty redraws its shell prompt.
            // An error is recorded explicitly and cannot supply a passing JSON report.
            if let Err(error) = probe.finish(now, live, period) {
                tracing::error!(event="desktop.unicode_probe_failed", %error);
            }
            live.window.close();
            return None;
        }
        probe.due = now + Duration::from_millis(20);
        Some(probe.due)
    }
}
