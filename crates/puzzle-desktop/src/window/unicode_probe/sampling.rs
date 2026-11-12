//! Independent diagnostic input timing and sampled first-submission GPU evidence.
//!
//! This observer must not schedule a key on the renderer: a slow glyph build
//! would then postpone the simulated physical key before its timestamp existed.
//! One finite timer posts the recorded native callbacks to the UI thread.
use super::*;
use crate::window::live::UnicodePressure;

/// Evidence helpers observe the real actor while independently exercising UI input.
impl Probe {
    /// Starts a finite input timer at producer release, independently of rendering.
    ///
    /// The copied host handle posts events and never lends the render-owned window.
    /// Pressure mode samples throughout thirty seconds; workloads send one chord.
    /// All timestamps precede UI delivery, so a busy renderer cannot hide waiting.
    /// This timer exists only for opt-in evidence and terminates after its last key.
    pub(super) fn schedule_key(&mut self, live: &super::super::live::LiveWindow) {
        // Host actions reach the same UI adapter as recorded native callbacks.
        let host = live.window.clone();
        // A compact final stamp preserves the existing single-key report contract.
        let issued = Arc::clone(&self.issued_ns);
        let delays = self.key_delays();
        // Dropping the probe disconnects this wait, cancelling all later keys.
        let cancelled = self.receipts.timer();

        // The small diagnostic timestamp list is independent of all font ownership.
        let samples = Arc::clone(&self.issued_samples);
        // Chords come from the existing native sequence catalog, including modifiers.
        let chord = std::env::var("PUZZLE_UNICODE_KEY").unwrap_or_else(|_| "ctrl+a".into());
        // Spawn failure remains visible and eventually fails the required receipt count.
        let result = std::thread::Builder::new()
            .name("puzzle-probe-key".into())
            .spawn(move || {
                // Anchor every key to producer release rather than the previous key.
                let origin = Instant::now();
                // Include early, middle and late pressure; no slow interval is skipped.
                for millis in delays {
                    // Rendering cannot delay this wait or the physical-key timestamp.
                    if !super::receipts::wait(
                        &cancelled,
                        (origin + Duration::from_millis(millis))
                            .saturating_duration_since(Instant::now()),
                    ) {
                        return;
                    }
                    // Invalid evidence chords cannot fabricate a successful latency.
                    if let Ok((events, _)) = crate::keyboard::sequences::for_chord(&chord) {
                        // Both parent and child use epoch nanoseconds across processes.
                        // Clock conversion failure becomes a missing timestamp, never a fast key.
                        let ns = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .and_then(|value| u64::try_from(value.as_nanos()).ok())
                            .unwrap_or(0);
                        // This includes event-loop delivery, encoding and actual child receipt.
                        if let Ok(mut timestamps) = samples.lock() {
                            timestamps.push(u128::from(ns));
                        }
                        // Publish the timestamp before posting the first event to the UI.
                        issued.store(ns, Ordering::Release);
                        // Key press/release and modifier restoration retain recorded order.
                        for event in events {
                            host.input(event);
                        }
                    }
                }
            });
        if let Err(error) = result {
            tracing::error!(event = "desktop.unicode_probe_failed", %error);
        }
    }
    /// A missing timer timestamp fails the fixture; it never becomes zero latency.
    /// Acquire pairs with the independent timer's release store.
    pub(super) fn issued_timestamp(&self) -> Result<u128, String> {
        match self.issued_ns.load(Ordering::Acquire) {
            0 => Err("Unicode key was never issued".into()),
            value => Ok(u128::from(value)),
        }
    }
    /// Pairs every pressure key with independent child receipt, rejecting missing samples.
    ///
    /// The sidecar is a complete ordered history, while ordinary workload producers
    /// keep the earlier one-integer file protocol. Neither format permits omission
    /// of a late key. Reversed wall-clock samples fail instead of clamping to zero.
    pub(super) fn key_latencies(&self) -> Result<Vec<f64>, String> {
        // Hold only diagnostic timestamps while pairing, never Session or glyph state.
        let sent = self
            .issued_samples
            .lock()
            .map_err(|_| "key timestamp owner poisoned")?;
        // Seal before opening the child sidecar: a missing receipt still keeps
        // every parent timestamp and cannot become another untraceable count failure.
        self.receipts.save_issued(&sent)?;
        // Parsing happens after observation, outside all measured frame phases.
        let received: Vec<u128> = if self.pressure.is_some()
            || std::env::var_os("PUZZLE_UNICODE_KEY_TIMES_MS").is_some()
        {
            let bytes = std::fs::read(self.receipts.path.with_extension("txt.samples"))
                .map_err(|e| format!("{}: {e}", self.receipts.path.display()))?;
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?
        } else {
            vec![
                std::fs::read_to_string(&self.receipts.path)
                    .map_err(|e| e.to_string())?
                    .trim()
                    .parse::<u128>()
                    .map_err(|e| e.to_string())?,
            ]
        };
        // Exact sample counts catch lost or duplicated keys as well as stale evidence.
        let expected = self.key_delays().len();
        self.issued_timestamp()?;
        // FIFO pairing measures each actual child receipt, not the final file write.
        self.receipts.pair(&sent, &received, expected)
    }

