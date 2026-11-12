//! Kitty graphics protocol state, independent of I/O and GPU resources (M15.2).
//! A complete transmission becomes a bounded decode request. The platform worker
//! returns immutable RGBA pixels; only successful validation commits image data.
//! The Session pauses later application bytes while decoding, preserving replies
//! and the cursor position at the final chunk without blocking input or frames.
//! Reference: https://sw.kovidgoyal.net/kitty/graphics-protocol/.
/// Exact device footprint and atomic visibility-aware admission.
mod admission;
/// Strict one-letter control parsing; all integer conversions are checked.
pub use crate::protocols::kitty::graphics::control;
/// Immutable native decode request and result values cross the owner seam.
mod decode;
/// Shared source-aspect geometry keeps visibility and drawing consistent.
pub(crate) mod geometry;
/// Unicode cells resolve virtual placements without a font lookup.
pub(crate) use crate::protocols::kitty::graphics::placeholders;
/// Grid geometry, anchor lifetime and visibility.
mod placement;
/// Decoded source/placement ownership and memory admission.
pub(crate) mod store;
pub use decode::pixel_bytes;
#[cfg(test)]
pub(crate) mod tests;
pub use decode::{DecodeRequest, DecodeResult, DecodedImage, Format};
pub use store::{Image, Placement, Store};
/// Wire errors have stable protocol codes, never embed application-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ImageError {
    /// Invalid syntax, dimensions or decoded pixel count.
    #[error("EINVAL:invalid graphics request")]
    Invalid,
    /// Unsupported media/features must fail rather than access local resources.
    #[error("ENOTSUP:unsupported graphics feature")]
    Unsupported,
    /// Configured decoded allowance cannot admit this transmission.
    #[error("ENOSPC:image storage limit exceeded")]
    NoSpace,
    /// Put/delete refers to an image that is not retained.
    #[error("ENOENT:image not found")]
    NotFound,
}
