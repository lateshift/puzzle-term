//! Physical images follow the same displayed source rows as text and placeholder tiles.
//! A run of consecutive source rows uses one translated full-image quad and a
//! row scissor. Omitted rows contribute no pixels and never stretch the remaining
//! picture. No code in this module runs until a user row omission is active.
//!
//! Clipping and source sampling are deliberately separate. Cropping the source
//! rectangle for each retained row would restart bilinear interpolation, making
//! seams or changing the picture's scale. Instead a run retains the full image
//! source/destination transform and scissors only its displayed row band.
//! A fold can split one placement into several runs, bounded by visible rows.
//! This does not allocate a texture per run: every command references the same
//! immutable image and GPU residency. Source text and image anchors remain in
//! VT order, so unfolding restores the original placement without reconstruction.
use super::{FrameSnapshot, seal};

impl FrameSnapshot {
    /// Inspect only the bounded visible row map, not retained image/text contents.
    pub(super) fn capture_projected_image(
        &mut self,
        placement: &crate::images::Placement,
        image: &crate::images::Image,
        extent: [usize; 3],
        source_row: &impl Fn(usize) -> Option<usize>,
    ) {
        let [column, source_top, height] = extent;
        // The loop index counts visible slots, never hidden history rows.
        let mut row = 0;
        while row < self.rows() {
            let Some(source) = source_row(row) else {
                row += 1;
                continue;
            };
            // Each iteration advances the visible index, even for offscreen ink.
            if source < source_top || source >= source_top.saturating_add(height) {
                row += 1;
                continue;
            }
            // This first matching row fixes the translation for the complete run.
            let start = row;
            row += 1;
            // Source and display must both advance together; a fold ends the run.
            while row < self.rows()
                && source_row(row) == Some(source + row - start)
                && source + row - start < source_top.saturating_add(height)
            {
                row += 1;
            }
            // Keep original image size/UVs; only the source-run translation changes.
            if let Some(mut sealed) = seal(
                placement,
                image,
                column,
                source_top as i64 - source as i64 + start as i64,
            ) {
                // Store exclusive display edges; the painter intersects the grid clip.
                sealed.clip_rows = Some([start as i64, row as i64]);
                self.images
                    .get_or_insert_with(Default::default)
                    .placements
                    .push(sealed);
            }
        }
    }
}
