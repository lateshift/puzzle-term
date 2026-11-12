//! Painter-order assembly from retained row geometry (§10.4).
//!
//! Backgrounds, glyphs and decorations form contiguous instance ranges.
//! Selection fills are appended after ordinary backgrounds and before text;
//! selection color affects only copies in the frame, preserving row caches.
//!
//! The row caches retain coordinates relative to a line's top edge. Assembly
//! supplies its current viewport y and padding x, including the integral
//! fractional-scroll displacement. Moving a retained line does not reshape it.
//! Background rows are copied first so selection paper can overwrite them.
//! The background count is captured only after selection fills are appended.
//!
//! Rect decorations and Slug ink live in separate instance arrays, but batches
//! restore painter order through z-slots. A background range cannot contain
//! an underline simply because its cache belongs to the same physical row.
//! Cursor assembly runs later and therefore remains above the selected text.
//!
//! Glyph blink flags and selection colors parallel the cached glyph vector.
//! Filtering a hidden blink glyph skips its color too, retaining a one-to-one
//! correspondence in the frame's temporary recolor stream. Curly underlines
//! follow glyphs with their own matching colors and stay in the text slot.
//! The temporary color Vec preserves capacity between unselected/selected
//! frames, just like RenderFrame's instance vectors (P1).
//! No cached transform or color is overwritten by these instance copies.

use super::{FrameBuilder, search, selection};
use crate::frame_snapshot::FrameSnapshot;
use puzzle_frame::{Batch, PipelineKind, RectU32, RenderFrame, ZSlot};
/// Builds transient screen-space copies while preserving retained row geometry.
impl FrameBuilder {
    /// Shifts row-relative instances into the padded device-pixel grid.
    pub(super) fn assemble(
        &mut self,
        snapshot: &FrameSnapshot,
        frame: &mut RenderFrame,
        width: u32,
        height: u32,
    ) {
        // This scratch stream follows emitted Slug instances, including blink filtering.
        self.selection_colors.clear();
        // The grid clip excludes padding. Scroll animations can later put
        // a fraction of a cached row outside this rectangle (§10.3).
        let clip = Some(self.grid_clip(frame.target_size, width, height));
        super::images::append(self, snapshot, frame, clip);
        // Backgrounds form one contiguous range in the rect buffer, even
        // though their source caches are organized by terminal row.
        for (index, row) in self.rows.iter().take(self.visible_rows).enumerate() {
            // Row-relative geometry remains reusable after scroll; only
            // these copied instances acquire the new screen y offset.
            let offset = self.padding as f32
                + index as f32 * self.metrics.height_px as f32
                + self.row_shift as f32;
            for source in &row.backgrounds {
                let mut instance = *source;
                instance.rect[0] += self.grid_x() as f32;
                instance.rect[1] += offset;
                frame.rects.push(instance);
            }
        }
        // Selected paper overwrites application paper but remains below glyph ink.
        search::append_fills(self, snapshot, frame);
        selection::append_fills(self, snapshot, frame);
        let backgrounds = frame.rects.len() as u32;
        // Decorations share the rect pipeline, but belong above glyphs in
        // painter order. Their range begins after all background runs.
        for (index, row) in self.rows.iter().take(self.visible_rows).enumerate() {
            let offset = self.padding as f32
                + index as f32 * self.metrics.height_px as f32
                + self.row_shift as f32;
            for ((source, blink), selected) in row
                .glyphs
                .iter()
                .zip(&row.glyph_blink)
                .zip(&row.glyph_selection_ink)
            {
                // Skipping the glyph also skips its selected color to retain parallel ordering.
                if *blink && !self.blink_on {
                    continue;
                }
                let mut instance = *source;
                instance.t[0] += self.grid_x() as f32;
                instance.t[1] += offset;
                frame.slugs.push(instance);
                self.selection_colors.push(*selected);
            }
            // Curly underlines occupy the same text z-slot but are drawn
            // after their row's glyph ink, without per-cell draw calls.
            for (source, selected) in row.waves.iter().zip(&row.wave_selection_ink) {
                let mut instance = *source;
                instance.t[0] += self.grid_x() as f32;
                instance.t[1] += offset;
                frame.slugs.push(instance);
                self.selection_colors.push(*selected);
            }
            for source in &row.decorations {
                let mut instance = *source;
                instance.rect[0] += self.grid_x() as f32;
                instance.rect[1] += offset;
                frame.rects.push(instance);
            }
        }
        let decorations = frame.rects.len() as u32;
        append_batches(frame, backgrounds, decorations, clip);
        // Complementary scissors recolor partial ligatures without repainting coverage twice.
        selection::partition_ink(self, snapshot, frame, clip);
        super::raster::assemble(self, snapshot, frame, clip);
    }
}

/// Uses one range per painter layer regardless of the visible row count.
fn append_batches(
    frame: &mut RenderFrame,
    backgrounds: u32,
    decorations: u32,
    clip: Option<RectU32>,
) {
    // Separate batches preserve the z-slot contract while keeping the
    // draw count constant with respect to the number of cells (P5).
    frame.batches.push(Batch {
        slot: ZSlot::Background,
        pipeline: PipelineKind::Rect,
        instances: 0..backgrounds,
        scissor: clip,
    });
    frame.batches.push(Batch {
        slot: ZSlot::Text,
        pipeline: PipelineKind::Slug,
        instances: 0..frame.slugs.len() as u32,
        scissor: clip,
    });
    frame.batches.push(Batch {
        slot: ZSlot::Text,
        pipeline: PipelineKind::Rect,
        instances: backgrounds..decorations,
        scissor: clip,
    });
}
