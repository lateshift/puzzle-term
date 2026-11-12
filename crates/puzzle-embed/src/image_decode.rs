//! Native-only bounded kitty image decoding (M15.2, R4 flate2 approval).
//! No filename or shared-memory path reaches this worker. A complete direct
//! payload owns its bytes, and the decoded RGBA allocation is checked first.
use puzzle_term::images::{
    DecodeRequest, DecodeResult, DecodedImage, Format, ImageError, pixel_bytes,
};
use std::{
    io::{Cursor, Read},
    sync::Arc,
};
/// Bounded App lifetime and readiness notifications.
pub(crate) mod worker;

/// Completes one protocol request without touching Session, rendering or native UI.
pub(crate) fn decode(request: DecodeRequest) -> DecodeResult {
    // Correlation survives every failure branch, so the Session can release its
    // source-order barrier without guessing which native request completed.
    let ticket = request.ticket;
    DecodeResult {
        ticket,
        image: pixels(request),
    }
}

/// Optional zlib expansion precedes raw/PNG validation and obeys an explicit bound.
fn pixels(mut request: DecodeRequest) -> Result<DecodedImage, ImageError> {
    // Raw lengths are implied by geometry. PNG can carry an S length only for
    // its compressed encoded bytes; its RGBA cost is checked from the header.
    let raw_length = match request.format {
        Format::Rgb => Some(pixel_bytes(request.size, 3)?),
        Format::Rgba => Some(pixel_bytes(request.size, 4)?),
        Format::Png => request.uncompressed_size,
    };
    // Check input and intermediate sizes before allocating output. A small zlib
    // stream cannot use its declared dimensions to bypass the configured bound.
    if request.payload.len() > request.limit || raw_length.is_some_and(|n| n > request.limit) {
        return Err(ImageError::NoSpace);
    }
    // Decompression runs on this bounded native worker, never the Session owner.
    // Browser code has no dependency on flate2 and will supply its own completion.
    if request.compressed {
        inflate(&mut request, raw_length.ok_or(ImageError::Invalid)?)?;
    }
    // PNG carries its own dimensions; raw s/v cannot override a validated header.
    if request.format == Format::Png {
        return png_pixels(&request.payload, request.limit);
    }
    let length = pixel_bytes(request.size, 4)?;
    if length > request.limit {
        return Err(ImageError::NoSpace);
    }
    // Raw transmission is packed row-major data with no stride or trailing bytes.
    // Validating it first makes RGB expansion infallible for every source pixel.
    if Some(request.payload.len()) != raw_length {
        return Err(ImageError::Invalid);
    }
    // RGBA moves its allocation; RGB gains opaque alpha once in native decoding.
    // Straight alpha is kept as source data for the color raster pipeline.
    let rgba = if request.format == Format::Rgba {
        request.payload
    } else {
        let mut pixels = Vec::with_capacity(length);
        for rgb in request.payload.chunks_exact(3) {
            pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        pixels
    };
    Ok(DecodedImage {
        size: request.size,
        rgba: Arc::from(rgba),
    })
}

/// Exact-length expansion rejects bombs, truncated input and trailing streams.
fn inflate(request: &mut DecodeRequest, expected: usize) -> Result<(), ImageError> {
    // One excess byte detects bombs; Read::take prevents unbounded expansion.
    // png's own allocation limit also covers decoder scratch, not just our final
    // pixels. Ancillary strings/profiles are not terminal image content and are
    // ignored instead of becoming extra unbounded retained metadata.
    let mut decoder = flate2::read::ZlibDecoder::new(request.payload.as_slice());
    let mut decoded = Vec::new();
    decoder
        .by_ref()
        .take(expected as u64 + 1)
        .read_to_end(&mut decoded)
        .map_err(|_| ImageError::Invalid)?;
    // Both truncation and trailing compressed junk are malformed. Exact
    // consumption also rules out silently accepting concatenated zlib streams.
    if decoded.len() != expected || decoder.total_in() != request.payload.len() as u64 {
        return Err(ImageError::Invalid);
    }
    // Drop compressed storage only after exact expansion has succeeded.
    // The decoded encoded-PNG/raw buffer now owns this request's payload.
    request.payload = decoded;
    Ok(())
}

/// PNG metadata, transformed bytes and final RGBA all use the Session allowance.
fn png_pixels(bytes: &[u8], limit: usize) -> Result<DecodedImage, ImageError> {
    let mut decoder =
        png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: limit });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    // Expand palette/transparency to channels and normalize 16-bit input to
    // RGBA8-compatible bytes. This is conversion, never a host color-profile load.
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| ImageError::Invalid)?;
    // Reject dimensions before allocating a frame buffer. The checked helper
    // handles both multiplication overflow and nonpositive image dimensions.
    let size = [reader.info().width, reader.info().height];
    let length = pixel_bytes(size, 4)?;
    if length > limit {
        return Err(ImageError::NoSpace);
    }
    // The transformed PNG may be RGB, grayscale, gray-alpha or RGBA. Both its
    // temporary buffer and its eventual four-channel representation are bounded.
    let packed = reader.output_buffer_size().ok_or(ImageError::NoSpace)?;
    if packed > limit {
        return Err(ImageError::NoSpace);
    }
    let mut buffer = vec![0; packed];
    let output = reader
        .next_frame(&mut buffer)
        .map_err(|_| ImageError::Invalid)?;
    // Consume the image terminator too: a truncated file is not a valid success
    // just because the first frame's scanlines happened to decode completely.
    reader.finish().map_err(|_| ImageError::Invalid)?;
    // A decoder may reserve for a larger frame than the bytes actually written;
    // only validated output bytes participate in channel conversion below.
    buffer.truncate(output.buffer_size());
    // The decoder has already expanded palettes. The only remaining conversion
    // is fixed-stride channel expansion with exact final pixel-count validation.
    let rgba = match output.color_type {
        png::ColorType::Rgba => buffer,
        kind => expand(&buffer, kind, length)?,
    };
    // This final equality catches any stride mismatch before publishing an Arc
    // that rendering could otherwise index using the advertised dimensions.
    if rgba.len() != length {
        return Err(ImageError::Invalid);
    }
    Ok(DecodedImage {
        size,
        rgba: Arc::from(rgba),
    })
}

/// EXPAND eliminates palette indices; all remaining normalized layouts have fixed strides.
fn expand(bytes: &[u8], kind: png::ColorType, length: usize) -> Result<Vec<u8>, ImageError> {
    // Channel count is determined by the decoded header, never a wire-controlled
    // stride. Unexpected layouts fail rather than indexing an undersized chunk.
    let stride = match kind {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        _ => return Err(ImageError::Invalid),
    };
    let mut pixels = Vec::with_capacity(length);
    // Reuse one local four-byte value and append into the pre-sized destination.
    // No per-pixel allocation or temporary color object reaches the Session.
    for pixel in bytes.chunks_exact(stride) {
        let rgba = match kind {
            png::ColorType::Rgb => [pixel[0], pixel[1], pixel[2], 255],
            png::ColorType::GrayscaleAlpha => [pixel[0], pixel[0], pixel[0], pixel[1]],
            _ => [pixel[0], pixel[0], pixel[0], 255],
        };
        pixels.extend_from_slice(&rgba);
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests;
