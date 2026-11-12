//! Observation-only native submissions for the two-surface integration.
//!
//! A shared interval is armed by the host after both children warm up. The actor
//! appends timestamps only after a real present; neither a redraw request nor a
//! received snapshot counts as a frame. Diagnostic locks protect scalar journals,
//! never terminal state, and are released before input, builds or driver calls.
//! Paired owner clocks come from the original fixed pipeline instrumentation.
use super::*;
use puzzle_embed::diagnostics::{
    session_thread::Handle,
    window::{
        live::{LiveWindow, PresentTiming},
        observer::SurfaceObserver,
        pacing::Pacer,
    },
};

/// Bounded, preallocated successful submission times and the cohort origin.
#[derive(Default)]
pub(super) struct Samples {
    /// None leaves warming presentations outside the measured denominator.
    pub(super) start: Option<Instant>,
    /// Times relative to start preserve every interior and edge gap.
    pub(super) presents: Vec<f64>,
    /// Immutable atomic counter capability, not access to the Session itself.
    pub(super) pipeline: Option<Handle>,
    /// Restart attribution belongs beside the interval rather than in an unrelated log.
    pub(super) restart: super::super::restart::Journal,
}
/// The diagnostic observer never supplies fixture input or alternative pacing.
pub(super) struct Observer {
    /// Host and actor exchange clocks without sharing any rendering resource.
    pub(super) samples: Arc<Mutex<Samples>>,
}
impl SurfaceObserver for Observer {
    /// No observer timer: actual host messages and display links drive the product.
    fn tick(
        &mut self,
        _now: Instant,
        live: &mut LiveWindow,
        _pacer: Option<&Pacer>,
    ) -> Option<Instant> {
        // The short guard protects timestamps only; it ends before the driver resumes.
        // Recovering poison preserves diagnostic data without affecting product owners.
        let mut samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        // This clone reads fixed observations after teardown; it cannot query VT.
        if samples.pipeline.is_none() {
            samples.pipeline = Some(live.session.clone());
        }
        None
    }
    /// Record queue submissions at the same boundary used by the existing cadence probe.
    fn presented(
        &mut self,
        _start: Instant,
        end: Instant,
        _prediction: Option<(Instant, Instant, u64, &'static str)>,
        _timing: &PresentTiming,
        _live: &mut LiveWindow,
    ) {
        // The short guard protects timestamps only; it ends before the driver resumes.
        // Recovering poison preserves diagnostic data without affecting product owners.
        let mut samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(start) = samples.start {
            samples
                .presents
                .push(end.duration_since(start).as_secs_f64() * 1000.0);
        }
    }
    /// Windows/Linux phase observations remain the driver's exclusive policy.
    #[cfg(not(target_os = "macos"))]
    fn calibrated(&mut self, _now: Instant) {}
    /// A macOS tick is not counted as a submitted frame.
    #[cfg(target_os = "macos")]
    fn refresh(&mut self, tick: crate::render_thread::surface::Refresh, _live: &LiveWindow) {
        let mut samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        let origin = samples.start;
        samples.restart.tick(tick.callback, origin);
    }
    /// Retain start-to-callback separately from input-to-first-present.
    fn display_restarted(&mut self, start: Instant, first: Instant) {
        let mut samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        samples.restart.restarted(start, first);
    }
}
