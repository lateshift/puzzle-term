//! Accepted pointer gestures route to local Select or application Mouse (§6.9).
//!
//! Tracking is sampled at dispatch. Shift chooses selection only under the
//! shared Shift-capture policy; outside distance never enters protocol bytes.
//! Generated Select/Copy/Mouse requests visit their own interceptor chains.
//!
//! Pointer policy and selection policy are separate bus boundaries. This handler
//! owns gesture intent, but never directly installs a highlight or writes copy.
//! A host may veto the generated Select request while preserving its pointer
//! routing policy. Copy-on-select queues after the final release selection, so
//! the FIFO cascade extracts the final accepted cells rather than an older hit.
//!
//! Tracking routes to the existing Mouse command, retaining report interception.
//! MouseKind alone is insufficient: Shift is a user policy, and click count is
//! local selection intent. All three are sampled with current application modes.
//! A mode change cancels local drag ownership before producing a protocol event.
//! This prevents a stale outside sample from continuing to schedule frames after
//! an application has claimed pointer input. Hover still updates the semantic
//! pointer cell used by the select-command-output action, without copying text.

use super::Handler;
use crate::{
    messages::{Command, Event},
    pointer::{Gesture, PointerInput},
    selection::CopyTarget,
    vt::{
        input::{Modifiers, MouseKind, MouseModes, MouseTracking},
        view::Format,
    },
};
use puzzle_bus::{Origin, hooks::HookCtx};

/// Pointer state is borrowed from Session, just like its selection and viewport.
impl Handler<'_> {
    /// Routes one coalesced sample and keeps the semantic pointer hit current.
    pub(super) fn pointer(
        &mut self,
        mut input: PointerInput,
        ctx: &mut HookCtx<'_, Command, Event>,
    ) {
        // The active builder wins over stale host geometry following a resize.
        if let Some(builder) = self.frame_policy.as_ref() {
            let metrics = builder.metrics;
            input.cell = [metrics.width_px, metrics.height_px];
        }
        if input.cell.contains(&0) || input.position.iter().any(|n| !n.is_finite()) {
            return;
        }
        let Some(point) = Gesture::hit(input, self.terminal, self.viewport) else {
            return;
        };
        // Semantic output actions can use this hit even when no drag is active.
        *self.selection_pointer = Some(point);
        let modes = MouseModes::from_terminal(self.terminal);
        // The request is sampled here because parser input may precede this message.
        let local = modes.tracking == MouseTracking::None
            || (!self
                .mouse_shift_capture
                .captures(self.terminal.mouse_shift_capture)
                && (input.event.modifiers.0 & Modifiers::SHIFT != 0));
        let origin = ctx.origin().unwrap_or(Origin::User);
        if !local {
            if self.viewport.projected() {
                // Raw points include fractional clipping; Mouse then names the
                // displayed row consistently with this accepted geometry hit.
                input.event.row = ((input.position[1].max(0.0)
                    - f64::from(self.viewport.device_shift()))
                    / f64::from(input.cell[1]))
                .floor() as u32;
            }
            // A tracking change cannot leave an old outside-drag refresh loop running.
            if self.pointer.cancel(self.terminal) {
                // Mode/Shift ownership changes settle the last local displacement.
                self.viewport.finish_drag(self.terminal, self.now);
            }
            ctx.submit(Command::Mouse(input.event), origin);
            return;
        }
        // Middle/right reports do not create a local selection gesture.
        if input.event.button != 0 {
            return;
        }
        // Release applies the final position before ending drag ownership.
        let request = match input.event.kind {
            MouseKind::Press => Some(self.pointer.press(
                self.terminal,
                point,
                input,
                self.selection.range(),
                self.now,
            )),
            MouseKind::Move | MouseKind::Release => {
                self.pointer.moved(self.terminal, point, input, self.now)
            }
            MouseKind::Wheel => None,
        };
        if let Some(request) = request {
            ctx.submit(Command::Select(request), origin);
        }
        if input.event.kind == MouseKind::Release {
            let copy = self.pointer.release();
            // Any fractional edge displacement settles using the wheel snap rule.
            self.viewport.finish_drag(self.terminal, self.now);
            if copy && self.selection.copy_on_select {
                // Select precedes Copy in the follow-up FIFO, so release copies its final hit.
                ctx.submit(
                    Command::Copy(CopyTarget::Selection(Format::PlainText)),
                    origin,
                );
            }
        }
    }
}

/// A folded display coordinate is not an application's physical screen row.
impl Handler<'_> {
    /// Use the same retained row map as drawing/selection; history has no PTY row.
    pub(super) fn application_pointer(
        &self,
        mut event: crate::vt::input::MouseEvent,
    ) -> Option<crate::vt::input::MouseEvent> {
        // Unfolded terminals retain their original input coordinates and encoder work.
        if !self.viewport.projected() {
            return Some(event);
        }
        // Pixel coordinates are signed until fractional clipping is accounted for.
        // The host already removed the grid inset through the common input adapter.
        let height = i64::from(self.viewport.row_height().max(1));
        let y = (i64::from(event.pixel_y) - i64::from(self.viewport.device_shift())).max(0);
        // Display row one may name source row four; never report the compressed index.
        let source = self.viewport.source_row(event.row as usize)?;
        // Historical text can be selected but cannot name an application-screen row.
        let row = source.checked_sub(self.terminal.screen().ring.scrollback_len())?;
        if row >= self.terminal.size().1 {
            return None;
        }
        event.row = row as u32;
        // Pixel-mode reports preserve the exact offset within the mapped source row.
        event.pixel_y = (row as i64 * height + y % height) as u32;
        Some(event)
    }
}
