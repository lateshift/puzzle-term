//! Bitmap ink remains on the same cell grid as Slug (§8.9, §10.3).
//!
//! Rasterizers return cropped samples relative to the primary baseline.
//! This module packs them once, then retains plain row-relative quads.
//! Cached rows refresh page ages during assembly, so LRU counts actual draws.
//! Selection uses complementary scissors, including half of a wide cluster;
//! masks receive selected tint while color texels bypass tint in the shader.
//! Pending uploads transfer to RenderFrame and survive skipped presentations.
//! Bitmap samples are independent of their destination rows. UVs name the
//! shared page while the destination rectangle is copied into each frame.
//! Scroll displacement and padding change only the copied device rectangle.
//! No browser text metric is permitted to redefine a cluster's advance.
//!
//! FontSet's reserved source IDs are distinct from OpenType face indices.
//! Their glyph IDs index retained whole-cluster requests, not Unicode scalars.
//! Exact device-size and DPR bits separate rasterizations at different scales.
//! This cache is checked before calling the expensive platform rasterizer.
//!
//! The crop origin is measured from the primary baseline, not the foreign
//! font's ascender. Adding it to the placement reproduces the fitted cell ink.
//! The rectangle's dimensions are the crop dimensions, excluding atlas padding.
//! UV maximums point to the exclusive crop edge; fragment centers sample texels.
//! Flags select both the independent format and its stable texture-array page.
//!
//! SGR blink filtering happens before copying the selected-color stream.
//! Both streams therefore share indices even when some cached quads are hidden.
//! A timer changes only assembly; it does not invoke Canvas or shape a row.
//! An isolated cursor run retains bitmap copies just as it retains Slug layers.
//! Filled cursors draw those quads after their rectangle in the cursor z-slot.
//!
//! Selected color copies preserve every crop, UV and page bit. Complementary
//! integer scissors prevent antialiased edges being blended twice at a boundary.
//! Selecting half a two-cell character changes precisely that cell's mask tint.
//! Fixed emoji RGB ignores both selected tint and cursorText in the fragment.
//! Selection region count is bounded independently of visible terminal height.
//! Unselected rows keep their cache colors for a later selection-free frame.
//!
//! Upload ownership leaves this module only through RenderFrame's pending Vec.
//! Clearing next-frame instances does not acknowledge these texture writes.
//! A skipped surface acquisition can retry the same ordered upload stream.
//! Page ages are refreshed from emitted quads, including cached and cursor ink.
//! This keeps unchanged but visible pages ahead of pages no longer drawn.
//! Restore writes use the same conversion path as ordinary cropped insertions.
//! The core remains GPU-independent and never exposes JS canvas objects.
use super::{CachedRow, FrameBuilder, context::TextContext, selection};
use crate::frame_snapshot::FrameSnapshot;
use puzzle_frame::{Batch, PipelineKind, RasterInstance, RectU32, RenderFrame, ZSlot};
use puzzle_text::{
    glyph::{BitmapKind, GlyphPlacement},
    raster::{PAGE, RasterHandle},
};

/// Protects every cache hit before a cold insertion can reclaim an older page.
pub(super) fn pin_cached(
    builder: &FrameBuilder,
    context: &mut TextContext,
    snapshot: &FrameSnapshot,
) {
    // Cache ordering already follows the new viewport; spare offscreen rows need no pin.
    // Pinning happens before row zero rebuilds, including a later unchanged cursor row.
    // Every inserted crop stamps its own page, protecting newly emitted cold-row quads.
    // Recency alone is insufficient: a page can be old yet still appear later this frame.
    for (index, row) in builder.rows.iter().take(builder.visible_rows).enumerate() {
        let Some(meta) = snapshot.row_meta(index) else {
            continue;
        };
        // Changed rows release their old addresses; unchanged rows have no copied cells.
        if row.id == meta.id
            && row.version == meta.version
            && row.block_signature == meta.block_signature
            && row.style_epoch == builder.style_epoch
            && row.atlas_generation == context.atlas_generation()
            && row.cursor_column == meta.cursor_column
        {
            for draw in &row.rasters {
                mark_draw(&mut context.raster, draw);
            }
        }
    }
}
/// Decodes only the format and layer bits; effect metadata occupies other flags.
pub(in crate::frame_builder) fn mark_draw(
    atlas: &mut puzzle_text::raster::RasterAtlas,
    draw: &RasterInstance,
) {
    let kind = if draw.flags & 1 == 0 {
        BitmapKind::Mask
    } else {
        BitmapKind::Color
    };
    atlas.mark_used(kind, (draw.flags >> 8) & 0xff);
}

