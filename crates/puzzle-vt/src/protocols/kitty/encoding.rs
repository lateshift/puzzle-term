//! Origin: kitty; <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>.
//! Kitty key-event encoding from the published protocol (§6.8).
//!
//! Progressive flags request ambiguity removal, transition reports,
//! alternate layout codes, all-key reports, and committed text.
//!
//! The five flags are independent wire commitments. Bit 1 prevents ESC
//! ambiguity; bit 2 reports repeat and release; bit 4 adds shifted and
//! physical key identities; bit 8 reports even ordinary text keys as
//! escape sequences; bit 16 adds the text those keys produced. A plain
//! key stays UTF-8 while bit 8 is off, preserving shell interaction.
//! Functional keys with historical CSI letter/tilde forms retain those
//! forms under enhancement. Only keys without such a form use CSI u.
//! These choices follow the kitty keyboard protocol's functional-key
//! and progressive-enhancement tables, linked from design §6.8.
//!
//! The renderer and grid do not inspect these bytes. They travel back to
//! the application through a future transport, where ambiguity would
//! become an application input bug rather than a visual rendering bug.
//! Keeping this conversion local makes the wire table auditable.
use crate::input::InputModes;
use crate::input::{Key, KeyEvent, KeyPhase};

/// Emits kitty's canonical CSI u form where progressive flags require it.
pub(crate) fn kitty(event: &KeyEvent, modes: InputModes) -> Vec<u8> {
    let flags = modes.kitty_flags;
    // Kitty's Report alternate keys is a pure form enhancement. Flag 4 alone
    // must not turn a legacy Ctrl byte or application keypad into CSI u.
    // Source: sw.kovidgoyal.net/kitty/keyboard-protocol/#report-alternate-keys
    if flags & !4 == 0 {
        return crate::input::key::legacy(event, modes);
    }
    // The application cannot interpret release without event-type flag 2.
    if event.phase == KeyPhase::Release && flags & 2 == 0 {
        return Vec::new();
    }
    if let Some(bytes) = plain_text(event, flags) {
        return bytes;
    }
    // The published function table keeps these CSI families even after
    // negotiation; encoding an arrow as a PUA CSI u would break clients.
    if let Some(bytes) = functional(event, flags) {
        return bytes;
    }
    // Enter, Tab and Backspace remain legacy unless all-key reporting was
    // requested, allowing `reset` in a shell after an application crash.
    if flags & 8 == 0 && matches!(event.key, Key::Enter | Key::Tab | Key::Backspace) {
        if event.phase == KeyPhase::Release {
            return Vec::new();
        }
        return crate::input::key::legacy(event, modes);
    }
    let mut head = alternate_head(event, flags);
    let phase = match event.phase {
        KeyPhase::Press => 1,
        KeyPhase::Repeat => 2,
        KeyPhase::Release => 3,
    };
    let needs_mods = event.modifiers.0 != 0 || (flags & 2 != 0 && phase != 1);
    // Event type is a colon subfield of modifiers. With no modifiers, 1
    // still has to appear before :2 or :3 (kitty Event types table).
    if needs_mods {
        head.push_str(&format!(";{}", event.modifiers.parameter()));
    }
    if flags & 2 != 0 && phase != 1 {
        head.push_str(&format!(":{phase}"));
    }
    if flags & 16 != 0
        && flags & 8 != 0
        && let Some(text) = &event.text
    {
        // Text is encoded as decimal Unicode scalars, never raw C0 bytes.
        append_text(&mut head, text);
    }
    format!("\x1b[{head}u").into_bytes()
}

/// Keeps ordinary text in UTF-8 until an application asks for all keys.
///
/// The `Option` distinguishes a handled text key from a functional key;
/// an empty vector means a text-key release was intentionally suppressed.
fn plain_text(event: &KeyEvent, flags: u8) -> Option<Vec<u8>> {
    let Key::Char(ch) = event.key else {
        return None;
    };
    // Shift changes the produced scalar, but Ctrl/Alt need distinct CSI
    // reports once ambiguity removal has been negotiated.
    if flags & 8 != 0 || event.modifiers.0 & !1 != 0 {
        return None;
    }
    // A release has no printable representation without all-key mode.
    if event.phase == KeyPhase::Release {
        return Some(Vec::new());
    }
    // IME committed text may contain several scalars and takes priority
    // over the key's layout identity.
    if let Some(text) = &event.text {
        return Some(text.as_bytes().to_vec());
    }
    let scalar = if event.modifiers.0 & 1 != 0 {
        event.shifted.unwrap_or(ch)
    } else {
        ch
    };
    let mut encoded = [0; 4];
    Some(scalar.encode_utf8(&mut encoded).as_bytes().to_vec())
}

