//! Kitty layout derives cell-sized destinations from current device metrics.
//!
//! Transmission-time cell extents are needed for cursor movement, but become
//! stale after font changes. Visibility and drawing use this same calculation
//! from the original c/r request and the immutable source crop (M15.3).
//! Source pixels are physical pixels; DPR is already included in cell metrics.

/// Natural images retain pixels; one cell axis derives scale; two axes aspect-fit.
pub(crate) fn display_size(source: [u32; 2], cells: [u32; 2], cell: [u32; 2]) -> [f64; 2] {
    let [width, height] = source.map(f64::from);
    let [c, r] = cells;
    // Callers use validated nonempty source crops and at least one pixel per cell.
    let [cw, ch] = cell.map(|value| f64::from(value.max(1)));
    let scale = match (c, r) {
        // No c/r means one decoded pixel per device pixel, regardless of DPR.
        (0, 0) => 1.0,
        (c, 0) => f64::from(c) * cw / width,
        (0, r) => f64::from(r) * ch / height,
        // The smaller constraint keeps the entire source inside the requested cells.
        (c, r) => (f64::from(c) * cw / width).min(f64::from(r) * ch / height),
    };
    [width * scale, height * scale]
}
