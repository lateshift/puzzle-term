//! Native wheel units cross the public Session lane intact (§7.2–§7.4).
//!
//! Winit's positive vertical axis points toward older text; Session uses
//! positive toward the live tail. Only that sign changes for local movement.
//! Horizontal motion retains the M7 reporting path and distinct 66/67 codes.
//! There is no mode snapshot attached to the vertical command: application
//! output can change tracking before dispatch, so Session checks it again.
//! The pointer payload supplies both cell and pixel positions for SGR/1016.
//! Geometry converts only horizontal pixels, whose legacy report path has
//! no local viewport effect; vertical conversion stays inside the core.
use super::{Adapter, Geometry};
use puzzle_term::{
    messages::Command,
    viewport::ScrollInput,
    vt::input::{MouseEvent, MouseKind},
};
use winit::event::MouseScrollDelta;
/// Physical input maps to neutral deltas; the Session alone chooses routing.
impl Adapter {
    /// Returns vertical input plus any complete horizontal application reports.
    pub(crate) fn wheel_command(
        &mut self,
        delta: MouseScrollDelta,
        geometry: Geometry,
        mods: u8,
    ) -> (Option<Command>, Vec<MouseEvent>) {
        // Pixels are already device pixels, including monitor scaling from winit.
        let (vertical, horizontal) = match delta {
            MouseScrollDelta::LineDelta(x, y) => (ScrollInput::Rows(-f64::from(y)), f64::from(x)),
            MouseScrollDelta::PixelDelta(p) => (
                ScrollInput::Pixels(-p.y),
                p.x / f64::from(geometry.cell[0].max(1)),
            ),
        };
        // Invalid hardware input is inert, matching the bounded protocol accumulator.
        let value = match vertical {
            ScrollInput::Rows(v) | ScrollInput::Pixels(v) => v,
        };
        let command = (value.is_finite() && value != 0.0).then(|| Command::Wheel {
            delta: vertical,
            pointer: self.event(MouseKind::Wheel, 64, geometry, mods),
        });
        // Never pre-round the vertical path: sub-row trackpad samples are meaningful.
        (command, self.wheel([horizontal, 0.0], geometry, mods))
    }
}
