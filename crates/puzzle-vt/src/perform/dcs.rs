//! DCS DECRQSS and XTGETTCAP with bounded payloads (§6.5, §6.12).
//!
//! vte streams DCS bytes one at a time. Unknown requests are discarded
//! without buffering; the two allowed query forms retain at most 4096
//! bytes, which suffices for status names and a short termcap list.
//! DECRQSS reports current state, not a hardcoded reset string. XTGETTCAP
//! decodes hexadecimal capability names and encodes values the same way,
//! following xterm ctlseqs' Request Termcap/Terminfo String contract.
//!
//! DCS is a streaming control string. `hook` selects the family from its
//! introducer, `put` sees body bytes, and `unhook` runs when ST closes it.
//! There is no reason to retain an unsupported sixel or passthrough body:
//! doing so would let an application consume arbitrary heap memory.
//! Even known queries are limited to 4096 bytes, far more than the short
//! capability names this emulator advertises. An oversized known query
//! is discarded as a whole rather than interpreted from a truncated prefix.
//!
//! DECRQSS names an existing status string and returns the terminal's
//! current representation of it. It must not reset graphic rendition or
//! margins just to construct a reply. The success reply begins `1$r`,
//! and unsupported names receive `0$r`. The five supported names are SGR,
//! top/bottom margins, left/right margins, cursor style and protection.
//! xterm ctlseqs defines the final wire form, including the DCS/ST wrapper.
//!
//! XTGETTCAP requests ASCII capability names encoded as hex octets. Its
//! response repeats each name, followed by `=` and a hex-encoded value.
//! A semicolon can separate several names in one request. We expose a
//! bundled terminfo source plus legacy names. Unknown or malformed names get a failure reply,
//! so a program does not mistake an unsupported capability for an empty
//! supported one. No host environment variables or filesystem data enter
//! these replies (§16).

/// Recognized DCS request family during vte passthrough.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DcsKind {
    /// Unsupported request: consume bytes without retaining them.
    None,
    /// DCS $ q: request current status string.
    Status,
    /// DCS + q: request termcap or terminfo capability.
    Termcap,
}

use crate::{
    cell::{Attrs, CellFlags, PackedColor},
    terminal::Terminal,
};
/// The checked-in terminfo source is also the XTGETTCAP capability table.
mod terminfo;

/// Starts only the two safe, supported DCS query families.
pub(super) fn hook(term: &mut Terminal, intermediates: &[u8], ignore: bool, action: char) {
    // Each new DCS begins with empty scratch state, even if the previous
    // string was cancelled before its terminator arrived.
    term.dcs_payload.clear();
    term.dcs_truncated = false;
    term.dcs_kind = if ignore || action != 'q' {
        // vte already flags malformed introducers; do not buffer their body.
        DcsKind::None
    } else {
        match intermediates {
            b"$" => DcsKind::Status,
            b"+" => DcsKind::Termcap,
            _ => DcsKind::None,
        }
    };
}

/// Buffers recognized request bytes without permitting unbounded growth.
pub(super) fn put(term: &mut Terminal, byte: u8) {
    // Unknown DCS families must have constant memory cost per input byte.
    if term.dcs_kind == DcsKind::None {
        return;
    }
    if term.dcs_payload.len() < 4096 {
        // Known query bodies stay below the local DCS ceiling.
        term.dcs_payload.push(byte);
    } else {
        // Remember overflow so the valid-looking prefix cannot be answered.
        term.dcs_truncated = true;
    }
}

/// Sends one reply after ST, or rejects a truncated request safely.
pub(super) fn unhook(term: &mut Terminal) {
    // A reply is produced only after ST, preserving parser order.
    if term.dcs_truncated {
        tracing::warn!(event = "vt.dcs_truncated", "discarded oversized DCS query");
    } else {
        match term.dcs_kind {
            DcsKind::Status => status(term),
            DcsKind::Termcap => termcap(term),
            DcsKind::None => {}
        }
    }
    term.dcs_kind = DcsKind::None;
    // Reuse the same backing allocation for the next small query.
    term.dcs_payload.clear();
}

/// Answers the five DECRQSS status names promised by design §6.5.
fn status(term: &mut Terminal) {
    // The status name is raw ASCII because DECRQSS has no numeric params.
    let value = match term.dcs_payload.as_slice() {
        b"m" => Some(sgr_state(term)),
        // DECSTBM and DECSLRM report one-based inclusive boundaries.
        b"r" => Some(format!(
            "{};{}r",
            term.margins.top + 1,
            term.margins.bottom + 1
        )),
        b"s" => Some(format!(
            "{};{}s",
            term.margins.left + 1,
            term.margins.right + 1
        )),
        b" q" => Some(format!("{} q", term.cursor_style)),
        // DECSCA protection is an attribute of newly printed cells.
        b"\"q" => Some(format!(
            "{}\"q",
            u8::from(term.pen.flags.contains(CellFlags::PROTECTED))
        )),
        _ => None,
    };
    if let Some(value) = value {
        // `1$r` is xterm's accepted-status marker.
        term.replies
            .extend_from_slice(format!("\x1bP1$r{value}\x1b\\").as_bytes());
    } else {
        // `0$r` is explicit failure, not an empty successful status.
        term.replies.extend_from_slice(b"\x1bP0$r\x1b\\");
    }
}

