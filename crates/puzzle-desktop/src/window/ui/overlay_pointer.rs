//! Native input invokes session-owned overlay models against presented regions.
//!
//! Models own hover and drag state independently of the frame builder. Pointer
//! handling requires no font borrow, renderer or builder loan. Pointer messages
//! reach the session owner, which hit-tests the last presented region table.
//! A window-leave transition discards pending hover samples so a later flush
//! cannot manufacture an enter from a coordinate that already expired.
//! Captured drags keep their release ownership across that transition.
//!
//! The OS cursor reflects the selected presented region even when a callback
//! declines consumption. Text/default regions map to the terminal's text cursor;
//! thumb contact maps to grab/grabbing without consulting glyph or row caches.
//! Hover and captured-release ordering remain in the shared model router.
//! Neither a Leave nor a fresh sample waits for a detached builder to return.
use super::InputWindow as LiveWindow;

/// The native edge maps model results to an OS cursor.
impl LiveWindow {
    /// Leave follows preceding samples in the owner's control FIFO.
    pub(in crate::window) fn pointer_left(&mut self) {
        self.pointer.discard_motion();
        self.sent(
            self.session
                .overlay_pointer(puzzle_term::overlay::provider::PointerEvent {
                    kind: puzzle_term::overlay::provider::PointerKind::Leave,
                    point: [0.0; 2],
                    button: None,
                    modifiers: 0,
                }),
        );
    }

    /// Converts the portable region cursor once at the native platform edge.
    pub(super) fn apply_overlay_cursor(&self, cursor: puzzle_term::overlay::draw::CursorIcon) {
        use puzzle_term::overlay::draw::CursorIcon;
        self.window.set_cursor(match cursor {
            CursorIcon::Default | CursorIcon::Text => winit::window::CursorIcon::Text,
            CursorIcon::Pointer => winit::window::CursorIcon::Pointer,
            CursorIcon::Grab => winit::window::CursorIcon::Grab,
            CursorIcon::Grabbing => winit::window::CursorIcon::Grabbing,
        });
    }
}