    /// Samples the first nonempty source image and every two seconds thereafter.
    ///
    /// This deterministic cadence uses successful presentation ordinals, including
    /// the first cold image. It never selects a frame because its ink looks ready.
    /// Readback failure is retained alongside passing samples in the raw report.
    pub(super) fn sample_gpu(&mut self, live: &mut super::super::live::LiveWindow, now: Instant) {
        if self.cadence_only
            || live.glyph_status.is_none_or(|status| status[0] == 0)
            || self
                .last_gpu_sample
                .is_some_and(|at| now.duration_since(at) < Duration::from_secs(2))
        {
            return;
        }
        // Stamp even an error so repeated failures do not turn into a polling loop.
        self.last_gpu_sample = Some(now);
        // This readback applies no uploads or fresh glyph preparation to the retained executor.
        let started = Instant::now();
        match live.frozen_unicode() {
            Ok(mut sample) => {
                // Associate GPU crops with the same ordinal as the CPU readiness series.
                // Observer work is separate from ordinary build/present CPU.
                sample["observer_ms"] = (started.elapsed().as_secs_f64() * 1000.0).into();
                sample["start_ms"] = self
                    .started
                    .map_or(0.0, |at| started.duration_since(at).as_secs_f64() * 1000.0)
                    .into();
                sample["presentation"] = live.ink_observer.frame.into();
                self.gpu_samples.push(sample);
            }
            Err(error) => {
                // Keep failed samples; a later complete image cannot erase them.
                self.gpu_samples
                    .push(serde_json::json!({"pass":false,"error":error}));
            }
        }
    }
    /// Complete CPU handles in every frame and independent sampled GPU ink are required.
    /// Empty series fail: successful process exit alone proves no rendered behavior.
    pub(super) fn complete_ink(&self, live: &super::super::live::LiveWindow, elapsed: f64) -> bool {
        // CPU evidence covers every frame; GPU evidence checks retained executor pixels.
        let ink = live.ink_observer.report(elapsed);
        !self.gpu_samples.is_empty()
            && self.gpu_samples.iter().all(|sample| sample["pass"] == true)
            && ink["missing_per_frame"].as_array().is_some_and(|frames| {
                !frames.is_empty() && frames.iter().all(|missing| *missing == 0)
            })
    }
}

/// Caller-authored replay observations leave all default acceptance samples intact.
impl Probe {
    /// Selects an explicit finite key timetable or the unchanged short/pressure gates.
    fn key_delays(&self) -> Vec<u64> {
        if let Ok(times) = std::env::var("PUZZLE_UNICODE_KEY_TIMES_MS") {
            // A malformed timetable yields no passing receipt instead of a silent fallback.
            return times
                .split(',')
                .map(str::parse)
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_default();
        }
        if self.pressure.is_some() {
            vec![1000, 4000, 8000, 12000, 20000, 28000]
        } else {
            vec![self.key_delay.as_millis() as u64]
        }
    }
}