/// Preserves kitty's CSI letter and tilde functional-key families.
fn functional(event: &KeyEvent, flags: u8) -> Option<Vec<u8>> {
    let (number, final_byte) = match event.key {
        Key::Up => (1, 'A'),
        Key::Down => (1, 'B'),
        Key::Right => (1, 'C'),
        Key::Left => (1, 'D'),
        Key::Home => (1, 'H'),
        Key::End => (1, 'F'),
        Key::Insert => (2, '~'),
        Key::Delete => (3, '~'),
        Key::PageUp => (5, '~'),
        Key::PageDown => (6, '~'),
        Key::Function(1) => (1, 'P'),
        Key::Function(2) => (1, 'Q'),
        // Kitty uses 13~ for F3 because CSI R also means cursor report.
        Key::Function(3) => (13, '~'),
        Key::Function(4) => (1, 'S'),
        Key::Function(n @ 5..=12) => (
            u32::from(crate::input::key::FUNCTION_TILDE[usize::from(n - 5)]),
            '~',
        ),
        _ => return None,
    };
    let phase = match event.phase {
        KeyPhase::Press => 1,
        KeyPhase::Repeat => 2,
        KeyPhase::Release => 3,
    };
    let modifiers = event.modifiers.parameter();
    // Omit the default number only for the letter-final family. Tilde
    // keys need the number even without any modifiers.
    let head = if modifiers == 1 && phase == 1 {
        if final_byte == '~' {
            number.to_string()
        } else {
            String::new()
        }
    } else if flags & 2 != 0 && phase != 1 {
        format!("{number};{modifiers}:{phase}")
    } else {
        format!("{number};{modifiers}")
    };
    Some(format!("\x1b[{head}{final_byte}").into_bytes())
}

/// Appends printable committed text as kitty decimal code points.
///
/// C0/C1 controls cannot be injected through the optional text field.
fn append_text(head: &mut String, text: &str) {
    // The first scalar follows a semicolon; later scalars are colon
    // subfields so the application sees one associated text sequence.
    let mut chars = text.chars().filter(|ch| !ch.is_control());
    if let Some(first) = chars.next() {
        head.push(';');
        head.push_str(&(first as u32).to_string());
        for ch in chars {
            head.push(':');
            head.push_str(&(ch as u32).to_string());
        }
    }
}

/// Resolves kitty's published functional key codes, including PUA keys.
///
/// Ordinary C0 keys retain their numbers for a canonical CSI u report.
/// F13–F35, keypad, media and modifier keys live in the PUA, avoiding a
/// collision with real Unicode text. Named media and modifier keys use
/// `NamedKey`, so platform adapters cannot invent an arbitrary PUA code.
fn kitty_code(key: Key) -> u32 {
    match key {
        // Kitty identifies the physical logical key by its unshifted form;
        // Shift's effect belongs in modifiers and optional shifted scalar.
        Key::Char(ch) => ch.to_ascii_lowercase() as u32,
        // Escape, Enter, Tab and Backspace use their legacy control numbers.
        Key::Escape => 27,
        Key::Enter => 13,
        Key::Tab => 9,
        Key::Backspace => 127,
        Key::Insert => 2,
        Key::Delete => 3,
        Key::PageUp => 5,
        Key::PageDown => 6,
        // The first cursor PUA block is contiguous in kitty's code table.
        Key::Left => 57350,
        Key::Right => 57351,
        Key::Up => 57352,
        Key::Down => 57353,
        Key::Home => 57354,
        Key::End => 57355,
        // F1–F12 normally use CSI letter/tilde, handled above.
        Key::Function(n @ 1..=12) => 57363 + u32::from(n),
        // F13–F35 start a distinct contiguous PUA range.
        Key::Function(n @ 13..=35) => 57376 + u32::from(n - 13),
        // Keypad identity matters when the all-keys flag is enabled.
        Key::Keypad(n @ 0..=9) => 57399 + u32::from(n),
        Key::KeypadEnter => 57414,
        Key::KeypadOp(ch) => match ch {
            // The order follows kitty's decimal/divide/multiply/minus/plus
            // constants; it is not ASCII punctuation order.
            '.' => 57409,
            '/' => 57410,
            '*' => 57411,
            '-' => 57412,
            '+' => 57413,
            '=' => 57415,
            _ => 0,
        },
        // The enum maps identity to the protocol's published PUA number.
        Key::Named(named) => named.code(),
        _ => 0,
    }
}

/// Colon subfields preserve layout Shift output and physical US shortcut identity.
fn alternate_head(event: &KeyEvent, flags: u8) -> String {
    let mut head = kitty_code(event.key).to_string();
    // Alternate codes are colon fields within the key parameter. An empty
    // shifted field must remain when only the physical base is known.
    if flags & 4 != 0 && (event.shifted.is_some() || event.base.is_some()) {
        head.push(':');
        if let Some(ch) = event
            .shifted
            .filter(|_| event.modifiers.has(crate::input::Modifiers::SHIFT))
        {
            head.push_str(&(ch as u32).to_string());
        }
        if let Some(ch) = event.base {
            head.push(':');
            head.push_str(&(ch as u32).to_string());
        }
    }
    head
}
