//! Search paper fills share grid coordinates and the existing background draw.
//!
//! Snapshot ranges contain only visible hits. Their rectangles overwrite SGR
//! backgrounds before selection and text, so selection remains the higher
//! priority cell interaction. Cached glyphs and shader math are unchanged.
//! Both fractional edge rows use the ordinary grid scissor and integral y
//! displacement. A cross-wrap hit paints partial first and last cell intervals.
use super::FrameBuilder;
use crate::frame_snapshot::FrameSnapshot;
use puzzle_frame::{RectInstance, RenderFrame};
/// Adds current query paper to the contiguous background instance range.
pub(super) fn append_fills(
    builder: &FrameBuilder,
    snapshot: &FrameSnapshot,
    frame: &mut RenderFrame,
) {
    // Visible filtering already happened at capture; no query text is read here.
    for hit in snapshot.search() {
        // A wrapped match contributes one clipped interval on each physical row.
        for row in 0..snapshot.rows() {
            let Some(id) = snapshot.row_meta(row).and_then(|meta| meta.id) else {
                continue;
            };
            // Column ranges are exclusive at the end, matching rectangle widths.
            let Some((first, end)) = hit.range.columns(id, snapshot.columns()) else {
                continue;
            };
            // These same coordinates place cached glyph ink and selection fills.
            frame.rects.push(RectInstance {
                rect: [
                    builder.grid_x() as f32 + first as f32 * builder.metrics.width_px as f32,
                    builder.padding as f32
                        + row as f32 * builder.metrics.height_px as f32
                        + builder.row_shift as f32,
                    (end - first) as f32 * builder.metrics.width_px as f32,
                    builder.metrics.height_px as f32,
                ],
                // Current status chooses only paper; original SGR ink remains cached.
                color: builder.colors.search_background(hit.current),
                // Cell-aligned paper requires square edges and no antialias ramp.
                corner_radius: 0.0,
                stroke_width: 0.0,
                flags: 0,
            });
        }
    }
}
