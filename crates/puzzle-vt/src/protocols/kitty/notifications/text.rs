//! OSC 99 base64 allows a stream split anywhere or independent padded chunks.
//! Text capacity is bounded before extension; there is no unbounded concatenation.
//!
//! OSC packet boundaries are not Unicode or base64 boundaries. Retaining a small
//! encoded tail lets a quartet and a UTF-8 scalar cross any number of reads.
//! Independent chunks may end in padding, but no bytes may follow padding inside
//! that same chunk. An unpadded final quartet is completed only at d=1.
//! RFC 4648 requires unused low bits to be zero; accepting arbitrary bits there
//! would silently repair malformed application data.
//! Decoded storage is checked before extension, so neither a never-ending stream
//! nor an incomplete UTF-8 sequence can grow the assembler without bound.
use crate::notification::MAX_TEXT;
/// Independent title/body stream with at most three retained encoded symbols.
#[derive(Default)]
pub(super) struct Text {
    /// Original UTF-8 bytes, checked when the whole notification is complete.
    bytes: Vec<u8>,
    /// An encoded quartet may span OSC packet boundaries.
    tail: Vec<u8>,
}
impl Text {
    /// Appends one packet; padding may end a chunk, rather than the entire notification.
    pub(super) fn feed(&mut self, input: &[u8], encoded: bool) -> bool {
        // Plain bytes cannot complete an outstanding encoded quartet.
        if !encoded {
            // Two/three final symbols are padded locally and use the same canonical decoder.
            if !self.tail.is_empty() {
                return false;
            }
            return self.append(input);
        }
        // Padding ends this packet, but the next packet may be independently encoded.
        let mut padded = false;
        for &byte in input {
            // A second quartet after a padded quartet is malformed within one packet.
            if padded {
                return false;
            }
            // Reject the entire assembly rather than interpreting invalid bytes as text.
            if byte != b'=' && digit(byte).is_none() {
                return false;
            }
            // The alphabet check precedes insertion; tail never exceeds four symbols.
            self.tail.push(byte);
            if self.tail.len() == 4 {
                // Remember padding before group() clears the reusable tail.
                padded = self.tail.contains(&b'=');
                if !self.group() {
                    return false;
                }
            }
        }
        true
    }
    /// Decode a canonical quartet; each independently encoded chunk may carry padding.
    fn group(&mut self) -> bool {
        // group() is called only with a complete four-symbol quartet.
        let g = &self.tail;
        // The first two symbols always carry data, never padding.
        let Some(a) = digit(g[0]) else { return false };
        let Some(b) = digit(g[1]) else { return false };
        // Padding in position three means exactly one decoded byte.
        let c = if g[2] == b'=' {
            0
        } else if let Some(c) = digit(g[2]) {
            c
        } else {
            return false;
        };
        // Padding in position four means at most two decoded bytes.
        let d = if g[3] == b'=' {
            0
        } else if let Some(d) = digit(g[3]) {
            d
        } else {
            return false;
        };
        // Compute output length before checking canonical unused bits.
        let n = if g[2] == b'=' {
            1
        } else if g[3] == b'=' {
            2
        } else {
            3
        };
        // One-byte groups require two pads; two-byte groups have two unused low bits.
        if n == 1 && (g[3] != b'=' || b & 15 != 0) || n == 2 && c & 3 != 0 {
            return false;
        }
        // Four six-bit values form three eight-bit bytes in network bit order.
        let bytes = [(a << 2) | (b >> 4), (b << 4) | (c >> 2), (c << 6) | d];
        // Retain tail capacity for the next packet instead of allocating each quartet.
        self.tail.clear();
        self.append(&bytes[..n])
    }
    /// Four bytes per permitted Unicode scalar bounds storage even before UTF-8 validation.
    fn append(&mut self, input: &[u8]) -> bool {
        // Saturating addition cannot wrap a hostile supplied length below the bound.
        if self.bytes.len().saturating_add(input.len()) > MAX_TEXT * 4 {
            return false;
        }
        // These bytes may contain only the prefix of a UTF-8 scalar until the final packet.
        self.bytes.extend_from_slice(input);
        true
    }
    /// An unpadded final quartet is allowed, but a single sextet can encode no byte.
    pub(super) fn finish(mut self) -> Option<String> {
        // Six bits alone do not encode even one full byte.
        if self.tail.len() == 1 {
            return None;
        }
        if !self.tail.is_empty() {
            // Synthesized pads encode the same length as a valid unpadded final group.
            while self.tail.len() < 4 {
                self.tail.push(b'=');
            }
            if !self.group() {
                return None;
            }
        }
        // Validation is delayed until all chunks arrive; a split UTF-8 scalar is valid.
        String::from_utf8(self.bytes).ok()
    }
}
/// RFC 4648 standard alphabet; controls and whitespace cannot be repaired silently.
fn digit(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
