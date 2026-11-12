//! PNG clipboard conversion uses the approved native-only codec and decoded bound.
//! Expansion is limited before allocating pixels; malicious compressed images cannot
//! bypass the same 64-MiB allowance that bounds an OSC 5522 binary transaction.
//! The terminal protocol bounds encoded clipboard representations independently
//! from image expansion. A small compressed PNG can describe an enormous image,
//! so dimensions are checked before the RGBA destination allocation is created.
//! The codec also receives a memory limit for its own decode buffers.
//! Indexed and low-bit-depth images expand through png's approved transformations;
//! 16-bit samples reduce to eight bits before conversion to the OS RGBA format.
//! Greyscale channels replicate into RGB, preserving an explicit alpha if present.
//! These are clipboard bytes, so the terminal's glyph gamma policy is irrelevant.
//! Encoding accepts only a complete width×height×4 buffer and checks arithmetic.
//! PNG dimensions are u32 even on a host whose image dimensions use usize.
//! The writer is dropped before inspecting its final output, ensuring IEND exists.
//! Oversized encoded output is rejected even if the decoded pixels fit.
//! No codec enters the core or wasm: browser image clipboard support is later host work.
use puzzle_term::clipboard::{MAX_BYTES, Status};
use std::{borrow::Cow, io::Cursor};

/// Converts PNG colour variants to the RGBA byte layout required by arboard.
pub(super) fn decode(bytes: &[u8]) -> Result<arboard::ImageData<'static>, Status> {
    let mut decoder =
        png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: MAX_BYTES });
    // Expand indexed/low-bit greyscale and reduce 16-bit channels before copying.
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| Status::Invalid)?;
    let (width, height) = (reader.info().width as usize, reader.info().height as usize);
    // Checked dimensions reject overflow before allocating either decoded representation.
    let length = width
        .checked_mul(height)
        .and_then(|v| v.checked_mul(4))
        .filter(|v| *v <= MAX_BYTES)
        .ok_or(Status::TooLarge)?;
    // The transformed codec output may have fewer channels than the RGBA destination.
    let size = reader
        .output_buffer_size()
        .filter(|v| *v <= MAX_BYTES)
        .ok_or(Status::TooLarge)?;
    // Both staging and destination are bounded, and exist only during requested I/O.
    let mut decoded = vec![0; size];
    let info = reader
        .next_frame(&mut decoded)
        .map_err(|_| Status::Invalid)?;
    // Iterate complete transformed pixels, never encoded PNG scanline/filter bytes.
    let channels = info.color_type.samples();
    let mut rgba = Vec::with_capacity(length);
    for pixel in decoded[..info.buffer_size()].chunks_exact(channels) {
        // Alpha stays straight, as arboard expects; no terminal colour encoding applies.
        match info.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(pixel),
            png::ColorType::Rgb => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            png::ColorType::Grayscale => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255])
            }
            png::ColorType::GrayscaleAlpha => {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]])
            }
            png::ColorType::Indexed => return Err(Status::Invalid),
        }
    }
    Ok(arboard::ImageData {
        width,
        height,
        bytes: Cow::Owned(rgba),
    })
}
/// Produces an exact RGBA PNG for an explicitly allowed binary clipboard read.
pub(super) fn encode(image: arboard::ImageData<'_>) -> Result<Vec<u8>, Status> {
    let length = image
        .width
        .checked_mul(image.height)
        .and_then(|v| v.checked_mul(4))
        .filter(|v| *v <= MAX_BYTES)
        .ok_or(Status::TooLarge)?;
    // Reject a malformed host image instead of passing a short buffer to the encoder.
    if image.bytes.len() != length {
        return Err(Status::Invalid);
    }
    // The PNG wire format cannot represent dimensions outside its 32-bit fields.
    let width = u32::try_from(image.width).map_err(|_| Status::TooLarge)?;
    let height = u32::try_from(image.height).map_err(|_| Status::TooLarge)?;
    let mut out = Vec::new();
    {
        // Explicit colour/depth prevents a platform-specific interpretation of RGBA.
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|_| Status::Io)?;
        writer
            .write_image_data(&image.bytes)
            .map_err(|_| Status::Io)?;
    }
    // Compression overhead may make the final file larger than its pixel storage.
    if out.len() > MAX_BYTES {
        return Err(Status::TooLarge);
    }
    Ok(out)
}
#[cfg(test)]
/// Round trips protect alpha and non-text bytes without accessing the user's clipboard.
mod tests;
