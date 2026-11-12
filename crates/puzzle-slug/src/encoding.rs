//! Single CPU encoding contract for Slug instance words (design §9.5).
//!
//! The vertex shader unpacks two u16 texture coordinates and two u8 band
//! maxima. The low maximum belongs to x (vertical-ray bands); the high
//! maximum belongs to y (horizontal-ray bands). Band headers remain y first.
//! Naming those axes here prevents pack recovery from swapping the two lists.
//! The final byte carries reference flags; the intervening byte stays reserved.

/// Even-odd fill, bit 28 in band_info and bit 12 after WGSL's high-half shift.
pub const EVEN_ODD: u8 = 0x10;
/// Optical weighting, bit 29 in band_info and bit 13 in the shader's glyph.w.
pub const WEIGHT: u8 = 0x20;

/// Encodes x/y maxima (counts minus one) and the complete reference flag byte.
#[must_use]
pub fn encode_band_info(x: u8, y: u8, flags: u8) -> u32 {
    // Explicit bytes preserve WGSL's bit layout on either host endianness.
    u32::from_le_bytes([x, 0, y, flags])
}

/// Returns x/y maxima and flags; the reserved byte has no shader meaning.
#[must_use]
pub fn decode_band_info(word: u32) -> (u8, u8, u8) {
    // Reading each axis independently avoids confusing header order with bit order.
    let [x, _, y, flags] = word.to_le_bytes();
    (x, y, flags)
}

/// Packs the band-texture coordinate as x in the low half and y in the high half.
#[must_use]
pub fn encode_glyph_loc(location: [u16; 2]) -> u32 {
    // Texture width validation belongs to the allocator or untrusted pack reader.
    let [x0, x1] = location[0].to_le_bytes();
    let [y0, y1] = location[1].to_le_bytes();
    u32::from_le_bytes([x0, x1, y0, y1])
}

/// Unpacks exactly the two coordinates consumed by WGSL's SlugUnpack.
#[must_use]
pub fn decode_glyph_loc(word: u32) -> [u16; 2] {
    // All u16 coordinates round-trip; range checks precede CPU texture access.
    let [x0, x1, y0, y1] = word.to_le_bytes();
    [u16::from_le_bytes([x0, x1]), u16::from_le_bytes([y0, y1])]
}

#[cfg(test)]
/// Round trips and the independent shader bit contract must agree for every field.
mod tests;
