//! Native pointer callbacks submit typed bus requests without frame waits.
//!
//! Overlay region routing runs before the terminal protocol or local selection.
//! Session-owned models route input against the last presented region table.
//! No builder or view callback is lent to input handling.
//! Only unhandled input is submitted to the terminal command queue afterward.
//! Physical overlay moves are coalesced independently of cell-level mouse reports.
//! A hover in the same cell can cross a small region without emitting app motion.
//! Recordings preserve handled samples too, retaining the actual input ownership.
//!
//! Button/wheel input is immediate; only motion waits for the next paced
//! display turn. Recordings therefore capture coalesced motion rather than
//! every high-frequency OS sample (§5.7, P12). Session owns local selection.
//! Input observations arrive as change-only events and guide host coalescing;
//! the Session owner samples current modes before encoding each queued sample.
//! This permits an application to enable tracking between two callbacks.
//! Cell metrics come from the host-owned text context already installed in
//! Session. Surface pixels and glyph pixels therefore share the same DPR.
//! Button presses and wheel ticks do not request or await a GPU frame: their
//! controls reach the owner immediately and encode there in writer-producer order.
//! Hover motion alone may need a display turn even when VT has no damage.
//! The paced redraw flushes motion before attempting a terminal snapshot.
//! If no row changed, it sends the report but performs no GPU presentation.
//! That keeps reporting at most once per refresh without manufacturing frames.
//! Click intervals use the window lifetime as a monotonic epoch. Wall-clock
//! changes cannot turn an ordinary press into an accidental triple click.
//! Pointer input is recorded as a typed command after coalescing, making
//! the replay independent of native callback frequency and font changes.
use super::InputWindow as LiveWindow;
use crate::mouse::Geometry;
use puzzle_term::{
    messages::Command,
    vt::input::{MouseEvent, MouseModes},
};
use std::time::Instant;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

/// Winit is read only at this platform edge; the adapter receives plain data.
impl LiveWindow {
    /// Keeps the latest eligible physical move for one upcoming refresh.
    pub(in crate::window) fn pointer_moved(&mut self, x: f64, y: f64) -> bool {
        self.drag_position([x, y].map(|n| n / self.window.scale_factor()));
        let Some((geometry, modes)) = self.pointer_geometry() else {
            return false;
        };
        self.pointer.moved([x, y], geometry, self.modifiers, modes);
        // Even a just-changed mode must see this physical sample. Owner policy decides.
        x.is_finite() && y.is_finite()
    }

    /// Maps native button identity and immediately pumps the public lane.
    pub(in crate::window) fn pointer_button(&mut self, button: MouseButton, state: ElementState) {
        let Some((geometry, _)) = self.pointer_geometry() else {
            return;
        };
        // The same native identity table is exercised without a live window.
        let button = crate::mouse::button_code(button);
        // Click-chain timing is host input data, not a clock in puzzle-vt.
        let now = Instant::now();
        let elapsed = now
            .saturating_duration_since(self.pointer_epoch)
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let event = self.pointer.button(
            button,
            state == ElementState::Pressed,
            geometry,
            self.modifiers,
            elapsed,
        );
        self.submit_pointer(event, now);
    }

    /// Converts physical smooth-wheel deltas using current device-cell metrics.
    pub(in crate::window) fn pointer_wheel(&mut self, delta: MouseScrollDelta) {
        let Some((geometry, _)) = self.pointer_geometry() else {
            return;
        };
        // Preserve trackpad pixels and hardware notches through the shared bus lane.
        let (vertical, horizontal) = self.pointer.wheel_command(delta, geometry, self.modifiers);
        if let Some(command) = vertical {
            self.submit_pointer_command(command, Instant::now());
        }
        // Horizontal directions keep their distinct xterm button reports.
        for event in horizontal {
            self.submit_pointer(event, Instant::now());
        }
    }

    /// Samples only once per display turn, even while the grid is unchanged.
    pub(in crate::window) fn flush_pointer(&mut self) {
        self.flush_drop(true);
        // Physical motion is coalesced here; only the Session owner samples live modes.
        // Overlay routing runs there before cell/pixel duplicate suppression.
        if let Some(event) = self.pointer.take_motion() {
            self.sent(self.session.pointer_motion(self.host_event(event)));
        }
    }

    /// Last-delivered geometry is a copied fact, never a synchronous VT query.
    fn pointer_geometry(&self) -> Option<(Geometry, MouseModes)> {
        let state = self.input_state?;
        let metrics = state.cell?;
        Some((
            Geometry {
                cell: [metrics.width_px, metrics.height_px],
                grid: state.grid.map(|n| n as u32),
            },
            state.mouse,
        ))
    }

    /// Records only the event that actually enters the policy chain.
    fn submit_pointer(&mut self, event: MouseEvent, now: Instant) {
        self.submit_pointer_command(Command::Mouse(event), now);
    }

    /// Wheel and button commands share recording and immediate ordered dispatch.
    fn submit_pointer_command(&mut self, command: Command, now: Instant) {
        // Puzzle records the converted core command on its Session owner.
        // Only authored pointer/wheel variants originate from this native edge.
        let result = match command {
            Command::Mouse(event) => self.session.pointer(self.host_event(event)),
            Command::Wheel { delta, pointer } => {
                // Winit precise deltas are backing pixels; the public seam uses logical pixels.
                let delta = match delta {
                    puzzle_term::viewport::ScrollInput::Pixels(value) => {
                        puzzle_term::viewport::ScrollInput::Pixels(
                            value / self.window.scale_factor(),
                        )
                    }
                    rows => rows,
                };
                self.session.wheel(delta, self.host_event(pointer))
            }
            other => self.session.dispatch(other),
        };
        self.sent(result);
        let _ = now;
    }

    /// Winit supplies backing pixels; Puzzle alone removes the rendered grid origin.
    fn host_event(&self, event: MouseEvent) -> puzzle_embed::SurfacePointer {
        // Button, motion and wheel share the same raw position, with no legacy
        // core-coordinate conversion or second inset subtraction in between.
        self.pointer
            .surface_pointer(event, self.window.scale_factor())
    }
}
