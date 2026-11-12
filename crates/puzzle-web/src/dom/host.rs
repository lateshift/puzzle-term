//! The DOM's copied geometry and rare platform-host seam (§12.2).
//!
//! Session stays Send and synchronous. The page owns JS references and routes
//! neutral commands into Session immediately, without waiting for a frame.
//! Rendering is a separate rAF path: none of these input queries draws pixels.
use super::keyboard::{KeyPolicy, MappedKey};
use puzzle_term::{messages::Command, theme::Appearance, vt::input::MouseModes};

/// Device-pixel geometry; CSS event positions are multiplied by dpr once.
#[derive(Clone, Copy)]
pub struct Geometry {
    /// Physical pixels per CSS pixel, including browser zoom.
    pub dpr: f64,
    /// Grid inset in device pixels, excluding the host page's own margin.
    pub padding: [f64; 2],
    /// Device-pixel dimensions of a single fixed terminal cell.
    pub cell: [f64; 2],
    /// Visible cell extent; out-of-grid pointer reports clamp to its edges.
    pub grid: [u32; 2],
    /// Device-pixel top-left of the cursor for the IME candidate anchor.
    pub cursor: [f64; 2],
}

/// Native DOM behavior is decided after the shared binding dry run.
pub enum KeyDisposition {
    /// A binding or the encoder consumed the event; prevent textarea text.
    Consumed,
    /// No binding owns the printable key; wait for its native input event.
    Text,
    /// Paste is a browser action: wait for clipboardData rather than reading it.
    Paste,
    /// Copy runs in this keydown's user activation; M8 supplies the selection.
    Copy(String),
    /// Rich copy keeps its plain fallback in the same keydown activation.
    CopyHtml {
        /// Escaped HTML for rich editors.
        html: String,
        /// Selected Unicode text for plain editors.
        text: String,
    },
}

/// Browser ownership stays outside the neutral Session and its Send services.
pub trait DomHost {
    /// Samples current-screen kitty flags and the user's side-aware Option policy.
    fn key_policy(&self) -> KeyPolicy;
    /// Runs bindings before deciding whether native text/paste owns this key.
    fn key(&mut self, mapped: MappedKey) -> KeyDisposition;
    /// Submits and pumps typed input now; only the renderer waits for rAF.
    fn command(&mut self, command: Command);
    /// Gives overlay regions first refusal before selection and application reporting.
    fn pointer(&mut self, input: puzzle_term::pointer::PointerInput) -> bool {
        // Headless synthetic DOM hosts may have no provider registry.
        self.command(Command::Pointer(input));
        false
    }
    /// Ends overlay hover when the pointer leaves the canvas; capture survives until release.
    fn pointer_left(&mut self) {}
    /// Determines wheel ownership through the Session routing and boundary policy.
    fn wheel_consumable(
        &self,
        delta: puzzle_term::viewport::ScrollInput,
        pointer: puzzle_term::vt::input::MouseEvent,
    ) -> bool;
    /// Accessibility suppresses touch momentum without changing stored settings.
    fn motion_reduced(&self) -> bool;
    /// Supplies current grid and candidate geometry, never a stale startup size.
    fn geometry(&self) -> Geometry;
    /// Queries tracking before canceling a browser pointer or wheel default.
    fn mouse_modes(&self) -> MouseModes;
    /// Shift returns a tracked mouse gesture to local selection when enabled.
    fn shift_selects(&self) -> bool;
    /// Hidden pages cancel render/blink wakes, not transport parsing.
    fn visibility(&mut self, visible: bool);
    /// Native appearance uses the same mode-2031 path as desktop settings.
    fn appearance(&mut self, appearance: Appearance);
    /// Reduced motion disables fades and animation, independent of theme mode.
    fn reduced_motion(&mut self, enabled: bool);
    /// CSS system-color activation is reported for the host's theme resolver.
    fn forced_colors(&mut self, enabled: bool);
    /// Coalesces geometry to the next rAF; PTY resize has its own trailing timer.
    fn resize(&mut self, device_pixels: [u32; 2], dpr: f64);
}
