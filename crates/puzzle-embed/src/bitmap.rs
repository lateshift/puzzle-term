//! The approved PNG codec stays entirely in the desktop platform (D-37).
//!
//! Embedded font bytes are untrusted: decode allocation and image dimensions
//! are bounded before samples reach the neutral fitter. PNG palette and low
//! bit-depth samples expand to bytes; 16-bit samples reduce to 8-bit channels.
//! Gray, gray-alpha and RGB become straight RGBA without assuming all PNGs
//! already contain four channels. Alpha is not premultiplied until filtering.
use puzzle_term::text::bitmap::{DecodedBitmap, PngDecoder};
use std::io::Cursor;

/// Stateless decoder passed into neutral bitmap-font preparation.
pub struct Decoder;

/// Only the native shell links this codec, never the text library or wasm.
impl PngDecoder for Decoder {
    /// Rejects malformed/oversized embedded PNGs with recoverable reports.
    fn decode(&mut self, bytes: &[u8]) -> Result<DecodedBitmap, String> {
        // The stream itself is bounded independently of decompressed image memory.
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("embedded PNG exceeds allowance".into());
        }
        // The codec bounds its working allocations as well as our output buffer.
        let mut decoder = ::png::Decoder::new_with_limits(
            Cursor::new(bytes),
            ::png::Limits {
                bytes: 32 * 1024 * 1024,
            },
        );
        // Font bitmaps do not need compressed metadata; skip its retained allocations.
        decoder.set_ignore_text_chunk(true);
        decoder.set_ignore_iccp_chunk(true);
        // Normalize palette and 16-bit images before interpreting channel strides.
        decoder
            .set_transformations(::png::Transformations::EXPAND | ::png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        // Inspect the header before any image-sized allocation is allowed.
        let info = reader.info();
        if info.width == 0 || info.height == 0 || info.width > 2048 || info.height > 2048 {
            return Err("embedded PNG dimensions exceed allowance".into());
        }
        // PNG computes the transformed output size with checked arithmetic.
        let length = reader
            .output_buffer_size()
            .ok_or("PNG byte count overflow")?;
        if length > 16 * 1024 * 1024 {
            return Err("decoded PNG exceeds allowance".into());
        }
        // This allocation happens only on a cold glyph, never on atlas cache hits.
        let mut buffer = vec![0; length];
        let output = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
        // CRC and stream errors remain fatal to this source, allowing the next fallback.
        reader.finish().map_err(|e| e.to_string())?;
        // Convert channel layouts without changing authored RGB or alpha values.
        let pixels = rgba(&buffer[..output.buffer_size()], output.color_type)?;
        Ok(DecodedBitmap {
            width: output.width,
            height: output.height,
            pixels,
        })
    }
}

/// Expands each supported byte format into the renderer's straight RGBA contract.
fn rgba(bytes: &[u8], kind: ::png::ColorType) -> Result<Vec<u8>, String> {
    // Rgba is already normalized; every other format has a fixed source stride.
    let stride = match kind {
        ::png::ColorType::Rgba => return Ok(bytes.to_vec()),
        ::png::ColorType::Rgb => 3,
        ::png::ColorType::GrayscaleAlpha => 2,
        ::png::ColorType::Grayscale => 1,
        // EXPAND must eliminate palette indices before this point.
        ::png::ColorType::Indexed => return Err("PNG palette was not expanded".into()),
    };
    // Reserve the final channel count once, avoiding growth during expansion.
    let mut pixels = Vec::with_capacity(bytes.len() / stride * 4);
    // Missing alpha means opaque; gray replicates the same value into all colors.
    for pixel in bytes.chunks_exact(stride) {
        let out = match kind {
            ::png::ColorType::Rgb => [pixel[0], pixel[1], pixel[2], 255],
            ::png::ColorType::GrayscaleAlpha => [pixel[0], pixel[0], pixel[0], pixel[1]],
            _ => [pixel[0], pixel[0], pixel[0], 255],
        };
        pixels.extend_from_slice(&out);
    }
    Ok(pixels)
}

#[cfg(test)]
/// Real PNG failures and installed-system-emoji readback stay desktop-only.
mod tests;
