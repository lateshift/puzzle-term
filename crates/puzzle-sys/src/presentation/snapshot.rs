//! Copy the image actually assigned by main, without a GPU readback or frame request.
//!
//! This capability has its own tiny ownership cell. Only main's contents commit
//! and close change it; no render, Session or GPU-completion code takes its guard.
//! Copy clones the front slot before reading pixels, excluding pool reuse through
//! the same Arc-count rule as compositor ownership. The guard covers no pixel work.
//! The texture uses Metal Shared storage and its queue fence has completed before
//! adoption. IOSurface's read-only access bracket makes those bytes CPU coherent;
//! it is not a mutex or a wait on the render/Session owner (Apple SDK IOSurfaceRef.h).
mod pixels;
use super::{Geometry, pool::Slot};
use objc2_io_surface::{IOSurfaceLockOptions, IOSurfaceRef};
use std::sync::{Arc, Mutex};

/// Copy failures preserve the caller's buffer; insufficient storage reports its shape.
#[derive(Debug)]
pub enum SnapshotError {
    /// No image has been presented, or the native host has closed.
    Unsupported,
    /// Width zero cannot describe a thumbnail.
    InvalidWidth,
    /// Tight RGBA8 rows require width × height × four bytes.
    BufferTooSmall([u32; 2]),
    /// An ownership guard or native CPU mapping was unavailable.
    Busy,
    /// An internal native storage descriptor did not match BGRA8 shared storage.
    Storage(String),
}
/// Immutable adopted frame includes its crop, which may be smaller than allocation.
struct Front {
    /// Retention prevents the render pool acquiring this slot while CPU reads.
    slot: Arc<Slot>,
    /// Actual rendered pixels, never current requested bounds or allocation buckets.
    geometry: Geometry,
}
/// Main-affine publication with Send-safe ownership for checked Surface capabilities.
#[derive(Clone, Default)]
pub struct Snapshot(Arc<Mutex<Option<Front>>>);
impl Snapshot {
    /// Only the main-thread native commit calls this after assigning layer contents.
    pub(super) fn presented(&self, slot: Arc<Slot>, geometry: Geometry) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(Front { slot, geometry });
    }
    /// A retained Surface clone must not retain a closed pane's image indefinitely.
    pub(super) fn clear(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    /// Downscale encoded premultiplied BGRA directly into caller-owned tight RGBA.
    /// No allocation, frame request, worker receive or new GPU command occurs.
    pub fn copy(&self, max_width: u32, out: &mut [u8]) -> Result<[u32; 2], SnapshotError> {
        // This lock has no worker writer; try_lock also makes nonwaiting explicit.
        let (slot, geometry) = {
            let front = self.0.try_lock().map_err(|_| SnapshotError::Busy)?;
            let front = front.as_ref().ok_or(SnapshotError::Unsupported)?;
            (Arc::clone(&front.slot), front.geometry)
        };
        let size = pixels::size(geometry.size, max_width)?;
        if out.len() < size[0] as usize * size[1] as usize * 4 {
            return Err(SnapshotError::BufferTooSmall(size));
        }
        // Completed pixels are immutable. The native read-only bracket prepares CPU
        // cache access; it does not wait for the completed producer or issue a render.
        // Reads can coexist with CA's reads; the retained slot excludes GPU reuse.
        let surface = &slot.image.surface;
        let _mapped = Mapping::new(surface)?;
        copy(surface, geometry.size, size, out)?;
        Ok(size)
    }
}
/// Read-only mapping balances IOSurface's API even if storage validation rejects it.
struct Mapping<'a>(&'a IOSurfaceRef);
impl<'a> Mapping<'a> {
    /// Metal's completion fence already ran; the seed is not needed for immutable pixels.
    #[allow(
        unsafe_code,
        reason = "R8/D-60: CPU access to completed shared IOSurface pixels"
    )]
    fn new(surface: &'a IOSurfaceRef) -> Result<Self, SnapshotError> {
        // SAFETY: the retained surface is live; constant read-only options affect
        // only this mapping. The optional seed pointer is explicitly null.
        let result = unsafe { surface.lock(options(), std::ptr::null_mut()) };
        if result != 0 {
            tracing::debug!(event = "snapshot.map_unavailable", result);
            return Err(SnapshotError::Busy);
        }
        Ok(Self(surface))
    }
}
impl Drop for Mapping<'_> {
    /// Unlock never writes image contents; a failed unlock remains an OS diagnostic.
    #[allow(
        unsafe_code,
        reason = "R8/D-60: balance the read-only IOSurface mapping"
    )]
    fn drop(&mut self) {
        // SAFETY: this live surface was successfully locked with identical options;
        // no seed storage is requested, and the map never mutates its pixels.
        let result = unsafe { self.0.unlock(options(), std::ptr::null_mut()) };
        if result != 0 {
            tracing::error!(event = "snapshot.unlock_failed", result);
        }
    }
}
/// ReadOnly must match at lock/unlock; it does not advance the surface's write seed.
fn options() -> IOSurfaceLockOptions {
    // AvoidSync rejects cache/paging preparation even for a completed shared image
    // on this driver. Ordinary read-only access is required; no GPU work is pending.
    IOSurfaceLockOptions::ReadOnly
}
/// Validate native allocation/stride before exposing CPU-readable immutable memory.
#[allow(unsafe_code, reason = "R8/D-60: bounded shared IOSurface pixel slice")]
fn copy(
    surface: &IOSurfaceRef,
    source: [u32; 2],
    size: [u32; 2],
    out: &mut [u8],
) -> Result<(), SnapshotError> {
    let stride = surface.bytes_per_row();
    // Validate the whole row stride, including native padding after the crop.
    // Sampling stays within source width, but its row offsets still use that stride.
    let length = stride
        .checked_mul(source[1] as usize)
        .filter(|length| *length <= surface.alloc_size() && *length <= isize::MAX as usize)
        .ok_or_else(|| SnapshotError::Storage("invalid IOSurface byte extent".into()))?;
    if source[0] as usize > surface.width()
        || source[1] as usize > surface.height()
        || surface.bytes_per_element() != 4
        || stride < source[0] as usize * 4
    {
        return Err(SnapshotError::Storage(
            "invalid IOSurface BGRA descriptor".into(),
        ));
    }
    // SAFETY: Mapping holds read-only CPU access; the retained slot excludes GPU
    // writes. Shared storage's base covers the validated allocation/stride above.
    // No slice escapes this call or outlives the retained image and native mapping.
    let data =
        unsafe { std::slice::from_raw_parts(surface.base_address().as_ptr().cast::<u8>(), length) };
    pixels::copy(data, stride, source, size, out);
    Ok(())
}
