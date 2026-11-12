//! Authored developer workloads attach to the embedding executor's observation seam.
//!
//! This adapter is compiled only with the harness's measurement feature. The
//! terminal executor owns pacing, snapshots and submission; these fixtures own
//! scripted inputs, private receipt files and reports. Keeping the two owners
//! separate lets a library host use the executor without environment-driven
//! benchmark programs or filesystem journals.
//!
//! Scroll and Unicode schedules may be enabled independently. Each keeps its
//! existing next-input deadline, and the adapter returns the earlier deadline
//! so neither fixture can postpone the other's input. These deadlines wake the
//! actor for fixture work; they do not replace a display-link opportunity.
//!
//! Presentation observations happen only after successful real submission.
//! Failed builds and skipped presentations cannot inflate the reported rate.
//! The Unicode observer may read the retained image on that same actor, while
//! scroll observations retain the acquisition and prediction timestamps.
//!
//! Drag replay advances from the selected display's actual tick. Posting its
//! host action leaves winit window operations on the main thread, including
//! programmatic moves that produce no AppKit live-resize notification.
use super::{
    live::{LiveWindow, PresentTiming},
    pacing::Pacer,
};
use puzzle_embed::diagnostics::window::observer::SurfaceObserver;
use std::time::Instant;

/// Finite workloads own their private journals and input schedules in the harness.
pub(super) struct Observer {
    /// Existing scroll/keyecho comparison fixture, enabled explicitly by environment.
    pub(super) scroll_probe: Option<super::scroll_probe::ScrollProbe>,
    /// Existing cold/steady Unicode fixture, never enabled by an ordinary host.
    pub(super) unicode_probe: Option<super::unicode_probe::Probe>,
}
impl Observer {
    /// Reserve each enabled fixture's own receipt and evidence ownership once.
    pub(super) fn new() -> Self {
        Self {
            scroll_probe: super::scroll_probe::ScrollProbe::enabled(),
            unicode_probe: super::unicode_probe::Probe::enabled(),
        }
    }
}
impl SurfaceObserver for Observer {
    /// Both finite input schedules retain their independent deadlines.
    fn tick(
        &mut self,
        now: Instant,
        live: &mut LiveWindow,
        pacer: Option<&Pacer>,
    ) -> Option<Instant> {
        [
            self.scroll_probe_tick(now, live),
            self.unicode_tick(now, live, pacer),
        ]
        .into_iter()
        .flatten()
        .min()
    }
    /// Count only successfully submitted frames, preserving complete interval evidence.
    fn presented(
        &mut self,
        start: Instant,
        end: Instant,
        prediction: Option<(Instant, Instant, u64, &'static str)>,
        timing: &PresentTiming,
        live: &mut LiveWindow,
    ) {
        if let Some(probe) = &mut self.unicode_probe {
            probe.presented(start, end, live);
        }
        if let Some(probe) = &mut self.scroll_probe {
            probe.presented(start, end, prediction, timing);
        }
    }
    /// Calibration observations do not offer alternative phase samples.
    #[cfg(not(target_os = "macos"))]
    fn calibrated(&mut self, now: Instant) {
        if let Some(probe) = &mut self.scroll_probe {
            probe.calibrated(now);
        }
    }
    /// A dragged replay advances on the actual target display's used tick.
    #[cfg(target_os = "macos")]
    fn refresh(&mut self, tick: crate::render_thread::surface::Refresh, live: &LiveWindow) {
        if let Some(probe) = &mut self.scroll_probe {
            probe.restart.tick(tick.callback, probe.started);
        }
        if std::env::var_os("PUZZLE_LIFECYCLE_AT_UNICODE").is_some() {
            live.window
                .replay(crate::render_thread::host::HostAction::ReplayTick {
                    display: tick.display,
                    index: tick.index,
                });
        }
    }
    /// A start receipt preserves its exact first callback even if later ticks coalesce.
    fn display_restarted(&mut self, start: Instant, first: Instant) {
        if let Some(probe) = &mut self.scroll_probe {
            probe.restart.restarted(start, first);
        }
    }
}
