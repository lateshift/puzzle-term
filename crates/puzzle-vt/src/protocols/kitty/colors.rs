//! Kitty OSC 21 colour control, using the common colour table and decoder.
//! Reference: https://sw.kovidgoyal.net/kitty/color-stack/#setting-and-querying-colors
//! Requests execute in order, including a set followed by a query in one packet.
//! Unknown names are Base64-encoded, never echoed as executable terminal bytes.
//!
//! OSC 21 carries semicolon-separated key operations. A bare key restores its
//! theme default; key=? queries; key=<spec> assigns a shared X11 colour. Empty
//! values explicitly request dynamic semantic colour. Palette slots cannot
//! be dynamic, because an indexed cell needs a concrete colour to draw.
//! Queries and assignments may be interleaved. Resolving while walking the
//! packet preserves ordering rather than answering from its initial state.
//! One response carries all queried/unknown fields, with the input terminator.
//! Set-only packets allocate no response buffer; String::new is heap-free.
//! VTE already bounded the input. No key can access clipboard, files or host
//! state: queries expose only this terminal's installed colour table.
//! Unsupported transparent-background slots are reported as unknown, never
//! silently advertised. Their alpha/matching semantics are outside this table.
//! The base64 helper handles arbitrary key bytes without assuming UTF-8; it
//! emits no padding as specified by kitty, and emits only safe ASCII symbols.
use crate::{
    color_spec,
    color_table::{ColorSlot, ColorTable, ColorValue, DynamicColor},
    terminal::Terminal,
};
use std::fmt::Write;

/// Handles one bounded batch, emitting one response only if any field needs one.
pub(crate) fn dispatch(term: &mut Terminal, params: &[&[u8]], bell: bool) {
    // The buffer stays empty unless a query or unsupported key needs a reply.
    let mut reply = String::new();
    for part in params.iter().skip(1) {
        // Split only the first equals sign; malformed value syntax belongs to the parser.
        let (key, value) = part
            .iter()
            .position(|b| *b == b'=')
            .map_or((*part, None), |i| (&part[..i], Some(&part[i + 1..])));
        // An unsupported name is reported even when its operation was a set/reset.
        let Some(slot) = slot(key) else {
            begin_field(&mut reply, b"unknown");
            base64(&mut reply, key);
            continue;
        };
        match value {
            Some(b"?") => {
                // Undefined semantic roles report an empty value, not guessed RGB.
                begin_field(&mut reply, key);
                if let Some(rgb) = term.colors.get(slot).rgb() {
                    let _ = write!(
                        reply,
                        "rgb:{:04x}/{:04x}/{:04x}",
                        u16::from(rgb[0]) * 257,
                        u16::from(rgb[1]) * 257,
                        u16::from(rgb[2]) * 257
                    );
                }
            }
            // Absence of an equals sign resets; it is distinct from an empty value.
            None => change(term, slot, None),
            Some(b"") if !matches!(slot, ColorSlot::Palette(_)) => {
                change(term, slot, Some(ColorValue::Dynamic))
            }
            Some(bytes) => {
                // Invalid specs leave the previous override intact and processing continues.
                if let Some(rgb) = color_spec::parse(bytes) {
                    change(term, slot, Some(ColorValue::Rgb(rgb)));
                }
            }
        }
    }
    // A write-only request must produce no unsolicited acknowledgement.
    if !reply.is_empty() {
        term.replies.extend_from_slice(reply.as_bytes());
        term.replies
            .extend_from_slice(if bell { b"\x07" } else { b"\x1b\\" });
    }
}

/// Maps only supported keys; transparent-background colours have no renderer yet.
fn slot(key: &[u8]) -> Option<ColorSlot> {
    use DynamicColor::*;
    // Literal names come from kitty; xterm numeric roles are not names here.
    let role = match key {
        b"foreground" => Foreground,
        b"background" => Background,
        b"cursor" => Cursor,
        b"cursor_text" => CursorText,
        b"selection_background" => SelectionBackground,
        b"selection_foreground" => SelectionForeground,
        b"visual_bell" => VisualBell,
        _ => {
            // Only 0–255 are kitty palette keys; special xterm aliases use OSC 4/5.
            let index = std::str::from_utf8(key).ok()?.parse::<u8>().ok()?;
            return ColorTable::indexed(u16::from(index));
        }
    };
    Some(ColorSlot::Dynamic(role))
}

/// Marks paint dirty for both concrete overrides and reset/dynamic changes.
fn change(term: &mut Terminal, slot: ColorSlot, value: Option<ColorValue>) {
    if let Some(value) = value {
        term.colors.set(slot, value);
    } else {
        term.colors.reset(slot);
    }
    // Recolour old rows without changing their cell contents or shaping inputs.
    term.screen_mut().damage.full = true;
}

/// Starts a safe known field in the single ordered OSC 21 response.
fn begin_field(reply: &mut String, key: &[u8]) {
    if reply.is_empty() {
        reply.push_str("\x1b]21");
    }
    reply.push(';');
    // Only literal keys or validated decimal numbers reach this helper.
    reply.push_str(std::str::from_utf8(key).unwrap_or(""));
    reply.push('=');
}

/// Emits standard unpadded Base64 so arbitrary unknown names cannot inject replies.
fn base64(output: &mut String, bytes: &[u8]) {
    // Standard alphabet, not the URL-safe variant; kitty requests this wire encoding.
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in bytes.chunks(3) {
        // Zero-fill missing tail bytes, then emit only the meaningful symbols.
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        // Two symbols for one byte, three for two, four for a complete triple.
        for i in 0..chunk.len() + 1 {
            output.push(ALPHABET[((bits >> (18 - i * 6)) & 63) as usize] as char);
        }
    }
}
