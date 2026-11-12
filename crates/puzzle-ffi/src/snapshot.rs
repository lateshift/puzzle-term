//! Portable thumbnail marshalling; image ownership and sampling live in puzzle-embed.
//!
//! No pointer crosses the Surface API. Output dimensions remain zero on failure,
//! except Exhausted, which returns the required shape without touching pixel bytes.
//! The registry is released before native pixel access; no host callback runs here.
use crate::{abi::ResultCode, memory, registry};
use puzzle_embed::{SnapshotError, SnapshotSize};

/// Copy tight premultiplied sRGB RGBA8 of the last actually presented image.
///
#[doc = "# Safety"]
/// Width/height are writable u32 pointers. A nonempty buffer names capacity writable
/// bytes; these three destinations do not overlap and remain exclusive for this call.
#[allow(
    unsafe_code,
    reason = "D-43: checked caller-owned snapshot buffer and dimensions"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_snapshot(
    id: u64,
    max_width_px: u32,
    buffer: *mut u8,
    capacity: usize,
    width: *mut u32,
    height: *mut u32,
) -> ResultCode {
    registry::result((|| {
        // Validate all output shapes before touching either dimensions or pixels.
        memory::pointer(width)?;
        memory::pointer(height)?;
        // SAFETY: validated pointers with caller-owned nonoverlapping writable u32s.
        unsafe { memory::write(width, 0)? };
        // SAFETY: same output contract; height is distinct from width and buffer.
        unsafe { memory::write(height, 0)? };
        // SAFETY: caller's exclusive output allocation is borrowed only for this call.
        let buffer = unsafe { memory::bytes_mut(buffer, capacity)? };
        // Clone the checked capability and release the registry at this semicolon.
        // Native pixel access never holds the global handle table or runs callbacks.
        let surface = registry::lock()?.surface(id)?.surface.clone();
        // A failed capacity probe returns shape but never writes any pixel byte.
        // Other errors carry no dimensions, avoiding stale geometry after failure.
        let (size, result) = match surface.snapshot(max_width_px, buffer) {
            Ok(size) => (Some(size), Ok(())),
            Err(SnapshotError::BufferTooSmall(size)) => (Some(size), Err(ResultCode::Exhausted)),
            Err(error) => (None, Err(code(error))),
        };
        // Report dimensions only after the copy/capacity check has completed.
        if let Some(SnapshotSize {
            width: w,
            height: h,
        }) = size
        {
            // SAFETY: validated exclusive destinations outlive the Surface copy.
            unsafe { memory::write(width, w)? };
            // SAFETY: height is the second independent output promised by the caller.
            unsafe { memory::write(height, h)? };
        }
        result
    })())
}
/// Preserve normal lifecycle/affinity errors; capacity is a distinct recoverable result.
fn code(error: SnapshotError) -> ResultCode {
    match error {
        SnapshotError::Api(error) => error.into(),
        SnapshotError::Unsupported => ResultCode::Unsupported,
        SnapshotError::Busy => ResultCode::Busy,
        SnapshotError::Platform(error) => {
            tracing::error!(event = "ffi.snapshot_failed", %error);
            ResultCode::Platform
        }
        SnapshotError::BufferTooSmall(_) => ResultCode::Exhausted,
    }
}
