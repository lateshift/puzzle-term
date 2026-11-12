//! Inclusive terminal scroll-region boundaries (§6.5).
//!
//! Margins name cells, not edges: a full grid ends at size minus one.
//! Construction handles an empty host size without subtracting below zero.
/// Inclusive scrolling and horizontal boundaries in zero-based cells.
#[derive(Clone, Copy, Debug)]
pub struct Margins {
    /// First row that scrolls under IND, IL or SU.
    pub top: usize,
    /// Last row that scrolls under IND, IL or SU.
    pub bottom: usize,
    /// First column for CR and horizontal editing when DECLRMM is on.
    pub left: usize,
    /// Last column for horizontal editing when DECLRMM is on.
    pub right: usize,
}

/// Full-grid boundaries are shared by startup and reset.
impl Margins {
    /// Returns the inclusive whole-grid scroll region in zero-based cells.
    pub(super) fn full(columns: usize, rows: usize) -> Self {
        // Saturation keeps this geometry helper safe even before host clamping.
        Self {
            top: 0,
            bottom: rows.saturating_sub(1),
            left: 0,
            right: columns.saturating_sub(1),
        }
    }
}
