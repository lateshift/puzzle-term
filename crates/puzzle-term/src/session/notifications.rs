//! Session-owner notification feedback and dirty-only toast publication (D-50).
//! Host callbacks are messages; neither the renderer nor an OS callback writes replies.
//!
//! Opt-in records host intent only; the first admitted notification registers
//! the painter. A terminal that never notifies has no extra overlay callback.
//! Once registered, the painter consumes an Arc source that changes only on
//! admission, replacement, dismissal or reset. Ordinary snapshots share it.
//! Feedback removes exactly one generation and flushes protocol replies through
//! the same transport queue used by parser queries and encoded keys.
use super::Session;
use crate::notification::Feedback;
use puzzle_overlay::provider::EventKinds;
/// Notification controls execute only on the Session owner.
impl Session {
    /// Cold host namespace distinguishes tokens from two simultaneous surfaces.
    pub fn set_notification_namespace(&mut self, namespace: u64) {
        self.notifications.namespace(namespace);
    }
    /// Installs a harness-only view through the same public split-overlay API.
    pub fn enable_notification_toasts(&mut self) {
        // Opt-in is idempotent and does not register a painter before first delivery.
        if self.notifications.toasts {
            return;
        }
        // Enabling twice cannot add duplicate providers or duplicate hit regions.
        self.notifications.toasts = true;
        self.notifications.dirty = true;
        self.refresh_notification_toasts();
    }
    /// Replies are appended after all earlier owner work, then flushed normally.
    pub fn notification_feedback(&mut self, token: &str, outcome: Feedback) {
        self.notifications.feedback(
            token,
            outcome,
            self.services.notifier.as_mut(),
            &mut self.terminal.replies,
        );
        self.refresh_notification_toasts();
        self.flush_clipboard_replies();
    }
    /// No table walk or text copy unless notification state actually changed.
    pub(super) fn refresh_notification_toasts(&mut self) {
        if !std::mem::take(&mut self.notifications.dirty) || !self.notifications.toasts {
            return;
        }
        // Source creation is below the dirty gate, never on the steady frame path.
        let source = self.notifications.toast_source();
        if source.is_none() && !self.notifications.registered {
            return;
        }
        if !self.notifications.registered {
            // Registration waits for actual admitted output, so ordinary terminal use
            // has no notification view, hit regions, allocations or per-frame callback.
            self.overlays_mut().add(
                Box::<puzzle_overlay::builtins::toast::Toasts>::default(),
                Box::new(puzzle_overlay::builtins::toast::ToastsView),
            );
            self.notifications.registered = true;
        }
        self.overlay_source.models.toasts = source;
        self.overlay_changed(EventKinds::STATUS);
        self.needs_frame = true;
        self.services.waker.request_frame();
    }
}
