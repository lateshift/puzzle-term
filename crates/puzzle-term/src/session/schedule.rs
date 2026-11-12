//! Event-driven presentation deadlines for mode 2026 and blinking (§10.4).
//!
//! VT parses bytes without a clock, while the session receives a host time
//! with each feed and frame. Synchronization retains dirty grid state but
//! delays its presentation until the application resets mode 2026 or the
//! 200 ms safety limit expires. Cursor and SGR-5 blink share a 530 ms
//! deadline; no polling frame loop is needed. Fifteen seconds after the
//! last keyboard input the caret is left visible and no timer is renewed.
use super::Session;
use std::time::Duration;
use web_time::Instant;

/// xterm's cursor/text phase interval from §10.4.
const BLINK_INTERVAL: Duration = Duration::from_millis(530);
/// A stalled synchronized-output program may not hide output indefinitely.
const SYNC_TIMEOUT: Duration = Duration::from_millis(200);
/// Idle caret blinking ends to satisfy P2's no-recurring-work rule.
const BLINK_IDLE: Duration = Duration::from_secs(15);

impl Session {
    /// Records parsed damage, deferring presentation during mode 2026.
    pub(super) fn output_changed(&mut self, now: Instant) {
        self.needs_frame = true;
        // The parser already applied all bytes. Suppressing the Waker here
        // hides only an intermediate bitmap, never a partial VT state.
        if self.terminal.modes.get(true, 2026).unwrap_or(false) {
            if self.sync_started.is_none() {
                self.sync_started = Some(now);
                // One deadline suffices for every later chunk of this
                // synchronized transaction; output cannot postpone it.
                self.services.waker.wake_at(now + SYNC_TIMEOUT);
            }
        } else {
            // A reset packet may be the only new packet after a long pause;
            // it must release the dirty grid immediately.
            self.sync_started = None;
            self.services.waker.request_frame();
        }
    }

    /// Returns whether a live mode-2026 transaction still hides damage.
    pub(super) fn sync_suppresses(&mut self, now: Instant) -> bool {
        if !self.terminal.modes.get(true, 2026).unwrap_or(false) {
            self.sync_started = None;
            return false;
        }
        let Some(started) = self.sync_started else {
            // An initial frame has no partial transaction to suppress.
            return false;
        };
        if now.saturating_duration_since(started) < SYNC_TIMEOUT {
            return true;
        }
        // A timed-out presentation starts a new safety window for later
        // output while the application still holds mode 2026 enabled.
        self.sync_started = Some(now);
        false
    }

    /// Starts a visible blink phase after input or a host appearance change.
    pub(super) fn reveal_blinking_ink(&mut self, now: Instant) {
        self.last_blink_activity = Some(now);
        self.blink_deadline = None;
        let mut was_hidden = false;
        if let Some(builder) = self.frame_policy.as_mut() {
            was_hidden = !builder.blink;
            builder.blink = true;
        }
        // Input does not dirty the grid by itself. Restoring a hidden
        // cursor on either trigger requires a new presentation (P2).
        if was_hidden {
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
        // Restart the 530 ms timer even if this phase was already visible.
        self.schedule_blink(now);
    }

    /// Converts an expired timer callback into one visible frame change.
    pub(super) fn advance_blink(&mut self, now: Instant) {
        if !self.blink_deadline.is_some_and(|deadline| now >= deadline) {
            return;
        }
        self.blink_deadline = None;
        // Idle is measured from input or an appearance reset, not shell
        // output; a noisy background process must not keep it flashing.
        let active = self
            .last_blink_activity
            .is_some_and(|input| now.saturating_duration_since(input) < BLINK_IDLE);
        if let Some(builder) = self.frame_policy.as_mut() {
            // The final idle transition leaves text and cursor visible.
            // Both sources share one phase so their timed draws coalesce.
            builder.blink = if active { !builder.blink } else { true };
            self.needs_frame = true;
        }
    }

    /// Requests one future wake only while a blinking source is visible.
    pub(super) fn schedule_blink(&mut self, now: Instant) {
        self.last_blink_activity.get_or_insert(now);
        if self.blink_deadline.is_some() || !self.blink_source_visible() {
            // A queued timer remains valid through ordinary output frames;
            // registering another would produce duplicate wakeups.
            return;
        }
        let Some(input) = self.last_blink_activity else {
            return;
        };
        if now.saturating_duration_since(input) >= BLINK_IDLE {
            // Once the phase is solid, no timer resumes it until input.
            return;
        }
        // A single wake controls both cursor and SGR-5 ink; the host may
        // coalesce it with a display refresh without rendering in between.
        let deadline = (now + BLINK_INTERVAL).min(input + BLINK_IDLE);
        self.blink_deadline = Some(deadline);
        self.services.waker.wake_at(deadline);
    }

    /// Checks VT presentation state without asking the bus or GPU.
    fn blink_source_visible(&self) -> bool {
        let Some(_) = self.frame_policy.as_ref() else {
            return false;
        };
        let cursor = self.terminal.buffer().cursor();
        let caret = cursor.is_some_and(|view| {
            // The timer uses the same effective setting as drawing, so an
            // ignored DECSCUSR request cannot keep waking a steady caret.
            let mode = self.terminal.modes.get(true, 12).unwrap_or(false);
            let index = self.terminal.screen().ring.scrollback_len() + self.terminal.cursor.row;
            let first = self.viewport.top_index();
            // A caret outside retained visible content must not wake a history view.
            let visible = (first..first + self.viewport.visible_count(self.terminal.size().1))
                .contains(&index);
            view.visible && visible && self.cursor_settings.presentation(view.style, mode).blinks
        });
        caret || self.blinking_ink
    }
}
