//! Winit callback dispatch into the owned RawInput adapter (§13.4).
//!
//! MacOS may deliver a modifier change on either side of IME Commit, so
//! callback order is preserved. The adapter sees the resulting RawInput and
//! applies the same policy that replay uses; it never reads winit fields.
//!
//! Focus is also a VT presentation fact. It travels through RawInput for
//! the recording and reaches the session owner as a cursor-focus message.

use super::{raw, ui::InputWindow as LiveWindow};
use puzzle_embed::RawInput;
use serde_json::json;
use winit::event::WindowEvent;

/// Converts each relevant native callback exactly once at the window edge.
pub(super) fn handle(live: &mut LiveWindow, event: WindowEvent) -> bool {
    match event {
        // Physical pointer coordinates already include the monitor DPR.
        WindowEvent::CursorMoved { position, .. } => {
            return live.pointer_moved(position.x, position.y);
        }
        WindowEvent::CursorLeft { .. } => live.pointer_left(),
        WindowEvent::MouseInput { state, button, .. } => live.pointer_button(button, state),
        WindowEvent::MouseWheel { delta, .. } => live.pointer_wheel(delta),
        WindowEvent::KeyboardInput { event, .. } => {
            // Private probes can reject accidental user input without recording
            // the actual typed key. Authored fixture input bypasses this OS edge.
            tracing::debug!(target:"puzzle_desktop::input", event="desktop.external_key");
            live.input(raw::key(&event));
        }
        // Separate modifier records reveal whether macOS clears Ctrl before
        // delivering the associated IME text for that same key press.
        WindowEvent::ModifiersChanged(modifiers) => {
            tracing::debug!(target: "puzzle_desktop::input", event = "ModifiersChanged", state = ?modifiers.state());
            live.input(raw::modifiers(&modifiers));
        }
        WindowEvent::Ime(ime) => {
            // Enabling IME at startup is not user input; text/preedit is.
            if matches!(&ime, winit::event::Ime::Commit(text) | winit::event::Ime::Preedit(text, _) if !text.is_empty())
            {
                tracing::debug!(target:"puzzle_desktop::input", event="desktop.external_ime");
            }
            live.input(raw::ime(ime));
        }
        WindowEvent::Focused(focused) => {
            tracing::debug!(target: "puzzle_desktop::input", event = "Focused", focused);
            // A recording marks focus both as an OS window event and as
            // adapter input, so key traces can be aligned to presentation.
            live.record_event("focus", json!({"focused": focused}));
            live.input(RawInput::Focus(focused));
            live.log_visibility("winit_focused");
        }
        _ => {}
    }
    false
}
