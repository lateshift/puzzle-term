//! Strict RFC 4648 streaming decode; packet boundaries are not base64 boundaries.
//! Reference: <https://sw.kovidgoyal.net/kitty/clipboard/> (Encoding of payloads).
//! Strict decoding matters because clipboard contents are not display text.
//! Silently deleting whitespace or controls would transform corrupted input into
//! apparently valid data and could change the bytes a program asked to copy.
//! A four-symbol group produces one, two or three bytes. The unused low bits
//! must be zero when padding shortens a group; this rejects noncanonical forms.
//! Padding closes the whole MIME stream, not the packet that happened to carry it.
//! The next packet may continue an incomplete group, but never a padded one.
//! The tail contains at most three bytes between calls, regardless of input size.
//! The aggregate counter includes bytes from representations later overwritten.
//! That keeps hostile retransmission from bypassing the transaction allowance.
//! Metadata uses the same decoder with a smaller independent textual limit.
//! UTF-8 is required for names and passwords, never for binary representations.
//! Outgoing replies encode each permitted decoded chunk separately, with its own
//! padding, as required for clients reading DATA packets independently.
//! The decoder owns no transport and cannot write a premature acknowledgement.
//! Its caller decides whether an error aborts a write or silently rejects a read.
use crate::clipboard::{MAX_BYTES, Status, Write};
/// Appends complete groups while retaining an incomplete group for the next packet.
pub(super) fn feed(write: &mut Write, input: &[u8]) -> Result<(), Status> {
    for &byte in input {
        // A padded group ends this MIME stream; later bytes would corrupt it.
        if write.padded || !(byte == b'=' || digit(byte).is_some()) {
            return Err(Status::Invalid);
        }
        // Packet boundaries may divide this four-symbol group at any position.
        write.tail.push(byte);
        if write.tail.len() == 4 {
            let group = &write.tail;
            let a = digit(group[0]).ok_or(Status::Invalid)?;
            // Zero-fill absent trailing bytes solely for assembling the encoded sextets.
            let b = digit(group[1]).ok_or(Status::Invalid)?;
            // Padded sextets contribute zero bits but must occur only at the end.
            let c = if group[2] == b'=' {
                0
            } else {
                digit(group[2]).ok_or(Status::Invalid)?
            };
            let d = if group[3] == b'=' {
                0
            } else {
                digit(group[3]).ok_or(Status::Invalid)?
            };
            // The final two symbols determine exactly how many bytes are meaningful.
            let count = if group[2] == b'=' {
                1
            } else if group[3] == b'=' {
                2
            } else {
                3
            };
            // Canonical padding includes zero unused bits; no hidden suffix is accepted.
            if count == 1 && (group[3] != b'=' || b & 15 != 0) || count == 2 && c & 3 != 0 {
                return Err(Status::Invalid);
            }
            // Check before extending storage, including at the exact 64-MiB boundary.
            if write.total.saturating_add(count) > MAX_BYTES {
                return Err(Status::TooLarge);
            }
            // Join sextet bits without interpreting binary image bytes as UTF-8.
            write.bytes.extend_from_slice(
                &[(a << 2) | (b >> 4), (b << 4) | (c >> 2), (c << 6) | d][..count],
            );
            write.total += count;
            // Later input to this MIME is forbidden once padding terminates it.
            write.padded = count != 3;
            write.tail.clear();
        }
    }
    Ok(())
}
/// Decodes metadata and read payloads as a complete, bounded stream.
pub(super) fn decode(input: &[u8], limit: usize) -> Option<String> {
    // Reject oversized metadata before constructing even its bounded temporary state.
    if input.len() > limit.div_ceil(3) * 4 {
        return None;
    }
    let mut state = Write::default();
    feed(&mut state, input).ok()?;
    if !state.tail.is_empty() || state.bytes.len() > limit {
        return None;
    }
    // Textual metadata may not use replacement characters for invalid UTF-8.
    String::from_utf8(state.bytes).ok()
}
/// Standard alphabet only: whitespace is corruption, never ignored.
fn digit(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
/// Appends independent padded chunks for outgoing metadata and binary data.
pub(crate) fn encode(bytes: &[u8], out: &mut Vec<u8>) {
    /// RFC 4648 alphabet, shared by metadata and binary reply encoding.
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // A final short group is padded rather than merged with the next DATA packet.
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.extend_from_slice(&[
            ALPHABET[(a >> 2) as usize],
            ALPHABET[((a & 3) << 4 | b >> 4) as usize],
            if chunk.len() > 1 {
                ALPHABET[((b & 15) << 2 | c >> 6) as usize]
            } else {
                b'='
            },
            if chunk.len() > 2 {
                ALPHABET[(c & 63) as usize]
            } else {
                b'='
            },
        ]);
    }
}