/// Serializes current SGR attributes for DECRQSS without changing the pen.
fn sgr_state(term: &Terminal) -> String {
    // Start from reset so the reply is self-contained if a program feeds
    // it back as an SGR sequence after DECRQSS.
    let mut codes = vec!["0".to_owned()];
    let attrs = term.pen.attrs;
    for (flag, code) in [
        (Attrs::BOLD, "1"),
        (Attrs::DIM, "2"),
        (Attrs::ITALIC, "3"),
        (Attrs::BLINK, "5"),
        (Attrs::INVERSE, "7"),
        (Attrs::HIDDEN, "8"),
        (Attrs::STRIKE, "9"),
        (Attrs::OVERLINE, "53"),
    ] {
        // Emit only active booleans; each code preserves the others.
        if attrs.contains(flag) {
            codes.push(code.to_owned());
        }
    }
    if attrs.underline_style() > 0 {
        // Colon form retains the style number, including curly/dotted/dashed.
        codes.push(format!("4:{}", attrs.underline_style()));
    }
    push_color(&mut codes, "38", term.pen.fg);
    push_color(&mut codes, "48", term.pen.bg);
    if let Some(color) = term.underline_color {
        // Sparse underline color is independent of cell foreground color.
        push_color(&mut codes, "58", color);
    }
    format!("{}m", codes.join(";"))
}

/// Appends one tagged palette or RGB color in canonical semicolon form.
fn push_color(codes: &mut Vec<String>, prefix: &str, color: PackedColor) {
    // PackedColor's high byte distinguishes default, indexed and RGB tags.
    match color.0 >> 24 {
        1 => codes.push(format!("{prefix};5;{}", color.0 & 0xff)),
        2 => codes.push(format!(
            "{prefix};2;{};{};{}",
            (color.0 >> 16) & 0xff,
            (color.0 >> 8) & 0xff,
            color.0 & 0xff
        )),
        _ => {}
    }
}

/// Answers a small explicit capability table in xterm's hex wire format.
fn termcap(term: &mut Terminal) {
    // A multi-name request succeeds only if every name is understood.
    let mut answer = String::new();
    for encoded in term.dcs_payload.split(|byte| *byte == b';') {
        let Some(name) = decode_hex(encoded) else {
            // Invalid hex cannot be reflected into a success reply.
            term.replies.extend_from_slice(b"\x1bP0+r\x1b\\");
            return;
        };
        let value = match name.as_slice() {
            // TN must agree with the PTY's TERM even when the bundled
            // entry was unavailable and the platform chose its fallback.
            b"TN" => Some(term.term_name.as_bytes().to_vec()),
            b"Co" => Some(b"256".to_vec()),
            b"RGB" => Some(b"8".to_vec()),
            b"kbs" => Some(vec![0x7f]),
            _ => terminfo::value(&name),
        };
        let Some(value) = value else {
            term.replies.extend_from_slice(b"\x1bP0+r\x1b\\");
            return;
        };
        if !answer.is_empty() {
            // xterm separates completed name/value pairs with semicolons.
            answer.push(';');
        }
        answer.push_str(&encode_hex(&name));
        answer.push('=');
        answer.push_str(&encode_hex(&value));
    }
    term.replies
        .extend_from_slice(format!("\x1bP1+r{answer}\x1b\\").as_bytes());
}

/// Decodes ASCII hex pairs without trusting UTF-8 or indexing odd lengths.
fn decode_hex(input: &[u8]) -> Option<Vec<u8>> {
    // Odd byte counts and long names are rejected before pair indexing.
    if input.is_empty() || !input.len().is_multiple_of(2) || input.len() > 64 {
        return None;
    }
    input
        .chunks_exact(2)
        .map(|pair| Some((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?))
        .collect()
}

/// Accepts upper or lower hexadecimal digits in a DCS capability name.
fn hex_digit(byte: u8) -> Option<u8> {
    // Requests are case-insensitive in the hexadecimal alphabet.
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Encodes a capability name or value as uppercase ASCII hex pairs.
fn encode_hex(bytes: &[u8]) -> String {
    // Two output characters represent each input byte in the wire format.
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[(byte >> 4) as usize]));
        result.push(char::from(DIGITS[(byte & 15) as usize]));
    }
    result
}
