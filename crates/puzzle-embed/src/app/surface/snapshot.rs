//! Caller-owned thumbnails of the last actual presentation (M12.tabs G3).
//!
//! This is a main-thread pixel copy, not a request to render current Session state.
//! It reads the immutable front image paired with its presented geometry, including
//! while hidden or while a newer resize is still pending. Native presentation owns
//! the source lifetime; Surface clones keep no extra image after native close.
use super::{ApiError, Surface};

/// Tight RGBA8 thumbnail dimensions in physical pixels, independent of host DPR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotSize {
    /// At most the requested maximum, never larger than the presented source.
    pub width: u32,
    /// Aspect-preserving integer height, with a minimum of one pixel.
    pub height: u32,
}
impl SnapshotSize {
    /// Required host allocation: exactly four bytes per pixel, no row padding.
    pub fn bytes(self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}
/// Snapshot failure leaves the caller's pixel buffer untouched.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// Normal Surface thread-affinity and closed-state contract applies.
    #[error(transparent)]
    Api(#[from] ApiError),
    /// No presented image, or this platform has no CPU-readable presentation backend.
    #[error("no readable presented image")]
    Unsupported,
    /// Caller may use these dimensions to allocate and call again.
    #[error("snapshot buffer too small for {0:?}")]
    BufferTooSmall(SnapshotSize),
    /// Native CPU access was unavailable; the caller can keep its old preview.
    #[error("snapshot storage temporarily busy")]
    Busy,
    /// Native storage rejected a descriptor; no worker or rendering was started.
    #[error("snapshot storage: {0}")]
    Platform(String),
}
impl Surface {
    /// Copy the last presented image as top-left premultiplied encoded sRGB RGBA8.
    ///
    /// `max_width_px` must be positive. Images are downscaled only; tight output
    /// rows preserve aspect ratio (height rounded down, minimum one). Pass an empty
    /// slice to learn the required size through `BufferTooSmall`. No Session query,
    /// render request, GPU readback or wait is performed. Call on the creating thread.
    pub fn snapshot(
        &self,
        max_width_px: u32,
        buffer: &mut [u8],
    ) -> Result<SnapshotSize, SnapshotError> {
        // Affinity/lifetime errors take precedence over native availability, as
        // they do for other Surface operations. No cached pixels escape after close.
        self.check()?;
        if max_width_px == 0 {
            return Err(
                ApiError::InvalidArgument("snapshot maximum width must be positive").into(),
            );
        }
        #[cfg(target_os = "macos")]
        {
            use puzzle_sys::presentation::SnapshotError as Native;
            // Cold attachment installs this capability once. Diagnostic/offscreen
            // surfaces without a native presentation owner have no front to copy.
            let front = self.snapshot.get().ok_or(SnapshotError::Unsupported)?;
            let dimensions = |[width, height]: [u32; 2]| SnapshotSize { width, height };
            front
                .copy(max_width_px, buffer)
                .map(dimensions)
                .map_err(|error| match error {
                    Native::Unsupported => SnapshotError::Unsupported,
                    Native::InvalidWidth => {
                        ApiError::InvalidArgument("invalid snapshot width").into()
                    }
                    Native::BufferTooSmall(size) => SnapshotError::BufferTooSmall(dimensions(size)),
                    Native::Busy => SnapshotError::Busy,
                    Native::Storage(error) => SnapshotError::Platform(error),
                })
        }
        #[cfg(not(target_os = "macos"))]
        {
            // M13 supplies readable storage for those backends; the ABI and bytes
            // are portable now without pretending a fresh render is a snapshot.
            let _ = buffer;
            Err(SnapshotError::Unsupported)
        }
    }
}
