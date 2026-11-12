//! Converts winit callbacks into owned RawInput before command mapping.
//!
//! The window is the only module that reads winit KeyEvent fields. Tests,
//! harness scenarios and recordings use the copied data instead, so they
//! can exercise the same adapter without manufacturing a private OS event.
//!
//! Logical and physical key identity serve different future consumers.
//! The terminal encoder uses the logical key and committed text today;
//! M7 keybindings can use a physical code without remapping a recording.
//! Location distinguishes a numpad or left/right modifier from the main
//! key even when its logical name is otherwise identical. Retaining all
//! fields here prevents a later diagnosis from relying on reconstructed
//! values that the OS callback never supplied.
//!
//! Modifiers are converted to the four low kitty/xterm bits at this
//! boundary. The adapter cannot accidentally interpret winit's private
//! bit representation as a protocol modifier. IME preedit remains data,
//! never a command, until a distinct Commit callback is observed.
//!
//! Debug spelling of named and physical keys is copied verbatim into the
//! raw event. K2 recordings preserve those strings, and K4 will pin the
//! macOS sequences from the user's real-window capture.

use puzzle_embed::{LEFT_OPTION, RIGHT_OPTION, RawIme, RawInput, RawKey, RawLogical};
use puzzle_term::vt::input::Modifiers;
use winit::{
    event::{ElementState, Ime, KeyEvent, Modifiers as WinitModifiers},
    keyboard::{Key, ModifiersKeyState},
    platform::modifier_supplement::KeyEventExtModifierSupplement,
};

/// Copies a layout key without retaining winit's private key representation.
fn logical(key: &Key) -> RawLogical {
    match key {
        Key::Named(name) => RawLogical::Named(format!("{name:?}")),
        Key::Character(text) => RawLogical::Character(text.to_string()),
        Key::Dead(_) => RawLogical::Dead,
        Key::Unidentified(_) => RawLogical::Unidentified,
    }
}

/// Copies every observable key field before the adapter chooses a command.
pub(super) fn key(event: &KeyEvent) -> RawInput {
    // The named-key debug spelling is the stable name matched by the
    // adapter table; a character keeps its full composed Unicode text.
    RawInput::Key(RawKey {
        logical: logical(&event.logical_key),
        // Winit exposes the current layout's base key independently of
        // Option; physical position alone cannot recover German `7` from `|`.
        base: logical(&event.key_without_modifiers()),
        // Preserve the physical code even though M6 does not bind shortcuts
        // by keyboard position yet.
        physical: format!("{:?}", event.physical_key),
        text: event.text.as_ref().map(ToString::to_string),
        location: format!("{:?}", event.location),
        pressed: event.state == ElementState::Pressed,
        repeat: event.repeat,
    })
}

/// Converts OS modifier bits to the neutral kitty/xterm positions.
pub(super) fn modifiers(event: &WinitModifiers) -> RawInput {
    // These constants are protocol bits shared with puzzle-vt, not the
    // winit ModifiersState implementation's internal bit positions.
    let mut bits = 0;
    let state = event.state();
    if state.shift_key() {
        bits |= Modifiers::SHIFT;
    }
    if state.alt_key() {
        bits |= Modifiers::ALT;
    }
    if state.control_key() {
        bits |= Modifiers::CTRL;
    }
    if state.super_key() {
        bits |= Modifiers::SUPER;
    }
    // Side metadata is meaningful for macOS Option. Other platforms keep
    // their Alt behavior until the separate AltGr policy arrives in M13.
    if cfg!(target_os = "macos") {
        if event.lalt_state() == ModifiersKeyState::Pressed {
            bits |= LEFT_OPTION;
        }
        if event.ralt_state() == ModifiersKeyState::Pressed {
            bits |= RIGHT_OPTION;
        }
    }
    RawInput::Modifiers(bits)
}

/// Carries an IME notification without treating preedit as committed text.
pub(super) fn ime(event: Ime) -> RawInput {
    // Preedit is not committed text. Its state only suppresses a matching
    // keydown until a later Commit has finalized the composition.
    RawInput::Ime(match event {
        Ime::Preedit(text, _) => RawIme::Preedit(text),
        Ime::Commit(text) => RawIme::Commit(text),
        Ime::Enabled => RawIme::Enabled,
        Ime::Disabled => RawIme::Disabled,
    })
}
