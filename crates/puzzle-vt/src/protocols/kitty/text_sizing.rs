//! OSC 66 wire grammar from <https://sw.kovidgoyal.net/kitty/text-sizing-protocol/>.
//! Metadata uses colons; only the first two semicolons are structural. Invalid
//! pairs are ignored independently, while unsafe/truncated text is rejected whole.
use crate::{Terminal, multicell::TextSizing};

/// OSC 66 permits at most 4096 safe UTF-8 bytes in one text payload.
/// Reference: <https://sw.kovidgoyal.net/kitty/text-sizing-protocol/> (escape code).
const MAX_TEXT_BYTES: usize = 4096;

/// Parses one exact raw OSC body after the bounded observer sees its terminator.
pub(crate) fn dispatch(term: &mut Terminal, raw: &[u8]) {
    // Dispatch is limited to this defining family; the observer also serves
    // other OSC protocols without assigning their wire grammar here.
    let Some(body) = raw.strip_prefix(b"66;") else {
        return;
    };
    // Only metadata ends at this semicolon. Later semicolons are visible text,
    // so vte's finite parameter-slice table cannot be the payload source.
    let Some(split) = body.iter().position(|b| *b == b';') else {
        return;
    };
    // Replacement UTF-8 would change the requested text and discovery widths.
    // Reject the complete operation while leaving subsequent output consumable.
    let Ok(text) = std::str::from_utf8(&body[split + 1..]) else {
        return;
    };
    // Safe UTF-8 excludes C0/C1 controls even if vte would silently discard them.
    if text.is_empty()
        || text.len() > MAX_TEXT_BYTES
        || text.chars().any(char::is_control)
        || term.clipboard_wire_invalid()
    {
        return;
    }
    // Font selection and fractional ink transforms belong to later rendering.
    // All wire validation finishes before the VT mutates occupancy or cursor.
    term.print_sized(text, metadata(&body[..split]));
}

/// Interprets independent optional pairs, retaining the last valid duplicate.
fn metadata(raw: &[u8]) -> TextSizing {
    let mut sizing = TextSizing::default();
    // Bad optional pairs do not poison valid neighbors. Assigning in wire order
    // also makes the last valid duplicate authoritative, as in the reference.
    for pair in raw.split(|b| *b == b':') {
        let Some((key, value)) = pair.split_first() else {
            continue;
        };
        let Some(value) = value.strip_prefix(b"=") else {
            continue;
        };
        // Signs, whitespace and decimal overflow are invalid numeric pairs,
        // rather than values to clamp into a different sizing request.
        if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let Some(number) = std::str::from_utf8(value)
            .ok()
            .and_then(|s| s.parse::<u8>().ok())
        else {
            continue;
        };
        // Protocol ranges bound rectangles before any grid arithmetic occurs.
        // Unknown keys are ignored so newer clients can add optional metadata.
        match (*key, number) {
            (b's', 1..=7) => sizing.scale = number,
            (b'w', 0..=7) => sizing.width = number,
            (b'n', 0..=15) => sizing.numerator = number,
            (b'd', 0..=15) => sizing.denominator = number,
            (b'v', 0..=2) => sizing.vertical = number,
            (b'h', 0..=2) => sizing.horizontal = number,
            _ => {}
        }
    }
    sizing
}
