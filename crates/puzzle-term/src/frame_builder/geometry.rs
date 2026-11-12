//! Shared cell metrics for frame geometry and initial PTY dimensions.
//!
//! The host must compute its first grid before it opens the shell, while
//! the frame builder must place glyphs on that same integer device grid.
//! Keeping both callers on this function prevents the startup 98-column
//! mismatch once seen at DPR two. Font fallback affects ink bounds but
//! never the primary face's cell advance; otherwise later cells drift.
//!
//! D-27 supplies light hinted outlines below DPR 1.5 and unhinted ones
//! above it. Cap-height snapping is permitted only on the unhinted path,
//! so changing monitors can invalidate both outline keys and metrics.
use super::FrameBuilder;
use puzzle_text::{
    font_set::{FontSet, FontStyle},
    metrics::{CellMetrics, CellOptions, MetricAdjustment},
};

impl FrameBuilder {
    /// Measures device-pixel cells before a platform starts its PTY child.
    ///
    /// A desktop shell must create the PTY at the same grid size that the
    /// eventual frame builder will use; otherwise the first prompt sees
    /// stale 80×24 dimensions until a later resize or SIGWINCH.
    pub fn metrics_for(fonts: &FontSet, font_size: f32, dpr: f32) -> Result<CellMetrics, String> {
        measure(fonts, font_size, dpr)
    }
}

/// Derives primary-face metrics before accepting a grid dimension.
pub(super) fn measure(fonts: &FontSet, font_size: f32, dpr: f32) -> Result<CellMetrics, String> {
    // The primary regular face owns grid advances even if a later glyph
    // falls back to a wider icon face (§8.3). Fallback changes ink only.
    let face = fonts
        .face(fonts.primary_face(FontStyle::Regular))
        .ok_or("primary face missing")?;
    CellMetrics::from_face(
        face,
        CellOptions {
            // D-27 feeds light-hinted outlines at low DPR into Slug;
            // cap snapping belongs only to the unhinted higher-DPR path.
            size: font_size,
            dpr,
            line_height: 1.0,
            width: MetricAdjustment::Px(0.0),
            height: MetricAdjustment::Px(0.0),
            baseline: MetricAdjustment::Px(0.0),
            snap_cap_height: dpr >= 1.5,
            hinted: dpr < 1.5,
        },
    )
}
