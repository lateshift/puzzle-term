//! OSC 72's case-sensitive metadata grammar; all numbers are bounded to 32 bits.
//! Reference: <https://sw.kovidgoyal.net/kitty/dnd-protocol/#metadata-reference>.
//!
//! OSC 72 keys are case-sensitive: x/y are cell coordinates, X/Y are device pixels.
//! Coordinates use i64 storage solely to represent both 32-bit wire number domains.
//! A negative coordinate must fit i32; a positive coordinate must fit u32.
//! The leave sentinel is -1 and must not wrap into a very large grid position.
//! Identity and operation fields accept unsigned decimal digits only.
//! Leading signs, spaces and decimals cannot accidentally become metadata values.
//!
//! The omitted type defaults to registration according to the protocol's table.
//! An explicit type flag lets continuation handling distinguish inheritance from conflict.
//! Only m=0 or m=1 are valid; other values cannot leave an assembly permanently open.
//! Repeated fields use the final value within a packet, matching ordinary OSC parsing.
//! No field text is retained after parsing: errors cannot reflect attacker-supplied escapes.
//! The caller owns first-chunk metadata and ignores later values except continuation.
//! This parser allocates neither a metadata map nor temporary numeric strings.

/// The first chunk's values persist; later chunks contribute only the m flag.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Metadata {
    /// Omitted type means registration, as the protocol's metadata table specifies.
    pub(crate) kind: u8,
    /// Distinguishes omitted t on a continuation from a conflicting new command.
    pub(crate) explicit: bool,
    /// Exactly one means more chunks, zero finishes the transaction.
    pub(crate) more: bool,
    /// Multiplexer ID; zero is omitted from encoded responses.
    pub(crate) client: u32,
    /// Copy, move or rejection; interpretation depends on command type.
    pub(crate) operation: u32,
    /// Case-sensitive x/y/X/Y keys, with signed or unsigned 32-bit wire values.
    pub(crate) coordinates: [i64; 4],
}
impl Metadata {
    /// Malformed/unknown metadata is ignored atomically, without logging user data.
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut result = Self {
            kind: b'a',
            explicit: false,
            more: false,
            client: 0,
            operation: 0,
            coordinates: [0; 4],
        };
        if text.is_empty() {
            return Some(result);
        }
        for field in text.split(':') {
            let (key, value) = field.split_once('=')?;
            match key {
                "t" if value.len() == 1 && b"aAmMrRopPeEkqQ".contains(&value.as_bytes()[0]) => {
                    result.kind = value.as_bytes()[0];
                    result.explicit = true;
                }
                "m" => {
                    result.more = match unsigned(value)? {
                        0 => false,
                        1 => true,
                        _ => return None,
                    }
                }
                "i" => result.client = unsigned(value)?,
                "o" => result.operation = unsigned(value)?,
                "x" | "y" | "X" | "Y" => {
                    let index = match key {
                        "x" => 0,
                        "y" => 1,
                        "X" => 2,
                        _ => 3,
                    };
                    // Use i64 storage to represent both protocol integer domains without wrapping.
                    let n = if value.starts_with('-') {
                        i64::from(value.parse::<i32>().ok()?)
                    } else {
                        i64::from(unsigned(value)?)
                    };
                    result.coordinates[index] = n;
                }
                _ => return None,
            }
        }
        Some(result)
    }
}
/// Reject signs, whitespace and overlong decimals before standard integer parsing.
fn unsigned(text: &str) -> Option<u32> {
    (!text.is_empty() && text.len() <= 10 && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}
