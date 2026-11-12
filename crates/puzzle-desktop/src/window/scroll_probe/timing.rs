//! Per-frame CPU and deadline evidence for native cadence diagnosis.
//!
//! Samples copy scalars during the run; JSON formatting waits until completion.
//! Build includes row shaping and glyph lookup. A warmed frame that prepares zero
//! new glyphs has zero outline-preparation cost; cold build cost is an upper bound.
//! Upload CPU includes atlas and instance queue writes, without a GPU completion wait.
//! Unknown-phase cadence slots are spacing reservations, not predicted refreshes:
//! their prediction and index stay null in evidence. Submission and present return
//! retain separate clocks to distinguish queue work from handing off a drawable.
//! Calibration times identify idle attempts before or between measured frames.
//! Frame-work values are emitted by the embedding executor after actual submission.
//! This host serializes those observations without changing the frame clock.
use super::super::live::PresentTiming;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Probe reports use the executor's exact phase facts.
pub(in crate::window) use puzzle_embed::diagnostics::window::frame_work::FrameWork;

/// Monotonic instants preserve the relationship between deadline, acquire and submit.
pub(super) struct Sample {
    /// Actual entry into the late-snapshot path.
    start: Instant,
    /// Command submission completed before handing the drawable to the surface.
    submitted: Instant,
    /// Successful native submission/presentation call completed here.
    end: Instant,
    /// Chosen snapshot time and corresponding D-33 refresh prediction.
    prediction: Option<(Instant, Instant, u64, &'static str)>,
    /// Acquisition completion supplies phase only when its wait exceeds one ms.
    acquired: Instant,
    /// CPU time waiting for a reusable Fifo surface texture.
    blocked: Duration,
    /// CPU work from the live builder and renderer, separate from acquisition.
    work: FrameWork,
}

impl Sample {
    /// Copies a completed frame's timing metadata without allocating.
    pub(super) fn new(
        start: Instant,
        end: Instant,
        prediction: Option<(Instant, Instant, u64, &'static str)>,
        timing: &PresentTiming,
    ) -> Self {
        Self {
            start,
            end,
            prediction,
            acquired: timing.acquired,
            submitted: timing.submitted,
            blocked: timing.blocked,
            work: timing.work,
        }
    }

    /// Formats all phases after the interval, including calibration since last submit.
    pub(super) fn json(&self, origin: Instant, calibrations: &[Instant]) -> Value {
        // Predictions can precede the recording origin, so retain signed differences.
        let ms = |at: Instant| {
            if at >= origin {
                at.duration_since(origin).as_secs_f64() * 1000.0
            } else {
                -origin.duration_since(at).as_secs_f64() * 1000.0
            }
        };
        let cpu = |duration: Duration| duration.as_secs_f64() * 1000.0;
        json!({
            "start_ms":ms(self.start), "submitted_ms":ms(self.submitted), "present_ms":ms(self.end),
            "snapshot_deadline_ms":self.prediction.map(|p| ms(p.0)),
            "predicted_refresh_ms":self.prediction.filter(|p| p.3 == "known").map(|p| ms(p.1)),
            "cadence_slot_ms":self.prediction.map(|p| ms(p.1)),
            "refresh_index":self.prediction.filter(|p| p.3 == "known").map(|p| p.2),
            "phase_status":self.prediction.map(|p| p.3),
            "acquired_ms":ms(self.acquired), "acquire_wait_ms":cpu(self.blocked),
            "build_phase_ms":self.work.build_phases.map(cpu), "rebuilt_rows":self.work.rebuilt_rows,
            "build_cpu_ms":cpu(self.work.build), "new_glyphs":self.work.glyphs,
            "glyph_prepare_ms":(self.work.glyphs == 0).then_some(0.0),
            "glyph_prepare_upper_bound_ms":cpu(self.work.build),
            "glyph_upload_records":self.work.uploads, "upload_cpu_ms":cpu(self.work.upload_cpu),
            "upload_submit_ms":cpu(self.work.submit_cpu),
            "frame_cpu_ms":cpu(self.end.duration_since(self.start)),
            "calibration_ms":calibrations.iter().copied().filter(|at| *at <= self.end).map(ms).collect::<Vec<_>>()
        })
    }
}
