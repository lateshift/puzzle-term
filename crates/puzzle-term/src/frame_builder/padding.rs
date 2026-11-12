//! Surface insets move assembled geometry, never em-space glyph data.
//!
//! A host supplies physical pixels after applying DPR. The harness keeps
//! sixteen-pixel overhang margins; a product host chooses its settings inset.
//! Scissors are intersected with the attachment during a shrinking resize,
//! because WebGPU rejects a rectangle extending beyond the actual surface.
use super::FrameBuilder;
use puzzle_frame::RectU32;

/// Geometry changes invalidate snapshots so retained rows are copied safely.
impl FrameBuilder {
    /// Horizontal device origin includes reserved UI cells; vertical padding does not.
    pub(super) fn grid_x(&self) -> u32 {
        self.padding
            .saturating_add(u32::from(self.gutter_columns).saturating_mul(self.metrics.width_px))
    }
    /// Intersects only text cells with the acquired surface, excluding the gutter.
    pub(super) fn grid_clip(&self, target: [u32; 2], width: u32, height: u32) -> RectU32 {
        // Only x changes; rows retain the same top/bottom padding and height.
        let mut rect = clip(target, width, height, self.padding);
        rect.x = self.grid_x().min(target[0]);
        rect.width = width.min(target[0].saturating_sub(rect.x));
        rect
    }
    /// Returns the future scrollbar's physical track beside the visible grid.
    pub fn scrollbar_track(&self, columns: usize, rows: usize) -> [f64; 4] {
        let mut track = scrollbar_track(self.metrics, self.padding, columns, rows);
        track[0] += f64::from(self.gutter_columns) * f64::from(self.metrics.width_px);
        track
    }
    /// Changes the device-pixel origin without re-preparing any outline.
    pub fn set_padding(&mut self, pixels: u32) {
        if self.padding != pixels {
            self.padding = pixels.min(4096);
            // A padding change affects every instance translation, including
            // the transient cursor; keep snapshots and clips in one epoch.
            self.style_epoch = self.style_epoch.wrapping_add(1);
        }
    }
}

/// Intersects the padded grid with the currently configured attachment.
pub(super) fn clip(target: [u32; 2], width: u32, height: u32, padding: u32) -> RectU32 {
    let x = padding.min(target[0]);
    let y = padding.min(target[1]);
    RectU32 {
        x,
        y,
        width: width.min(target[0].saturating_sub(x)),
        height: height.min(target[1].saturating_sub(y)),
    }
}

/// Input models and drawing share the same physical grid-edge track calculation.
pub(super) fn scrollbar_track(
    metrics: puzzle_text::metrics::CellMetrics,
    padding: u32,
    columns: usize,
    rows: usize,
) -> [f64; 4] {
    // Metrics and padding are already physical pixels; applying DPR here again
    // would make the pointer model disagree with the rendered grid edge.
    let width = columns as f64 * f64::from(metrics.width_px);
    let height = rows as f64 * f64::from(metrics.height_px);
    // The model occupies the last eight physical pixels inside the grid.
    // M9 may style that width, while input and draw use identical bounds.
    [
        f64::from(padding) + (width - 8.0).max(0.0),
        f64::from(padding),
        8.0_f64.min(width),
        height,
    ]
}
