//! Wheel routing samples live application modes after bus policy (§7.4).
//!
//! Shift always chooses local history. Otherwise mouse reports take priority
//! over alternate-scroll arrow keys; neither path waits for a display frame.
//! Local motion retains hardware units so trackpad inertia is not animated twice.
//!
//! Protocol conversion has a bounded fractional-row ledger. Switching screens,
//! tracking encodings or DECCKM discards its fraction before a new step can
//! reach a different destination. A local Shift gesture clears it as well.
//! Pixel input is divided by the same device-cell height used for rendering;
//! the user multiplier applies only to local history, not application reports.
//! Mouse reports retain pointer modifiers and coordinates. Alternate-scroll
//! keys are unmodified legacy arrows, as mode 1007 promises, rather than the
//! kitty representation of a physical key which the user never pressed.
//! An exited Session can still inspect its retained history, but protocol
//! steps cannot reopen the closed ordered input transport.
use super::Handler;
use crate::{
    messages::{Command, Event},
    session::wheel::Delivery,
    viewport::ScrollInput,
    vt::input::{InputModes, Key, KeyEvent, MouseEvent, MouseKind, encode_key, encode_mouse},
};
use puzzle_bus::hooks::HookCtx;
/// The shared command handler owns routing; platform adapters supply only input.
impl Handler<'_> {
    /// Routes one accepted wheel callback without fabricating keyboard modifiers.
    /// `delta` is device pixels or rows, positive toward newer text.
    /// `pointer` supplies report coordinates and the Shift routing override.
    /// `ctx` publishes local position changes after applying the viewport clamp.
    pub(super) fn wheel_command(
        &mut self,
        delta: ScrollInput,
        pointer: MouseEvent,
        ctx: &mut HookCtx<'_, Command, Event>,
    ) {
        let route = crate::session::wheel::delivery(self.terminal, pointer);
        let Some(route) = route else {
            // A local gesture starts a separate ledger and preserves pixel/notch units.
            self.wheel.clear();
            // Horizontal gestures have no local or alternate-arrow destination.
            if pointer.button >= 66 {
                return;
            }
            let before = (self.viewport.scroll_px(), self.viewport.new_output());
            self.viewport.wheel(self.terminal, delta, self.now);
            // Observers receive the accepted clamp, which may be a boundary no-op.
            self.viewport_changed(before, ctx);
            if self.viewport.animation_frame(self.now) {
                *self.needs_frame = true;
                self.services.waker.request_frame();
            }
            return;
        };
        // EOF keeps history usable but must never write more application reports.
        if self.exited.is_some() {
            return;
        }
        let steps = self.wheel.take(
            delta,
            self.viewport.row_height(),
            self.terminal.screen_epoch,
            route,
        );
        // At most 64 complete rows leave one callback; fractions stay in Steps.
        for _ in 0..steps.unsigned_abs() {
            let bytes = self.wheel_bytes(route, steps, pointer);
            // The ordered transport gets each complete step before pump returns.
            if !bytes.is_empty()
                && let Err(error) = self.write_input(&bytes)
            {
                tracing::warn!(event="session.input_write_failed", %error);
            }
        }
    }
    /// Encodes one complete application step using its selected live destination.
    fn wheel_bytes(&self, route: Delivery, steps: i32, pointer: MouseEvent) -> Vec<u8> {
        // Signed local deltas use down-positive; xterm wheel-up is button 64.
        let mut pointer = pointer;
        if self.viewport.projected() {
            let y = (i64::from(pointer.pixel_y) - i64::from(self.viewport.device_shift())).max(0);
            pointer.row = (y / i64::from(self.viewport.row_height().max(1))) as u32;
        }
        let Some(pointer) = self.application_pointer(pointer) else {
            return Vec::new();
        };
        match route {
            Delivery::Mouse(tracking, encoding, horizontal) => encode_mouse(
                MouseEvent {
                    kind: MouseKind::Wheel,
                    button: (if horizontal { 66 } else { 64 }) + u16::from(steps > 0),
                    ..pointer
                },
                tracking,
                encoding,
                // Shift was routed locally above; suppressing it here is redundant.
                false,
            ),
            Delivery::Keys(_) => {
                // Mode 1007 promises xterm arrow forms, even with kitty negotiated.
                let mut modes = InputModes::from_terminal(self.terminal);
                // DECCKM remains intact while disabling only kitty's key vocabulary.
                modes.kitty_flags = 0;
                encode_key(
                    &KeyEvent::press(if steps < 0 { Key::Up } else { Key::Down }),
                    modes,
                )
            }
        }
    }
}
