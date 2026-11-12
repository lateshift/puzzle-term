//! MacOS Ctrl+letter callback sequences used by the headless adapter (§13.4).
//!
//! The private `target/recordings/keys-macos.pzrec` supplies observed key
//! fields and event order. Its contents cannot be checked in because it
//! includes the user's screen; only entry numbers and neutral letter fields
//! appear here. Most letters were observed directly. The user explicitly
//! allowed filling the six absent lowercase chords from the uniform pattern
//! shared by observed A, B, D, U and X events on 2026-09-30. Those six are
//! marked inferred, never represented as observed recording rows.
//! `RawInput::Modifiers` sets the adapter state before a keydown and clears
//! it after keyup. The actual recording also includes physical ControlLeft
//! callbacks, which produce no terminal command and do not change this
//! adapter state; omitting them keeps the scenario focused on emitted bytes.
//!
//! This table is a diagnostic seam, not a keyboard layout implementation.
//! It cannot replace real winit events in the window, and it must not be
//! expanded to unrelated chords by extrapolating from US-letter positions.
//! The six inferred lowercase rows are confined to the user's requested
//! Ctrl+A–Z matrix. They use exactly the keydown, keyup and modifier order
//! seen for neighboring recorded letters. Their provenance is explicit so
//! a later macOS recording can replace inference with an observed entry.
//! `Code(KeyX)` is retained as evidence for the physical-key field even
//! though this adapter currently selects the logical character. A later
//! physical binding must not silently reinterpret these scenario events.

use super::raw::{RawInput, RawKey, RawLogical};

/// Provenance for the first observed keydown or an explicit inference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// One-based entry number in the private macOS recording.
    Recorded(usize),
    /// User-authorized completion from the recorded uniform callback shape.
    Inferred,
}

/// Returns host-shaped callbacks for one lower-case Ctrl chord.
pub fn for_chord(chord: &str) -> Result<(Vec<RawInput>, Source), String> {
    let (letter, shifted) = parse_chord(chord)?;
    let source = provenance(letter, shifted);
    let value = if shifted {
        letter.to_ascii_uppercase()
    } else {
        letter
    };
    let logical = RawLogical::Character(value.to_string());
    let physical = format!("Code(Key{})", letter.to_ascii_uppercase());
    // In the recording, macOS supplies printable letter text on keydown
    // under Ctrl, but no text on keyup. The adapter uses the logical key
    // and suppresses modified text fallback, so this field tests that guard.
    let bits = 4 | u8::from(shifted);
    let press = RawKey {
        logical: logical.clone(),
        base: RawLogical::Character(letter.to_string()),
        physical: physical.clone(),
        text: Some(value.to_string()),
        location: "Standard".into(),
        pressed: true,
        repeat: false,
    };
    let release = RawKey {
        // Kitty can request release reports. Retaining the same logical
        // identity on keyup lets the VT encoder decide from active flags.
        logical,
        base: RawLogical::Character(letter.to_string()),
        physical,
        text: None,
        location: "Standard".into(),
        pressed: false,
        repeat: false,
    };
    Ok((
        vec![
            RawInput::Modifiers(bits),
            RawInput::Key(press),
            RawInput::Key(release),
            RawInput::Modifiers(0),
        ],
        source,
    ))
}

/// Keeps the accepted vocabulary narrower than the M7 keybinding grammar.
fn parse_chord(chord: &str) -> Result<(char, bool), String> {
    let (part, shifted) = if let Some(part) = chord.strip_prefix("ctrl+shift+") {
        (part, true)
    } else if let Some(part) = chord.strip_prefix("ctrl+") {
        (part, false)
    } else {
        return Err("input needs ctrl+letter or ctrl+shift+d".into());
    };
    let mut chars = part.chars();
    let letter = chars.next().ok_or("input lacks letter")?;
    if chars.next().is_some() || !letter.is_ascii_lowercase() || (shifted && letter != 'd') {
        return Err("K4 input supports lowercase ctrl+a..z and ctrl+shift+d".into());
    }
    Ok((letter, shifted))
}

/// Names each observed entry without exposing adjacent private recording data.
fn provenance(letter: char, shifted: bool) -> Source {
    if shifted {
        // Ctrl+Shift+D: physical ShiftLeft at entry 657, modifiers 5 at
        // entry 659, and the uppercase D keydown at entry 661.
        return Source::Recorded(661);
    }
    Source::Recorded(match letter {
        'a' => 1016,
        'b' => 1103,
        'c' => 1122,
        'd' => 212,
        'f' => 1456,
        'h' => 1640,
        'i' => 1206,
        'j' => 1408,
        'k' => 1170,
        'l' => 1469,
        'm' => 1269,
        'n' => 1302,
        'o' => 1189,
        's' => 1077,
        'u' => 281,
        'v' => 1510,
        'w' => 1161,
        'x' => 469,
        'y' => 1284,
        'z' => 1373,
        // E, G, P, Q, R and T were absent from this recording. The user
        // authorized these six to use the identical observed callback shape.
        _ => return Source::Inferred,
    })
}

#[cfg(test)]
/// Every accepted chord gets exactly one press and one release.
mod tests;
