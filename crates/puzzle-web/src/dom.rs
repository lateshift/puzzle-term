//! Browser event ownership around the typed session input seam (§12.2).
//!
//! The platform host implements these queries and routes commands into Session;
//! DOM listeners never encode terminal bytes themselves. Rendering is owned by
//! the host's rAF scheduler, not by input handlers or ResizeObserver callbacks.
/// IME event-order normalization before the bus input lane.
mod composition;
/// The platform host queries modes and dispatches typed commands.
mod host;
/// Layout-aware key mapping keeps produced text separate from physical codes.
pub mod keyboard;
/// Removing a terminal also removes every DOM closure registered for it.
mod listener;
pub use host::{DomHost, Geometry, KeyDisposition};
/// Size and media observers report facts without initiating a draw.
mod monitor;
/// Element ownership and typed event dispatch for one mounted terminal.
mod mounted;
/// Physical pointer mapping and non-passive scrolling ownership.
mod pointer;
/// Textarea keyboard, clipboard and IME listeners.
mod text;
pub use mounted::{DomInput, platform};

/// Transient input state belongs to the DOM lifetime, not the terminal buffer.
struct KeyboardState<H: DomHost> {
    /// Commands go to the same session entry point used by programmatic input.
    host: H,
    /// Layout and Option-side facts persist across DOM key callbacks.
    keyboard: keyboard::Keyboard,
    /// Provisional composition never reaches application stdin.
    composition: composition::Composition,
    /// True only after this terminal successfully acquired fullscreen keyboard lock.
    locked: bool,
    /// A compatibility mousedown may supply a count only for its own pointer press.
    pending_mouse_press: Option<u16>,
}

/// A new textarea starts with no modifiers or unfinished composition.
impl<H: DomHost> KeyboardState<H> {
    /// The host is passed in; mounting input does not construct another Session.
    fn new(host: H) -> Self {
        Self {
            host,
            keyboard: keyboard::Keyboard::default(),
            composition: composition::Composition::default(),
            locked: false,
            pending_mouse_press: None,
        }
    }
}

#[cfg(test)]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(test)]
/// Actual DOM dispatch witnesses protect listener ownership and default actions.
mod tests;
