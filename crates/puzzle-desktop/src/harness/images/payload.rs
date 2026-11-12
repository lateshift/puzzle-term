//! Authored RGBA/PNG quadrants enter through actual kitty APC bytes and the decoder.
//!
//! Sixteen-by-sixteen sources keep candidate crops small while exposing every
//! quadrant at terminal scale. RGB is straight-alpha, matching PNG's semantics;
//! the shader must premultiply before filtering, not after blending source taps.
//! The encoder only authors test bytes. Production decoding still validates the
//! declared dimensions, decompressed length and Session allowance on its worker.
use super::*;
/// Four saturated quadrants make source crops and alpha independently observable.
fn pixels(alpha: bool) -> Vec<u8> {
    // Store channels in row-major order, exactly as the raw f=32 medium requires.
    let mut pixels = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            // An asymmetric crop selects the right quadrants without consulting UV code.
            let color = match (x < 8, y < 8) {
                (true, true) => [255, 0, 0],
                (false, true) => [0, 255, 0],
                (true, false) => [0, 0, 255],
                (false, false) => [255, 255, 0],
            };
            pixels.extend_from_slice(&color);
            // Half opacity leaves underlying text/paper measurable in the PNG fixture.
            pixels.push(if alpha { 128 } else { 255 });
        }
    }
    pixels
}
/// PNG uses the already-approved native codec; raw bytes share the same pixel source.
pub(super) fn transmit(
    session: &mut Session,
    control: &str,
    png: bool,
    now: Instant,
) -> Result<(), String> {
    // Raw and PNG transport differ, while their authored source colors agree.
    let rgba = pixels(png);
    let bytes = if png {
        encode_png(&rgba, [16, 16])?
    } else {
        rgba
    };
    // Kitty names encoded PNG as f=100, rather than the source channel count.
    let format = if png { 100 } else { 32 };
    let payload = base64(&bytes);
    session.feed(
        format!("\x1b_Ga=T,f={format},s=16,v=16,C=1,{control};{payload}\x1b\\").as_bytes(),
        now,
    );
    // Missing requests catch broken APC extraction instead of bypassing it in the fixture.
    let request = session
        .take_image_decode_request()
        .ok_or("image decode request absent")?;
    // Complete through the platform codec, not by injecting fabricated decoded pixels.
    let result = puzzle_embed::diagnostics::decode_image(request);
    // A codec error preserves its actual protocol error in the harness diagnostic.
    if let Err(error) = &result.image {
        return Err(format!("image decode: {error:?}"));
    }
    // The completion consumes the same ticket and ordered reply barrier as native sessions.
    session.apply(SessionMessage::ImageDecoded(result), now)
}
/// The PNG fixture has authored alpha and no external asset or filesystem source.
/// Source dimensions are independent of cell geometry: odd sizes exercise partial
/// placeholder tiles, while the original quadrant fixture keeps its 16×16 source.
pub(super) fn encode_png(rgba: &[u8], size: [u32; 2]) -> Result<Vec<u8>, String> {
    // Finish the writer before moving its output, ensuring the final PNG chunk exists.
    let mut bytes = Vec::new();
    {
        let mut encoder = ::png::Encoder::new(&mut bytes, size[0], size[1]);
        // Explicit channel/depth metadata exercises RGBA PNG alpha transformation.
        encoder.set_color(::png::ColorType::Rgba);
        encoder.set_depth(::png::BitDepth::Eight);
        // Header and image data pass through the approved png encoder, not a fake format.
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}
/// Small deterministic fixture encoder, with standard RFC 4648 padding.
pub(super) fn base64(bytes: &[u8]) -> String {
    // Standard alphabet has no whitespace or URL-safe substitutions in graphics payloads.
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // Three input bytes become four alphabet symbols; the final chunk is padded.
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    // Chunking is an encoder detail; the protocol sees one bounded Base64 payload here.
    for chunk in bytes.chunks(3) {
        // Pack absent low bytes as zero, then replace their symbols with equals signs.
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            // Symbol zero/one always exist; the last two depend on input byte count.
            out.push(if index > chunk.len() {
                '='
            } else {
                alphabet[((value >> shift) & 63) as usize] as char
            });
        }
    }
    out
}
