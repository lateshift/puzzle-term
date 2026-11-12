//! Plain values describe work that must never run on the Session parse thread.
//! Both raw conversion and compressed PNG decoding obey the same output bound.
use super::ImageError;
use std::sync::Arc;

/// Kitty's supported direct transmission pixel encodings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// Three unpremultiplied bytes per pixel, no alpha in the source.
    Rgb,
    /// Four unpremultiplied bytes per pixel.
    Rgba,
    /// PNG dimensions come from the validated image header.
    Png,
}

/// One complete bounded transmission, owned exclusively by the native worker.
#[derive(Debug)]
pub struct DecodeRequest {
    /// A stale result must not complete a cancelled/replaced transfer.
    pub ticket: u64,
    /// Raw RGB, RGBA or encoded PNG bytes after Base64 decoding.
    pub format: Format,
    /// Required positive dimensions for raw pixels; ignored for PNG.
    pub size: [u32; 2],
    /// `o=z` asks the native worker to decompress before image decoding.
    pub compressed: bool,
    /// `S` gives the decompressed PNG byte count when combined with `o=z`.
    pub uncompressed_size: Option<usize>,
    /// The source buffer never exceeds the configured image allowance.
    pub payload: Vec<u8>,
    /// Maximum decoded RGBA bytes admitted by this Session.
    pub limit: usize,
}

/// Immutable pixels may be shared with sealed frame publications without copying.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    /// Physical source dimensions, checked against byte count before publication.
    pub size: [u32; 2],
    /// Straight-alpha RGBA8 source, independent of glyph atlas storage.
    pub rgba: Arc<[u8]>,
}

/// Successful or failed worker work returns to the Session as one ordered value.
#[derive(Debug)]
pub struct DecodeResult {
    /// Matches the outstanding request; later source bytes remain paused until it does.
    pub ticket: u64,
    /// No image or placement is mutated on a decode failure.
    pub image: Result<DecodedImage, ImageError>,
}

/// Checked pixel multiplication is shared by protocol admission and native decoding.
pub fn pixel_bytes(size: [u32; 2], channels: usize) -> Result<usize, ImageError> {
    if size.contains(&0) {
        return Err(ImageError::Invalid);
    }
    // Checked conversion also protects 32-bit hosts from a wide intermediate.
    usize::try_from(size[0])
        .ok()
        .and_then(|w| usize::try_from(size[1]).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or(ImageError::NoSpace)
}
