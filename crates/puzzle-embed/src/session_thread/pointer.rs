//! Converts view-local host points against the last actually presented geometry.
//! Core PointerInput remains signed grid-relative device pixels, preserving web
//! and selection contracts. Fractional scrolling is resolved by core selection;
//! subtracting its row shift here too would move a dragged selection twice.
use super::*;
use crate::SurfacePointer;
use puzzle_term::{SessionMessage, messages::Command, viewport::ScrollInput};

/// Destination policy is decided after position conversion on the Session owner.
#[derive(Clone, Copy)]
pub(crate) enum Action {
    /// Press/release or horizontal application wheel report.
    Button,
    /// Coalesced hover/drag callback.
    Motion,
    /// Vertical wheel with logical-pixel or row deltas.
    Wheel(ScrollInput),
}
impl Handle {
    /// Ordered with geometry and presentation messages, never a synchronous query.
    pub(crate) fn host_pointer(&self, input: SurfacePointer, action: Action) -> Result<(), String> {
        self.post(endpoint::Control::Pointer(input, action))
    }
}
impl Owner {
    /// Translates once, then uses the existing core policy and selection lanes.
    pub(super) fn apply_pointer(&mut self, input: SurfacePointer, action: Action) {
        // Presented geometry is returned in the same control FIFO as input. A future
        // frame being built cannot move the hit targets under this callback.
        let geometry = self.session.overlay_geometry();
        // Scroll deltas and pointer positions enter in the same logical units.
        // Only this boundary converts them into the core's device-pixel contract.
        // Empty startup geometry cannot cause division by zero. Before presentation
        // there is no ink or hit table; successful frames always provide positive DPR.
        let scale = f64::from(geometry.dpr.max(f32::EPSILON));
        let pointer = map(input, geometry, self.session.terminal().size());
        let message = match action {
            // Let core deduplication sample current modes; host-side mode observations
            // may be older than the application's most recent parser transaction.
            Action::Motion => SessionMessage::PointerMotion(pointer),
            // Horizontal wheel reports have no local vertical-scroll behavior.
            // Buttons still take the ordinary overlay/selection policy route.
            Action::Button => SessionMessage::Command(
                if pointer.event.kind == puzzle_term::vt::input::MouseKind::Wheel {
                    Command::Mouse(pointer.event)
                } else {
                    Command::Pointer(pointer)
                },
                puzzle_term::Origin::User,
            ),
            Action::Wheel(delta) => {
                // Protocol wheels and local scrolling see the same grid-relative sample.
                let delta = match delta {
                    ScrollInput::Pixels(value) => ScrollInput::Pixels(value * scale),
                    rows => rows,
                };
                SessionMessage::Command(
                    Command::Wheel {
                        delta,
                        pointer: pointer.event,
                    },
                    puzzle_term::Origin::User,
                )
            }
        };
        self.record_pointer(&message);
        // Native input remains recoverable. A closed transport or a rejected command
        // must not unwind the worker or make later accepted input disappear.
        if let Err(error) = self.session.apply(message, Instant::now()) {
            tracing::warn!(event = "embed.pointer_rejected", %error);
        }
    }
    /// Replay consumes core coordinates, so record only after the native conversion.
    fn record_pointer(&self, message: &SessionMessage) {
        // Host coordinates contain padding; recording them as core PointerInput would
        // reintroduce the original mismatch during a deterministic headless replay.
        let Some(recorder) = &self.recorder else {
            return;
        };
        let result = match message {
            SessionMessage::Command(command, _) => recorder.command(Some(command)),
            SessionMessage::PointerMotion(pointer) => {
                recorder.command(Some(&Command::Pointer(*pointer)))
            }
            _ => return,
        };
        if let Err(error) = result {
            tracing::warn!(event = "desktop.record_failed", %error);
        }
    }
}
/// Cell indices saturate only for protocols; signed distance survives local dragging.
fn map(
    input: SurfacePointer,
    geometry: &puzzle_term::overlay::geometry::TextGeometry,
    grid: (usize, usize),
) -> puzzle_term::pointer::PointerInput {
    // Empty startup geometry cannot cause division by zero. Before presentation
    // there is no ink or hit table; successful frames always provide positive DPR.
    let scale = f64::from(geometry.dpr.max(f32::EPSILON));
    // Subtract the logical inset before applying DPR, keeping outside distance
    // signed for selection auto-scroll. Protocol saturation happens separately.
    let position = std::array::from_fn(|axis| {
        (input.position[axis] - f64::from(geometry.origin()[axis])) * scale
    });
    // Cell geometry was published from integer device metrics divided by DPR;
    // rounding reverses that conversion without truncating floating-point error.
    let cell = geometry
        .cell
        .map(|n| (f64::from(n) * scale).round().max(1.0) as u32);
    // The live grid bounds application reports. Fractional scrolling can publish
    // an extra clipped row in TextGeometry, which is not an extra terminal row.
    let grid = [grid.0, grid.1].map(|n| n.max(1).min(u32::MAX as usize) as u32);
    // Xterm cell/pixel encodings cannot express negative coordinates. The raw
    // position above deliberately keeps that information for local selection.
    let pixel = position.map(|n| n.max(0.0) as u32);
    // Ignore every host-derived coordinate: only native button/modifier metadata
    // survives. This prevents padding or a stale host metric from being applied twice.
    let mut event = input.event;
    event.column = (pixel[0] / cell[0]).min(grid[0] - 1);
    event.row = (pixel[1] / cell[1]).min(grid[1] - 1);
    // The final valid pixel belongs to the final cell; the exclusive edge itself
    // is clamped, avoiding an out-of-bounds SGR-pixel report on an outside drag.
    event.pixel_x = pixel[0].min(grid[0].saturating_mul(cell[0]).saturating_sub(1));
    event.pixel_y = pixel[1].min(grid[1].saturating_mul(cell[1]).saturating_sub(1));
    puzzle_term::pointer::PointerInput {
        position,
        cell,
        event,
    }
}

#[cfg(test)]
/// Mapping and local selection use the same origin at both device scales.
mod tests;
