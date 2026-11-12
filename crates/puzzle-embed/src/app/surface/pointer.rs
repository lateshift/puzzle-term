//! View-local host points are converted only by Puzzle, using presented geometry.
//! Hosts supply logical points including padding; negative edge drags stay signed.
//! The event's derived coordinates are compatibility metadata, never authority.
//! Hosts do not need to wait for inputState before sending their first callback.
//! The Session owner applies conversion in order with presentation messages,
//! keeping overlay regions, local selection and application reports aligned.
use super::*;

/// A host gesture sample in view-local logical pixels (AppKit points).
#[derive(Clone, Copy, Debug)]
pub struct SurfacePointer {
    /// Signed view-local position, top-left origin and y down; never subtract a grid origin.
    pub position: [f64; 2],
    /// Button/modifier facts only; cell/pixel fields are ignored and resolved by Puzzle.
    pub event: puzzle_term::vt::input::MouseEvent,
}
impl Surface {
    /// Buttons route through presented overlays before application reporting or selection.
    pub fn pointer(&self, input: SurfacePointer) -> Result<(), ApiError> {
        self.host_pointer(input, crate::session_thread::pointer::Action::Button)
    }
    /// Motion is deduplicated using the Session owner's current mouse modes.
    pub fn pointer_motion(&self, input: SurfacePointer) -> Result<(), ApiError> {
        self.host_pointer(input, crate::session_thread::pointer::Action::Motion)
    }
    /// Precise deltas are logical pixels; notches retain their row units.
    pub fn wheel(
        &self,
        delta: puzzle_term::viewport::ScrollInput,
        pointer: SurfacePointer,
    ) -> Result<(), ApiError> {
        self.host_pointer(
            pointer,
            crate::session_thread::pointer::Action::Wheel(delta),
        )
    }
    /// Validation and posting never borrow rendering or block native input.
    fn host_pointer(
        &self,
        input: SurfacePointer,
        action: crate::session_thread::pointer::Action,
    ) -> Result<(), ApiError> {
        // Affinity/closed checks precede queueing, just like keys and settings.
        // Invalid floating-point values must never enter geometry division.
        self.check()?;
        if input.position.iter().any(|n| !n.is_finite()) {
            return Err(ApiError::InvalidArgument("pointer position must be finite"));
        }
        // The separate control inbox wakes an idle owner without a frame wait.
        // Rendering, fallback and drawable acquisition cannot block this call.
        self.session
            .host_pointer(input, action)
            .map_err(ApiError::Platform)
    }
}
