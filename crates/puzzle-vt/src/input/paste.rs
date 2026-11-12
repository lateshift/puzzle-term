//! Focus reports and sanitised bracketed paste (§6.8, §16).
//!
//! Paste is untrusted: an embedded closing delimiter could end the
//! bracket early and make following bytes execute as typed shell input.
//! Strip that exact marker before adding our own. Other C0 commands are
//! removed; tabs and normalized line breaks remain useful text.
//!
//! This conversion runs before transport output, so no VT parser or grid
//! state can be altered by pasted controls on the terminal's input side.
//! A multiline confirmation policy belongs to the later interceptor chain
//! (§6.8); the byte sanitation rule applies whether or not one is present.
use super::InputModes;

/// Emits focus state only after the application enabled DECSET 1004.
///
/// Focus bytes use CSI I/O and never include a cursor position.
#[must_use]
pub fn encode_focus(focused: bool, modes: InputModes) -> Vec<u8> {
    if !modes.focus_reports {
        return Vec::new();
    }
    if focused {
        b"\x1b[I".to_vec()
    } else {
        b"\x1b[O".to_vec()
    }
}

/// Normalises and bounds one paste before the transport writes it.
///
/// The output is at most the input length plus two bracket markers.
#[must_use]
pub fn encode_paste(text: &str, modes: InputModes) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + if modes.bracketed_paste { 12 } else { 0 });
    if modes.bracketed_paste {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    // Inspect UTF-8 bytes without changing non-control multibyte scalars;
    // none of those continuation bytes can be an ASCII escape introducer.
    let input = text.as_bytes();
    let mut index = 0;
    while index < input.len() {
        if input[index..].starts_with(b"\x1b[201~") {
            // Consuming the whole marker prevents a pasted close bracket
            // from ending the application's protected paste region.
            index += 6;
            continue;
        }
        match input[index] {
            // CRLF is one Enter-like CR; bare LF also becomes CR, while a
            // pre-existing CR remains a single CR.
            b'\r' => {
                bytes.push(b'\r');
                if input.get(index + 1) == Some(&b'\n') {
                    index += 1;
                }
            }
            b'\n' => bytes.push(b'\r'),
            b'\t' => bytes.push(b'\t'),
            // ESC, BEL, DEL and other controls cannot inject terminal or
            // shell actions through the paste channel.
            0..=31 | 127 => {}
            byte => bytes.push(byte),
        }
        index += 1;
    }
    if modes.bracketed_paste {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}
