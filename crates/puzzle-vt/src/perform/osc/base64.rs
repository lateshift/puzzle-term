//! Strict bounded base64 decoder for OSC 52 clipboard text (§16).
//!
//! A clipboard write is still a policy request, never an immediate host
//! action. Decoding here validates the wire bytes before a later Session
//! submits that request to its interceptor chain. The vte OSC buffer caps
//! input at 64 KiB, so the decoded vector is at most 48 KiB.
//! Standard base64 encodes three bytes in four ASCII symbols. OSC 52
//! carries text, so a syntactically valid base64 group must still decode
//! as UTF-8 before it becomes a clipboard request. Rejecting the whole
//! payload avoids silently changing what the application asked to copy.

/// Decodes standard padded base64, rejecting malformed groups and UTF-8.
pub(super) fn decode(input: &[u8]) -> Option<String> {
    // Four-symbol alignment is required; accepting partial groups would
    // make truncated OSC 52 input look like a valid shorter clipboard write.
    if input.len() > crate::osc::MAX_BUFFER_BYTES || !input.len().is_multiple_of(4) {
        return None;
    }
    let mut bytes = Vec::with_capacity(input.len() / 4 * 3);
    for (index, group) in input.chunks_exact(4).enumerate() {
        // The two leading symbols are mandatory in every encoded group.
        let a = digit(group[0])?;
        let b = digit(group[1])?;
        let last = (index + 1) * 4 == input.len();
        let pad2 = group[2] == b'=';
        let pad3 = group[3] == b'=';
        // A padded group can only be the final group; interior padding
        // otherwise conceals arbitrary bytes after an apparent ending.
        // Padding is legal only at the end and cannot precede data.
        if (!last && (pad2 || pad3)) || (pad2 && !pad3) {
            return None;
        }
        let c = if pad2 { 0 } else { digit(group[2])? };
        let d = if pad3 { 0 } else { digit(group[3])? };
        // Missing third/fourth sextets suppress output bytes rather than
        // adding zero bytes to the clipboard text.
        bytes.push((a << 2) | (b >> 4));
        if !pad2 {
            bytes.push((b << 4) | (c >> 2));
        }
        if !pad3 {
            bytes.push((c << 6) | d);
        }
    }
    String::from_utf8(bytes).ok()
}

/// Maps one ASCII alphabet byte without accepting shell-control bytes.
fn digit(byte: u8) -> Option<u8> {
    // OSC allows other printable bytes, but the clipboard protocol's
    // alphabet is exactly RFC 4648 standard base64, with no whitespace.
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
#[path = "base64/tests.rs"]
mod tests;
