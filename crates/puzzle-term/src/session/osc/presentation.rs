//! Presented pointer UI effects finish after the overlay callback releases source.
//! The callback can request navigation or a dirty publication, never write keys.
//! A model callback has already ended before a scroll command is submitted.
//! No source pointer message has a wire decoder or a route from PTY output.
//! Named key actions stay inside their original bus cascade instead.
use super::super::Session;
use crate::osc::OscUiEffect;
use web_time::Instant;

impl Session {
    /// UI callbacks request one normal viewport operation, preserving scroll policy.
    pub(in crate::session) fn apply_osc_ui_effect(
        &mut self,
        effect: Option<OscUiEffect>,
        now: Instant,
    ) {
        // Unknown or stale presented tokens produce no mutation or wake.
        let Some(effect) = effect else { return };
        // Pointer outcomes use the same extension channel as named user actions.
        if let Some(event) = effect.event {
            self.bus.publish(crate::messages::Event::Ext(event));
        }
        // One dirty request shares ordinary output pacing; it starts no animation.
        if effect.changed {
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
        if let Some(line) = effect.scroll_to {
            // Queue through interceptors; a host may veto or rewrite navigation.
            self.submit(
                crate::messages::Command::ScrollTo(crate::viewport::ScrollTarget::Line(line)),
                crate::Origin::User,
            );
            // Pointer dispatch is outside a bus drain. Named actions instead
            // enqueue their scroll inside the current cascade (commands/bindings).
            self.pump(now);
        }
    }
}
