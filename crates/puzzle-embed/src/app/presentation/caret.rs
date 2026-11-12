//! Change-only caret facts are counted at enqueue and host-tick delivery.
//!
//! These observations never request a frame, replay a fact or change admission.
//! A queued count ahead of delivery identifies an undrained host inbox; no new
//! queued fact points upstream at publication or actual contents assignment.
//! The private diagnostic target is explicit and contains coordinates, not text.
use super::SurfaceId;

/// Per-surface totals share the existing short submission-ledger guard.
#[derive(Default)]
pub(super) struct Facts {
    /// Facts accepted by the App inbox after an actual CA image assignment.
    pub(super) emitted: u64,
    /// Facts passed to the Rust host sink by App::tick.
    pub(super) delivered: u64,
    /// Closed inbox sends are separate from facts still waiting to be drained.
    failed: u64,
}

/// Counters describe delivery only; they cannot affect cursor or image state.
impl Facts {
    /// Record the enqueue outcome with the image serial that supplied the caret.
    pub(super) fn emitted(
        &mut self,
        surface: SurfaceId,
        serial: u64,
        rect: Option<[f32; 4]>,
        queued: bool,
    ) {
        // Saturate diagnostics rather than making a centuries-long run panic.
        self.emitted = self.emitted.saturating_add(u64::from(queued));
        self.failed = self.failed.saturating_add(u64::from(!queued));
        // Only changed caret notifications enter this opt-in target. Ordinary
        // blink/coalesced frames do not produce a log line or counter update.
        tracing::debug!(target: "puzzle_desktop::caret", event="embed.caret_emitted",
            ?surface, serial, ?rect, queued, emitted=self.emitted,
            delivered=self.delivered, failed=self.failed);
    }

    /// The host sink received this owned fact; native UI application is downstream.
    pub(super) fn delivered(&mut self, surface: SurfaceId, rect: Option<[f32; 4]>) {
        self.delivered = self.delivered.saturating_add(1);
        // A C host additionally records its actual callback boundary, so a
        // translation/drop can be distinguished from deferred AppKit effects.
        tracing::debug!(target: "puzzle_desktop::caret", event="embed.caret_delivered",
            ?surface, ?rect, emitted=self.emitted, delivered=self.delivered,
            failed=self.failed);
    }
}
