//! Per-surface native UI exclusions use view-local logical top-left points.
//!
//! Validation/copying is host work; placement and navigation stay on the owner.
//! No grid, padding, font or surface-size update is sent by this operation.
//!
//! Each update is a replacement, so hiding host UI cannot leave old exclusions.
//! The readable arrays do not survive the call; the worker receives owned values.
//! Invalid geometry is rejected before queuing, preserving the previous placement.
use super::Surface;
use crate::ApiError;
use puzzle_term::{
    SessionMessage,
    overlay::{geometry::Rect, occlusion},
};

impl Surface {
    /// Maximum UI rectangles in one list; replaces the list atomically.
    pub const MAX_OCCLUDED_RECTS: usize = occlusion::MAX_RECTS;

    /// Replace native UI coverage as [x,y,width,height] in cursorRect coordinates.
    /// Empty clears; finite coordinates and nonnegative extents are required.
    /// Data is copied before return and takes effect on the next owner/frame turn.
    pub fn set_occluded_rects(&self, rects: &[[f64; 4]]) -> Result<(), ApiError> {
        if rects.len() > Self::MAX_OCCLUDED_RECTS {
            return Err(ApiError::InvalidArgument("too many occluded rectangles"));
        }
        // Reject before narrowing: NaN/overflow must not become a plausible position.
        if rects.iter().flatten().any(|value| !value.is_finite()) {
            return Err(ApiError::InvalidArgument("non-finite occluded rectangle"));
        }
        let rects: Vec<_> = rects
            .iter()
            .map(|rect| Rect {
                origin: [rect[0] as f32, rect[1] as f32],
                size: [rect[2] as f32, rect[3] as f32],
            })
            .collect();
        // Core validation also catches narrowing overflow and negative extents.
        if !occlusion::valid(&rects) {
            return Err(ApiError::InvalidArgument("invalid occluded rectangle"));
        }
        self.send(SessionMessage::OccludedRects(rects))
    }
}
