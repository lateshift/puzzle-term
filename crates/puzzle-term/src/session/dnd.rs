//! Owner-side drop geometry and lifecycle messages; no host waits for Session.
//!
//! Native hosts supply view-local logical coordinates, including the rendered inset.
//! The Session subtracts that inset once; hosts must never pre-subtract a grid origin.
//! The latest presented geometry is authoritative, just as it is for overlay hit regions.
//! A newly requested resize cannot make input target cells that have not been presented.
//! DPR converts logical coordinates into the device pixels required by OSC 72 X/Y.
//! Rounded cell dimensions match the frame builder's snapped physical cell metrics.
//! Fractional scrolling offsets row boundaries without moving the surface's pixel origin.
//! Cell indices and device pixels therefore have separate vertical computations.
//! Padding and outside-grid positions clamp to the nearest valid terminal cell.
//! Finite-coordinate validation happens before float-to-integer conversion.
//! The terminal's registration epoch independently rejects delayed native gestures.
//! A drop counts as other user input for single-use clipboard-grant revocation.
//! Protocol replies are flushed through pump in the existing writer-producer order.
//! No host callback is executed while translating geometry on the sole Session owner.

use super::Session;
use crate::dnd::{Input, Kind};
impl Session {
    /// Applies native data only on the sole owner; stale registrations are inert.
    pub fn drop_input(&mut self, input: Input, now: web_time::Instant) -> Result<(), String> {
        if input.position.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite drop point".into());
        }
        // A real native gesture revokes a pending unrelated clipboard-paste grant.
        self.clipboard.revoke_paste_grant();
        // Presentation acknowledged these cell edges; pending resizes cannot move them.
        let geometry = self.overlay_geometry();
        let scale = f64::from(geometry.dpr.max(f32::EPSILON));
        let pixel = std::array::from_fn::<_, 2, _>(|axis| {
            ((input.position[axis] - f64::from(geometry.origin()[axis])) * scale).max(0.0) as u32
        });
        let cell = geometry
            .cell
            .map(|n| (f64::from(n) * scale).round().max(1.0) as u32);
        let (columns, rows) = self.terminal.size();
        let grid = [columns, rows].map(|n| n.max(1).min(u32::MAX as usize) as u32);
        // Fractional viewport displacement changes which visible row owns the point.
        let row_y = geometry
            .lines
            .first()
            .map_or(f64::from(geometry.inset), |line| f64::from(line.y));
        let row =
            (((input.position[1] - row_y) * scale).max(0.0) as u32 / cell[1]).min(grid[1] - 1);
        let position = [
            (pixel[0] / cell[0]).min(grid[0] - 1),
            row,
            pixel[0].min(grid[0].saturating_mul(cell[0]) - 1),
            pixel[1].min(grid[1].saturating_mul(cell[1]) - 1),
        ];
        let event = match input.kind {
            Kind::Move => puzzle_vt::dnd::Input::Move {
                position,
                operations: input.operations,
                items: input.items,
            },
            Kind::Leave => puzzle_vt::dnd::Input::Leave,
            Kind::Drop => puzzle_vt::dnd::Input::Drop {
                position,
                operations: input.operations,
                items: input.items,
            },
        };
        self.terminal.drop_input(input.generation, event)?;
        self.forward_events();
        self.pump(now);
        Ok(())
    }
}