/// Retains one prepared bitmap draw and its parallel appearance values.
pub(super) fn append(
    cache: &mut CachedRow,
    placement: &GlyphPlacement,
    handle: RasterHandle,
    blink: bool,
    selected: u32,
) {
    cache.rasters.push(instance(placement, handle));
    // The shader ignores tint for authored color but uses it for coverage masks.
    cache.raster_blink.push(blink);
    cache.raster_selection_ink.push(selected);
}

/// Reconstructs cropped cell ink while leaving the atlas gutter outside its UVs.
fn instance(placement: &GlyphPlacement, handle: RasterHandle) -> RasterInstance {
    let rect = handle.rect;
    // Bitmap coordinates are device pixels; only UVs normalize to the fixed page.
    RasterInstance {
        rect: [
            (placement.origin_px[0] + handle.offset[0]) as f32,
            (placement.origin_px[1] + handle.offset[1]) as f32,
            rect.width as f32,
            rect.height as f32,
        ],
        // UV edges enclose exactly the crop, with pixel centers landing at texel centers.
        uv: [
            rect.x as f32 / PAGE as f32,
            rect.y as f32 / PAGE as f32,
            (rect.x + rect.width) as f32 / PAGE as f32,
            (rect.y + rect.height) as f32 / PAGE as f32,
        ],
        // Authored-color sampling ignores this word; masks inherit the resolved text ink.
        tint: placement.color,
        // Layer bits are independent of the mask/color selection bit.
        flags: (u32::from(handle.epoch) << 16)
            | (handle.page << 8)
            | u32::from(handle.kind == BitmapKind::Color),
    }
}

/// Copies visible row quads without re-rasterizing unchanged clusters.
pub(super) fn assemble(
    builder: &mut FrameBuilder,
    snapshot: &FrameSnapshot,
    frame: &mut RenderFrame,
    clip: Option<RectU32>,
) {
    // Reuse the selection-color allocation when damage leaves most rows unchanged.
    builder.raster_selection_colors.clear();
    let first = frame.rasters.len() as u32;
    for (index, row) in builder.rows.iter().take(builder.visible_rows).enumerate() {
        // Cached coordinates are row-relative; scroll displacement may be fractional.
        let y = builder.padding as f32
            + index as f32 * builder.metrics.height_px as f32
            + builder.row_shift as f32;
        for ((source, blink), selected) in row
            .rasters
            .iter()
            .zip(&row.raster_blink)
            .zip(&row.raster_selection_ink)
        {
            // Omit blinking samples before indexing the parallel selection stream.
            if *blink && !builder.blink_on {
                continue;
            }
            // Copy plain quads without modifying the cached placement for later frames.
            let mut draw = *source;
            draw.rect[0] += builder.grid_x() as f32;
            draw.rect[1] += y;
            frame.rasters.push(draw);
            builder.raster_selection_colors.push(*selected);
        }
    }
    // One batch covers all pages; each instance chooses its array layer.
    frame.batches.push(Batch {
        slot: ZSlot::TextRaster,
        pipeline: PipelineKind::Raster,
        instances: first..frame.rasters.len() as u32,
        scissor: clip,
    });
    partition(builder, snapshot, frame, clip);
}

