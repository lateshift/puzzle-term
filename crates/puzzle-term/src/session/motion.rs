//! Session-owned protocol deduplication after host refresh coalescing (P12, D-50).
//!
//! Hosts retain physical samples without inspecting live VT modes. The owner
//! routes overlays first, then compares either signed local positions or the
//! actual protocol's cell/pixel coordinates. A mode change between OS arrival
//! and control application is therefore visible before any bytes are encoded.
use crate::{
    pointer::PointerInput,
    vt::input::{Modifiers, MouseEncoding, MouseKind, MouseModes, MouseTracking},
};

/// These scalar identities never retain text or allocate per pointer sample.
#[derive(Default)]
pub(super) struct Motion {
    /// Application reports suppress unchanged coordinates in their active encoding.
    reported: Option<(MouseEncoding, u32, u32)>,
    /// Local selection preserves subcell and outside-distance motion.
    local: Option<[f64; 2]>,
}

impl Motion {
    /// Button transitions start a new drag, even at the preceding drag's last point.
    pub(super) fn reset(&mut self) {
        // Both identities describe a drag lifetime, not a permanent position.
        self.reported = None;
        self.local = None;
    }

    /// Decides unhandled motion from owner-side modes, never a stale host observation.
    pub(super) fn accepts(
        &mut self,
        input: PointerInput,
        modes: MouseModes,
        shift_selects: bool,
    ) -> bool {
        let event = input.event;
        // Malformed host coordinates have no useful local or protocol direction.
        if event.kind != MouseKind::Move || input.position.iter().any(|n| !n.is_finite()) {
            return false;
        }
        // Shift is a selection override only when the user's policy enables it.
        let local = modes.tracking == MouseTracking::None
            || (shift_selects && event.modifiers.0 & Modifiers::SHIFT != 0);
        if local {
            // Outside-distance changes drive selection even at an equal clamped cell.
            let changed = self.local != Some(input.position);
            self.local = Some(input.position);
            return changed;
        }
        // Hover reporting and held-button reporting are distinct xterm modes.
        if modes.tracking != MouseTracking::AnyMotion
            && !(modes.tracking == MouseTracking::ButtonMotion && event.button_held)
        {
            // A tracking transition still cancels a local left drag in the handler.
            // Normal/X10 encoding itself rejects this motion, producing no report.
            return event.button == 0 && event.button_held;
        }
        // Mode 1016 preserves movement within a cell. Other encodings cannot
        // express that movement and must suppress identical cell reports.
        let (x, y) = if modes.encoding == MouseEncoding::SgrPixels {
            (event.pixel_x, event.pixel_y)
        } else {
            (event.column, event.row)
        };
        // Include the encoding so a mode change can report the same position.
        let identity = (modes.encoding, x, y);
        let changed = self.reported != Some(identity);
        self.reported = Some(identity);
        changed
    }
}

impl super::Session {
    /// Overlay ownership precedes mode-dependent suppression and application encoding.
    pub(super) fn apply_motion(&mut self, input: PointerInput, now: web_time::Instant) {
        // Font/grid messages ordered before this sample define its actual cell.
        let input = self.owner_pointer(input);
        // Regions refer to presented ink, and a handled region consumes motion.
        let result = self.route_overlay_sample(input, now);
        // Cursor feedback returns asynchronously without a host-side hit query.
        self.host_events
            .push_back(super::host_events::HostEvent::CursorChanged(result.cursor));
        if result.handled {
            return;
        }
        // The parser may have changed these modes since the OS callback arrived.
        let modes = MouseModes::from_terminal(&self.terminal);
        // Coalescing must use current ownership, just like the eventual pointer handler.
        if self
            .motion
            .accepts(input, modes, self.mouse_shift_selects())
        {
            // The same ordinary command policies protect local selection and reports.
            self.submit(
                crate::messages::Command::Pointer(input),
                puzzle_bus::Origin::User,
            );
            self.pump(now);
        }
    }

    /// Re-derives protocol coordinates after queued font or grid changes.
    pub(super) fn owner_pointer(&self, mut input: PointerInput) -> PointerInput {
        let Some(cell) = self.cell_metrics() else {
            // Headless callers without font measurements supply their own geometry.
            return input;
        };
        // Metrics are validated nonzero device pixels before entering Session.
        input.cell = [cell.width_px, cell.height_px];
        let (columns, rows) = self.terminal.size();
        let grid = [columns, rows].map(|n| n.min(u32::MAX as usize) as u32);
        // Saturating conversion leaves negative distance only in the local sample.
        let pixel = input.position.map(|n| n.max(0.0) as u32);
        // Signed outside distance remains untouched for local selection dragging.
        // Only application reports clamp to the Session's current physical extent.
        input.event.column = (pixel[0] / input.cell[0]).min(grid[0].saturating_sub(1));
        input.event.row = (pixel[1] / input.cell[1]).min(grid[1].saturating_sub(1));
        input.event.pixel_x = pixel[0].min(grid[0].saturating_mul(input.cell[0]).saturating_sub(1));
        input.event.pixel_y = pixel[1].min(grid[1].saturating_mul(input.cell[1]).saturating_sub(1));
        input
    }
}
