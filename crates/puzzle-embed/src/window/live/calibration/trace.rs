//! One-turn idle calibration trace for comparison with the 300-frame FIFO burst.
//! Samples are retained without logging until the attempt completes, so output
//! formatting cannot release drawables between acquisitions under investigation.
use std::time::{Duration, Instant};

/// Bounded development observation, absent from normal presentation builds.
pub struct Trace {
    /// A shared monotonic origin makes successive acquire/present gaps comparable.
    epoch: Instant,
    /// One tuple per actual drawable acquisition, submission and presentation.
    samples: Vec<(f64, f64, f64, f64, Option<u32>, usize)>,
}

impl Trace {
    /// Reserves the four records needed by the largest approved latency experiment.
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            samples: Vec::with_capacity(4),
        }
    }

    /// Retains acquire start/end and render-submit/present-return, all in ms.
    pub fn push(
        &mut self,
        acquired: Instant,
        blocked: Duration,
        submitted: Instant,
        presented: Instant,
        draws: Option<u32>,
        uploads: usize,
    ) {
        // The acquire duration was sampled around precisely this API call.
        let ms = |at: Instant| at.duration_since(self.epoch).as_secs_f64() * 1000.0;
        self.samples.push((
            // Reconstruct call entry from its precisely measured wait duration.
            ms(acquired - blocked),
            // A qualifying completion, rather than submit, supplies refresh phase.
            ms(acquired),
            // This is CPU queue submission, with no GPU completion fence.
            ms(submitted),
            // Return from present can immediately permit another acquire.
            ms(presented),
            // Nonzero draws distinguish real retained rendering from empty work.
            draws,
            // Retained atlas upload records are separate from instance-buffer writes.
            uploads,
        ));
    }
}

impl Drop for Trace {
    /// Emits after the whole turn, including early completion at a blocking acquire.
    fn drop(&mut self) {
        tracing::debug!(event="desktop.calibration_trace", ?self.samples,
            "tuples: acquire-start/end, render-submit, present-return, draws, uploads");
    }
}
