//! Opt-in instrumentation seam keeps authored workloads in the harness.
//! The executor emits facts after ordinary work; observers supply only their
//! own finite measurement deadlines, never display phase or replacement ink.
//!
//! Calls run exclusively on the render actor, so an observer may inspect the
//! retained frame without synchronizing with the Session owner. Its fixture
//! sends priority messages just as a host does. Readbacks are intentionally
//! included in whole-interval measurements; they cannot repair missing uploads.
//! This trait is absent unless the native diagnostic feature is selected.
//! Ordinary hosts never acquire a callback or branch per cell from this seam.
use super::{
    live::{LiveWindow, PresentTiming},
    pacing::Pacer,
};
use crate::render_thread::surface::Refresh;
use std::time::Instant;

/// Development-only hook, absent from the normal embedding build.
pub trait SurfaceObserver: Send {
    /// Advance the authored fixture and return its next explicit input deadline.
    fn tick(
        &mut self,
        now: Instant,
        live: &mut LiveWindow,
        pacer: Option<&Pacer>,
    ) -> Option<Instant>;
    /// Observe a successful real submission before a subsequent image replaces it.
    fn presented(
        &mut self,
        start: Instant,
        end: Instant,
        prediction: Option<(Instant, Instant, u64, &'static str)>,
        timing: &PresentTiming,
        live: &mut LiveWindow,
    );
    /// Record an actual idle calibration outside the measured presentation loop.
    #[cfg(not(target_os = "macos"))]
    fn calibrated(&mut self, now: Instant);
    /// Observe the tick selected for this surface without reserving another one.
    #[cfg(target_os = "macos")]
    fn refresh(&mut self, tick: Refresh, live: &LiveWindow);
    /// Retain native restart clocks without changing subscription or frame policy.
    fn display_restarted(&mut self, _start: Instant, _first: Instant) {}
}
