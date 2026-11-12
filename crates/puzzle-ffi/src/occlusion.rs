//! Borrowed host UI geometry enters the same checked per-surface owner lane.
//!
//! An empty list may be null. Nonempty arrays must be live/readable for the call;
//! Surface copies them and validates geometry before any message is enqueued.
//!
//! Count is bounded before borrowing the foreign allocation or allocating a copy.
//! Coordinate validation belongs to the safe API, shared by C and Rust hosts.
use crate::{
    abi::{LogicalRect, ResultCode},
    memory, registry,
};

/// Replace the host coverage list; coordinates match surface/cursorRect exactly.
///
#[doc = "# Safety"]
/// Nonempty rects points to count initialized, readable LogicalRect values.
#[allow(unsafe_code, reason = "D-43: C slice borrows the audited array helper")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_set_occluded_rects(
    id: u64,
    rects: *const LogicalRect,
    count: usize,
) -> ResultCode {
    if count > puzzle_embed::Surface::MAX_OCCLUDED_RECTS {
        return ResultCode::InvalidArgument;
    }
    // SAFETY: caller promises readable values; array checks null/alignment/range.
    let rects = match unsafe { memory::array(rects, count) } {
        Ok(rects) => rects,
        Err(error) => return error,
    };
    // No foreign pointer enters the queued Session message or survives this call.
    let rects: Vec<_> = rects
        .iter()
        .map(|rect| [rect.x, rect.y, rect.width, rect.height])
        .collect();
    registry::with_surface(id, |surface| surface.set_occluded_rects(&rects))
}
