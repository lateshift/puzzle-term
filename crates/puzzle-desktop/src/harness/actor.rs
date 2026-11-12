//! Headless scenarios execute on the same render actor as native layers (D-41).
//!
//! Existing golden scenarios exercise ownership transfer and worker teardown
//! without introducing references or a separate test-only GPU executor.
//! The caller waits for one finite scenario; interactive input uses its own
//! native owner and never enters this headless wait.
use crate::render_thread::{Driver, RenderThread, surface::Pending};
use std::{sync::mpsc, time::Instant};

/// One finite scenario owns its text and offscreen GPU resources.
struct Scenario {
    /// Owned path survives transfer onto the process worker.
    path: String,
    /// Script failures return to the existing command-line error boundary.
    result: mpsc::Sender<Result<(), String>>,
}

/// Scenario steps run serially on the worker, using the existing dispatcher.
impl Driver for Scenario {
    /// Runs once; the caller removes the completed owner before joining.
    fn advance(&mut self, _updates: Pending, _now: Instant) -> Option<Instant> {
        let _ = self.result.send(super::run_inline(Some(&self.path)));
        None
    }
}

/// Transfers the deterministic harness through the native surface actor seam.
pub(crate) fn run(scenario: Option<&str>) -> Result<(), String> {
    let path = scenario
        .ok_or("usage: puzzle-desktop harness <scenario.pzs>")?
        .to_owned();
    let (result, received) = mpsc::channel();
    let mut actor = RenderThread::start()?;
    // The worker owns the GPU through every script step and its destruction.
    let (id, _) = actor.add(Box::new(Scenario { path, result }))?;
    let outcome = received.recv().map_err(|e| e.to_string())?;
    actor.remove(id);
    // Joining after result delivery releases the retained worker and resources.
    actor.shutdown();
    outcome
}
