//! DECKPAM numeric keypad byte families from xterm ctlseqs (§6.8).
//!
//! Application mode uses SS3 finals; ordinary mode types characters.
//! The distinction belongs to terminal mode 66, not to the keyboard layout.
use super::{Modifiers, legacy_char};

/// Legacy DECKPAM maps digits to SS3 p–y.
///
/// Plain numeric keypad mode must still type the corresponding digit.
pub(super) fn digit(digit: u8, application: bool) -> Vec<u8> {
    if application {
        format!("\x1bO{}", char::from(b'p' + digit)).into_bytes()
    } else {
        vec![b'0' + digit]
    }
}

/// Legacy DECKPAM uses punctuation finals, unlike the main keyboard.
///
/// Returning no bytes for an unknown operator avoids a fabricated command.
pub(super) fn operator(operator: char, application: bool) -> Vec<u8> {
    if application {
        let final_byte = match operator {
            '+' => 'k',
            '-' => 'm',
            '*' => 'j',
            '/' => 'o',
            '.' => 'n',
            '=' => 'X',
            _ => return Vec::new(),
        };
        format!("\x1bO{final_byte}").into_bytes()
    } else {
        legacy_char(operator, Modifiers(0), None)
    }
}
