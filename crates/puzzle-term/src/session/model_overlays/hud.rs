//! Opt-in frame observation feeds a debug painter without creating an idle loop.
//!
//! The bus owns the observer and its one-value outbox. Session drains it after
//! presentation and freezes its counters for the next naturally requested frame.
//! Measuring a completed frame never requests another frame just to measure it.
//! Hidden HUDs have no registration, clock sampling or retained event payload.
//!
//! Frame building and parser ownership are separated on desktop (D-50).
//! The presenter sends scalar counters back as a message; this owner adds
//! the parser byte rate using its own monotonically consumed-byte total.
//! The preceding sample, not transport arrival, defines that rate interval.
//! A host subscriber can observe statistics without showing the panel.
//! Removing the built-in observer leaves any host subscription untouched.
//! Consequently the snapshot flag always follows the bus's complete mask.
use super::*;
use crate::messages::{Command, Event, FrameStats};
use puzzle_bus::hooks::{EventKinds as BusKinds, HookCtx, HookId, Observer};

/// One completed observation, consumed before another presentation can arrive.
#[derive(Default)]
struct Latest(Option<FrameStats>);
/// The HUD uses the same public observer seam as a host statistics subscriber.
impl Observer<Command, Event> for Latest {
    /// Copies only bounded scalar counters; no per-frame heap storage is needed.
    fn observe(&mut self, event: &Event, _: &mut HookCtx<'_, Command, Event>) {
        if let Event::FrameRendered(stats) = event {
            self.0 = Some(*stats);
        }
    }
    /// Moves observation back to the Session owner after dispatch has finished.
    fn take_event(&mut self) -> Option<Event> {
        self.0.take().map(Event::FrameRendered)
    }
}
/// Debug visibility, the bus handle and the previous parser-rate boundary.
#[derive(Default)]
pub(in crate::session) struct Hud {
    /// Frozen painter source; no renderer cache or transport is retained here.
    pub value: puzzle_overlay::models::hud::Hud,
    /// Removed on hide so ordinary frames don't sample statistics clocks.
    observer: Option<HookId>,
    /// Bytes really consumed since the previous observed presentation.
    previous: Option<(Instant, u64)>,
}
/// Statistics are presentation facts and cannot replace scheduling policy.
impl Session {
    /// Explicit debug-host toggle; registration failures leave the HUD disabled.
    pub fn set_performance_hud(&mut self, shown: bool) -> Result<(), String> {
        if shown == self.model_overlays.hud.value.shown {
            return Ok(());
        }
        let hud = &mut self.model_overlays.hud;
        if shown {
            // Registration happens once per visibility edge, not during a frame build.
            hud.observer = Some(self.bus.observe(
                BusKinds::at(16),
                0,
                "performance-hud",
                Latest::default(),
            )?);
            hud.previous = self.host_time.map(|now| (now, self.parsed_total));
        } else if let Some(id) = hud.observer.take() {
            self.bus.remove(id);
            hud.previous = None;
        }
        // Visibility changes paint once; statistics updates deliberately do not.
        hud.value.shown = shown;
        self.overlay_changed(EventKinds::STATUS);
        self.needs_frame = true;
        self.services.waker.request_frame();
        Ok(())
    }
    /// Publishes a successfully submitted frame, then drains the HUD's public observer.
    pub(in crate::session) fn frame_statistics(&mut self, mut stats: FrameStats, now: Instant) {
        // The subscription may have been removed since the renderer sealed its request.
        if !self.bus.observes(BusKinds::at(16)) {
            return;
        }
        let hud = &mut self.model_overlays.hud;
        if let Some((previous, bytes)) = hud.previous {
            let seconds = now.saturating_duration_since(previous).as_secs_f64();
            if seconds > 0.0 {
                stats.parse_bytes_per_second =
                    self.parsed_total.saturating_sub(bytes) as f64 / seconds;
            }
        }
        // Keep the next rate boundary even when only an external observer is present.
        hud.previous = Some((now, self.parsed_total));
        self.bus.publish(Event::FrameRendered(stats));
        self.pump(now);
        if let Some(id) = self.model_overlays.hud.observer
            && let Some(Event::FrameRendered(stats)) = self.bus.take_observed(id)
        {
            let value = &mut self.model_overlays.hud.value;
            value.cpu_ms = stats.total_cpu_ms();
            value.parse_bytes_per_second = stats.parse_bytes_per_second;
            value.instances = stats.instances;
            value.glyph_bytes = stats.glyph_bytes;
            value.upload_bytes = stats.upload_bytes;
            // Dirty data waits for the next real output/input frame. No wake is issued.
            self.overlay_changed(EventKinds::STATUS);
        }
    }
}
