//! Kitty base64 reference: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
//! Strict Base64 admission for one kitty chunk, with no per-byte allocations.
//! Kitty chunks end at a quartet boundary. Validate the whole chunk before
//! appending so malformed input cannot leave partial transfer bytes behind.
use crate::images::ImageError;

/// Converts an RFC 4648 alphabet byte; padding is handled only in the final quartet.
fn digit(byte: u8) -> Result<u8, ImageError> {
    // Do not accept whitespace or URL-safe aliases: a terminal APC payload
    // uses the standard alphabet, with its total length capped by the filter.
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(ImageError::Invalid),
    }
}

/// Decodes at most 4,096 characters into stack scratch before changing `out`.
pub(crate) fn append(
    bytes: &[u8],
    more: bool,
    limit: usize,
    out: &mut Vec<u8>,
) -> Result<(), ImageError> {
    // All chunks end on a quartet boundary, including an empty legal first
    // chunk. Padding is meaningful only in the last packet of a transfer.
    if bytes.len() > crate::vt::apc::CHUNK_BYTES || !bytes.len().is_multiple_of(4) {
        return Err(ImageError::Invalid);
    }
    // The wire cap derives this scratch size; no attacker-controlled stack growth.
    let mut decoded = [0_u8; crate::vt::apc::CHUNK_BYTES / 4 * 3];
    let used = decode_quartets(bytes, more, &mut decoded)?;
    // Addition must be checked before reserving: a declared image allowance
    // cannot be bypassed through integer wrap or many individually valid chunks.
    if out
        .len()
        .checked_add(used)
        .is_none_or(|bytes| bytes > limit)
    {
        return Err(ImageError::NoSpace);
    }
    // Reserve only after every quartet and the aggregate allowance have passed.
    out.extend_from_slice(&decoded[..used]);
    Ok(())
}

/// Validates each quartet before the caller appends any decoded bytes.
fn decode_quartets(bytes: &[u8], more: bool, decoded: &mut [u8]) -> Result<usize, ImageError> {
    let mut used = 0;
    for (index, quartet) in bytes.chunks_exact(4).enumerate() {
        // The first two symbols are never padding. Every accepted quartet
        // contributes at least one byte to the bounded scratch buffer.
        let a = digit(quartet[0])?;
        let b = digit(quartet[1])?;
        let padding = usize::from(quartet[3] == b'=') + usize::from(quartet[2] == b'=');
        // Padding inside a chunk would hide a suffix, and padding before m=0
        // would make the next chunk impossible to concatenate unambiguously.
        if padding != 0 && (more || (index + 1) * 4 != bytes.len()) {
            return Err(ImageError::Invalid);
        }
        // A padding symbol contributes zero bits but reduces the committed
        // length below; the unused scratch suffix is never appended.
        let c = if quartet[2] == b'=' {
            0
        } else {
            digit(quartet[2])?
        };
        // The final symbol can be padding independently of the third symbol.
        // The canonical-shape check below rejects a hole such as Zg=A.
        let d = if quartet[3] == b'=' {
            0
        } else {
            digit(quartet[3])?
        };
        // Canonical pad bits and the only valid padding shapes prevent silent truncation.
        if (quartet[2] == b'=' && quartet[3] != b'=')
            || (padding == 2 && b & 15 != 0)
            || (padding == 1 && c & 3 != 0)
        {
            return Err(ImageError::Invalid);
        }
        // Reassemble the original three octets from four six-bit symbols.
        // Wrapping shifts intentionally discard the higher bits in each octet.
        decoded[used] = (a << 2) | (b >> 4);
        decoded[used + 1] = (b << 4) | (c >> 2);
        decoded[used + 2] = (c << 6) | d;
        // Only complete source octets count toward the aggregate quota.
        used += 3 - padding;
    }
    Ok(used)
}

#[cfg(test)]
mod tests;
