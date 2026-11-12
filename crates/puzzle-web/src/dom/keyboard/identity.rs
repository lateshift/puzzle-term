//! W3C named-key and numeric-pad identity, before layout text is interpreted.
//!
//! NumLock can change a keypad's key to End or Insert; code still identifies
//! the pad. Its decimal separator stays in KeyEvent.text for normal mode,
//! while the typed dot identity selects SS3/kitty's independent pad code.
//!
//! DOM key is layout text or a W3C name, not a hardware scan code. A one-scalar
//! value therefore stays a character even if it is non-ASCII. Escape, Tab and
//! arrows use the neutral enums so the core can choose DECCKM and kitty forms.
//! A keypad digit is checked as an ASCII byte only after its suffix length
//! was validated; arbitrary synthetic events cannot index outside the string.
//! Location 3 handles hosts that omit NumpadEnter's code but retain its location.
//! An unknown named key is left to the browser: it must not become a guessed
//! printable character or an accidental control sequence. Modifier identity
//! is supplied separately by Keyboard and is not inferred from these names.
//! The function-key upper bound is kitty's published 35-key range (§6.8);
//! legacy byte availability is deliberately decided by the encoder, not here.
use super::scalar;
use puzzle_term::vt::input::Key;

/// Resolves browser key values without importing a platform keyboard library.
pub(super) fn key(name: &str, code: &str, location: u32) -> Option<Key> {
    if let Some(digit) = code.strip_prefix("Numpad")
        && digit.len() == 1
        && digit.as_bytes()[0].is_ascii_digit()
    {
        return Some(Key::Keypad(digit.as_bytes()[0] - b'0'));
    }
    // Physical codes take priority over layout characters or NumLock names.
    match code {
        "NumpadEnter" => return Some(Key::KeypadEnter),
        "NumpadAdd" => return Some(Key::KeypadOp('+')),
        "NumpadSubtract" => return Some(Key::KeypadOp('-')),
        "NumpadMultiply" => return Some(Key::KeypadOp('*')),
        "NumpadDivide" => return Some(Key::KeypadOp('/')),
        "NumpadDecimal" => return Some(Key::KeypadOp('.')),
        "NumpadEqual" => return Some(Key::KeypadOp('=')),
        _ => {}
    }
    if let Some(ch) = scalar(name) {
        return Some(Key::Char(ch));
    }
    // Kitty defines F13–35 even though legacy xterm only defines F1–12.
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u8>().ok())
        && (1..=35).contains(&n)
    {
        return Some(Key::Function(n));
    }
    Some(match name {
        "ArrowUp" => Key::Up,
        "ArrowDown" => Key::Down,
        "ArrowLeft" => Key::Left,
        "ArrowRight" => Key::Right,
        "Home" => Key::Home,
        "End" => Key::End,
        "Insert" => Key::Insert,
        "Delete" => Key::Delete,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Enter" if location == 3 => Key::KeypadEnter,
        "Enter" => Key::Enter,
        "Tab" => Key::Tab,
        "Backspace" => Key::Backspace,
        "Escape" => Key::Escape,
        _ => return None,
    })
}
