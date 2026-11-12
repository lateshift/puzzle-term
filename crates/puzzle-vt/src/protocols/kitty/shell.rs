//! Bounded command labels carried on FinalTerm C marks by kitty's extension.
//! Reference: <https://sw.kovidgoyal.net/kitty/shell-integration/#notes-for-shell-developers>.
//! URL metadata is data only: it is never evaluated as shell quoting or code.
//!
//! FinalTerm still owns the C boundary and its tracked mark. This helper reads
//! only kitty's optional metadata, so missing or broken labels cannot erase a
//! command's output zone. The ordinary scalar print path never calls it.
//! Storage and work are bounded by the label, not the length of shell history.
//! The extra decoded byte proves truncation without retaining a second full OSC.
//! Host chrome receives plain Unicode, never an ANSI control string. A truncated
//! multibyte scalar is removed at a character boundary before adding the ellipsis.
//! These labels are untrusted display metadata, not authenticated process identity.

/// User-approved M12.tabs G1 maximum, including a truncation ellipsis, in UTF-8 bytes.
pub const MAX_COMMAND_BYTES: usize = 1024;

/// Decode the URL form emitted by Puzzle snippets and modern fish.
/// Missing/malformed metadata must not suppress the ordinary output boundary.
pub(crate) fn command(params: &[&[u8]]) -> Option<String> {
    let value = params
        .iter()
        .skip(2)
        .find_map(|part| part.strip_prefix(b"cmdline_url="))?;
    let mut bytes = Vec::with_capacity(MAX_COMMAND_BYTES + 1);
    let mut input = value.iter().copied();
    // One extra byte distinguishes exact fit from truncation. Never allocate
    // a command-sized copy of the full, independently bounded OSC payload.
    while let Some(byte) = input.next() {
        let byte = if byte == b'%' {
            let high = (input.next()? as char).to_digit(16)?;
            let low = (input.next()? as char).to_digit(16)?;
            (high * 16 + low) as u8
        } else {
            byte
        };
        bytes.push(byte);
        if bytes.len() > MAX_COMMAND_BYTES {
            break;
        }
    }
    let truncated = bytes.len() > MAX_COMMAND_BYTES;
    // Lossy decoding is bounded too; a cut multibyte suffix never survives the
    // ellipsis cut. Replace controls for safe plain-text host chrome/tooltips.
    let mut text: String = String::from_utf8_lossy(&bytes)
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if truncated || text.len() > MAX_COMMAND_BYTES {
        let mut end = MAX_COMMAND_BYTES - '…'.len_utf8();
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    Some(text)
}