/// Applies selected tint to masks with exactly one coverage blend per pixel.
fn partition(
    builder: &FrameBuilder,
    snapshot: &FrameSnapshot,
    frame: &mut RenderFrame,
    clip: Option<RectU32>,
) {
    let Some(paint) = snapshot.paint() else {
        return;
    };
    // A background-only selection needs no duplicate text stream or extra scissors.
    if !builder.colors.selection_recolors(paint) {
        return;
    }
    // Without a visible grid rectangle there are no selected device pixels to draw.
    let Some(grid) = clip else {
        return;
    };
    // The same integer cell regions partition vector and raster text.
    let (selected, count) = selection::regions(builder, snapshot, grid);
    // An empty selection keeps the original single raster batch.
    if count == 0 {
        return;
    }
    // Find the text batch only; cursor raster ink belongs to its later z-slot.
    let Some(index) = frame
        .batches
        .iter()
        .position(|b| b.slot == ZSlot::TextRaster)
    else {
        return;
    };
    // Replace its draw regions, retaining the original quads as the ordinary stream.
    let original = frame.batches.remove(index);
    // Append a recolored stream so disjoint scissors can reference either slice.
    let first = frame.rasters.len() as u32;
    for index in original.instances.clone() {
        let mut draw = frame.rasters[index as usize];
        // Authored color ignores tint; mask alpha is applied by the shader.
        draw.tint = builder.raster_selection_colors[(index - original.instances.start) as usize];
        frame.rasters.push(draw);
    }
    let range = first..frame.rasters.len() as u32;
    // The complement excludes selected pixels, preventing double alpha blending.
    let (ordinary, remaining) = selection::complement(grid, &selected[..count]);
    for rect in ordinary.into_iter().take(remaining) {
        frame.batches.push(Batch {
            scissor: Some(rect),
            ..original.clone()
        });
    }
    // A selection of half a wide glyph changes only that cell's coverage tint.
    for rect in selected.into_iter().take(count) {
        frame.batches.push(Batch {
            instances: range.clone(),
            scissor: Some(rect),
            ..original.clone()
        });
    }
}

/// Redraws the isolated cursor cluster above a filled block.
pub(super) fn cursor(
    builder: &FrameBuilder,
    backend: &mut puzzle_text::glyph::slug::SlugBackend,
    frame: &mut RenderFrame,
    row: usize,
    y: f32,
    clip: Option<RectU32>,
    color: [u8; 3],
) {
    // Append a recolored stream so disjoint scissors can reference either slice.
    let first = frame.rasters.len() as u32;
    // Row rebuilding isolated this cluster at the cursor even when ligatures are on.
    for source in &builder.rows[row].cursor_rasters {
        let mut draw = *source;
        draw.rect[0] += builder.grid_x() as f32;
        draw.rect[1] += y;
        // CursorText affects masks; the shader preserves fixed emoji RGB.
        if draw.flags & puzzle_frame::raster::RASTER_EFFECT == 0 {
            draw.tint = super::colors::rgba(color);
        } else if draw.flags & puzzle_frame::raster::RASTER_RECOLOR == 0 {
            // A metadata index must be recolored through its retained matrix/alpha key.
            match backend.raster_cursor(draw.tint, super::colors::rgba(color)) {
                Ok(index) => draw.tint = index,
                Err(error) => tracing::warn!(event = "overlay.cursor_rejected", %error),
            }
        }
        frame.rasters.push(draw);
    }
    frame.batches.push(Batch {
        slot: ZSlot::Cursor,
        pipeline: PipelineKind::Raster,
        instances: first..frame.rasters.len() as u32,
        scissor: clip,
    });
}

/// GPU and CPU ink ownership is translated without changing cell-placement code.
mod uploads;
pub(super) use uploads::collect as uploads;
