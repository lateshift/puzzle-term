//! Bounded little-endian `.pzfp` encoding and untrusted-byte decoding.
//!
//! The directory precedes raw Slug texel planes. Every length is checked
//! against the remaining input before a Vec is allocated. A bytemuck cast
//! views an aligned little-endian payload directly; an unaligned payload
//! is copied through checked four- or two-byte words. That fallback keeps
//! externally supplied packs safe regardless of their slice offset.
//!
//! Version 1 begins with a 52-byte fixed header. The fields appear in
//! this order: four magic bytes, u16 version, u16 weight, u64 source
//! hash, u16 units per em, a zero u16, five f32 metrics, then three u32
//! counts for glyph directory, curve texels and band texels. Every scalar
//! uses little-endian byte order, including IEEE754 floating bits.
//!
//! Each 36-byte directory record contains scalar, glyph ID, em advance,
//! packed band-header location, band flags and a four-float em bbox. A
//! location of all one bits denotes an intentionally empty glyph such as
//! a space. Otherwise the low 16 bits name a column and the high 16 bits
//! name a row in the 4096-texel band plane. Directory records are sorted
//! by Unicode scalar at generation time for reproducible pack bytes.
//!
//! The directory is followed by curve texels as four f32 values each,
//! then band texels as two u16 values each. Physical row gaps remain in
//! these slices: removing a gap would change the shader's integer texel
//! addresses even though it might make the file a little smaller.
//!
//! The parser first checks that count-derived bytes exactly consume the
//! remaining file. This rejects both truncation and trailing content,
//! and bounds every later `Vec` allocation. It then checks each glyph's
//! header location and finite values before any handle is exposed. A
//! valid source hash is useful for takeover by the full font, but it is
//! not trusted as proof that arbitrary external bytes are well formed.
use super::FontPack;
/// Validates external bytes before upload.
mod decode;
/// Writes the stable on-disk format.
mod encode;

/// Upper bound matching the normal combined Slug memory allowance.
const MAX_BYTES: usize = 48 * 1024 * 1024;
/// Fixed number of bytes per cmap and glyph-directory record.
const GLYPH_BYTES: usize = 36;

/// Cursor over an untrusted byte slice with checked movement.
struct Reader<'a> {
    /// Raw candidate pack supplied by a platform or test.
    bytes: &'a [u8],
    /// First unread byte, never advanced beyond `bytes.len()`.
    offset: usize,
}

/// Bounds-checked little-endian fields shared by header and directory.
impl<'a> Reader<'a> {
    /// Takes an exact byte count without integer wrap or slicing panic.
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        // A hostile count may overflow before it can be compared with
        // the slice length; checked addition closes that bypass.
        let end = self
            .offset
            .checked_add(count)
            .ok_or("pack offset overflow")?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or("pack is truncated")?;
        // Advance only after the slice has been proven to exist, so the
        // cursor remains valid after every returned parse error.
        self.offset = end;
        Ok(bytes)
    }

    /// Reads one little-endian 16-bit field after `take` checked bounds.
    fn u16(&mut self) -> Result<u16, String> {
        // Converting to a fixed array forces exactly two source bytes;
        // a short field never reaches a native-endian integer cast.
        let bytes: [u8; 2] = self.take(2)?.try_into().map_err(|_| "pack u16 is short")?;
        Ok(u16::from_le_bytes(bytes))
    }

    /// Reads one little-endian 32-bit field after `take` checked bounds.
    fn u32(&mut self) -> Result<u32, String> {
        // Counts and texel addresses have fixed-width encoding on wasm
        // and native targets even though usize differs between them.
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| "pack u32 is short")?;
        Ok(u32::from_le_bytes(bytes))
    }

    /// Reads a stable 64-bit source hash, not a native-endian pointer.
    fn u64(&mut self) -> Result<u64, String> {
        // The source identity is a file field, never a process hash seed.
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(|_| "pack u64 is short")?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Reinterprets a validated IEEE754 bit pattern as one metric.
    fn f32(&mut self) -> Result<f32, String> {
        // Finite-value checks happen at the semantic field that uses
        // this float, since a raw IEEE bit pattern may encode NaN.
        Ok(f32::from_bits(self.u32()?))
    }
}

/// Writes one scalar without relying on struct padding or host endianness.
fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
/// Writes one scalar without relying on struct padding or host endianness.
fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
/// Writes one source identity without relying on host endianness.
fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
/// Writes one IEEE754 value by its stable bit representation.
fn push_f32(out: &mut Vec<u8>, value: f32) {
    push_u32(out, value.to_bits());
}

/// Encodes a validated in-memory pack for preloading.
pub(super) fn encode(pack: &FontPack) -> Vec<u8> {
    encode::encode(pack)
}

/// Decodes untrusted bytes with a validation report.
pub(super) fn decode(bytes: &[u8]) -> Result<FontPack, String> {
    decode::decode(bytes)
}
