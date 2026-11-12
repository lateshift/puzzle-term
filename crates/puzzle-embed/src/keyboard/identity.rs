//! Separates layout identity, produced text and physical US identity (M7.2).
//!
//! Winit stores physical codes as Code(KeyY) in older recordings. Removing
//! only that wrapper retains the exact W3C spelling used by keybindings.
//! The key-without-modifiers supplement supplies the user's layout, not US.
//! These are deliberately different: German z lives on physical KeyY.
//! The numeric pad is identified before its layout text, preserving DECKPAM
//! and kitty KP codes even when NumLock changes its logical identity.
use super::{Modifiers, RawKey, VtKey, VtKeyEvent, map_key};
use puzzle_term::vt::input::physical::us_scalar;

/// Extracts a known physical spelling while preserving existing raw recordings.
fn code(raw: &RawKey) -> &str {
    raw.physical
        .strip_prefix("Code(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(&raw.physical)
}

/// Adds wire alternates without confusing produced Shift text with the key.
pub(super) fn alternates(event: &mut VtKeyEvent, raw: &RawKey, use_base: bool) {
    let code = code(raw);
    if !code.is_empty() && !code.starts_with("Unidentified") {
        event.code = Some(code.into());
        event.base = us_scalar(code);
    }
    // Only Shift's produced layout scalar belongs in the optional shifted field.
    // Without Shift, even an uppercase CapsLock scalar is not a shifted alternate.
    // Option-as-Alt discards Option-produced text; winit does not expose its
    // Shift-only alternate, so leave that optional field absent rather than guess.
    if !use_base
        && event.modifiers.has(Modifiers::SHIFT)
        && let Some(VtKey::Char(ch)) = map_key(&raw.logical)
    {
        event.shifted = Some(ch);
    }
}

/// Resolves typed keypad identity and the layout's unshifted kitty key.
pub(super) fn key(raw: &RawKey, mapped: Option<VtKey>, modifiers: u8) -> Option<VtKey> {
    let code = code(raw);
    if let Some(digit) = code.strip_prefix("Numpad")
        && digit.len() == 1
        && digit.as_bytes()[0].is_ascii_digit()
    {
        return Some(VtKey::Keypad(digit.as_bytes()[0] - b'0'));
    }
    // NumLock may turn a numpad digit into a navigation logical key.
    // Physical identity therefore wins before the ordinary key mapper.
    // Operators retain their KP identity even when a layout emits comma.
    let keypad = match code {
        "NumpadEnter" => Some(VtKey::KeypadEnter),
        "NumpadAdd" => Some(VtKey::KeypadOp('+')),
        "NumpadSubtract" => Some(VtKey::KeypadOp('-')),
        "NumpadMultiply" => Some(VtKey::KeypadOp('*')),
        "NumpadDivide" => Some(VtKey::KeypadOp('/')),
        "NumpadDecimal" => Some(VtKey::KeypadOp('.')),
        "NumpadEqual" => Some(VtKey::KeypadOp('=')),
        _ => None,
    };
    // Location is an independent fallback when a host cannot identify its code.
    if let Some(key) = keypad.or_else(|| located_keypad(raw, mapped)) {
        return Some(key);
    }
    // Kitty's first key field is unshifted *layout* identity. Legacy uses the
    // shifted alternate, and shortcut matching uses produced layout text.
    if modifiers & Modifiers::SHIFT != 0 && matches!(mapped, Some(VtKey::Char(_))) {
        map_key(&raw.base).or(mapped)
    } else {
        mapped
    }
}

/// Distinguishes keypad Enter and printable numpad keys when code is absent.
fn located_keypad(raw: &RawKey, mapped: Option<VtKey>) -> Option<VtKey> {
    if raw.location != "Numpad" {
        return None;
    }
    // A location-only decimal comma selects KP_DECIMAL for kitty/DECKPAM.
    // Its original text stays on KeyEvent for normal numeric mode.
    match mapped? {
        VtKey::Enter => Some(VtKey::KeypadEnter),
        VtKey::Char(ch @ '0'..='9') => Some(VtKey::Keypad(ch as u8 - b'0')),
        VtKey::Char(ch @ ('+' | '-' | '*' | '/' | '.' | '=')) => Some(VtKey::KeypadOp(ch)),
        VtKey::Char(',') => Some(VtKey::KeypadOp('.')),
        _ => None,
    }
}
