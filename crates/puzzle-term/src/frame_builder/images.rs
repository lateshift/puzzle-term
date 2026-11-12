//! Kitty images retain one source-to-destination transform per placement.
//!
//! Placeholder cells are windows onto that transform, not independently scaled
//! image crops. Integer device-pixel scissors partition adjoining cells exactly;
//! the raster shader interpolates the same full-image UVs on either side of a seam.
//! Contiguous cells are coalesced into row spans, then identical adjacent rows
//! into rectangles. Editing a hole preserves that hole instead of painting over it.
//! Sources remain immutable and independent of the glyph atlas (M15.3/M15.6).
//!
//! Coordinate spaces:
//! - anchor is the printed cell in the visible viewport, possibly above row zero;
//! - tile is the row/column inside the virtual prototype, not inside the screen;
//! - crop is the source rectangle in decoded image pixels;
//! - rect is the complete placement in device pixels, before any cell clipping.
//! Subtracting tile from anchor recovers the placement origin. A moved or reflowed
//! tile can have a different origin from its neighbours; it must then stay separate.
//! A physical placement has no tile coordinate and needs only the viewport scissor.
//!
//! Fitting each cell's crop independently restarts interpolation at every boundary.
//! An odd-height photo gives fractional UV crops whose rounding can differ across
//! separately projected quads, even though their ideal source edges agree. Full-placement
//! mapping preserves one continuous sampling function; scissors only restrict ink.
//! The image shader evaluates that function at exact device-pixel centres, avoiding
//! translation-dependent vertex interpolation at otherwise identical clipped quads.
//!
//! The publication has already sorted equal source/z placements into screen order.
//! A peekable iterator can therefore join consecutive cells without allocating a
//! temporary span list. Prototype coordinates must also be consecutive: adjacent
//! screen cells alone are insufficient after an edit, copy or horizontal movement.
//! Whole consecutive spans can then extend the last batch's scissor vertically.
//! This preserves the painter order and cannot cover a missing cell between spans.
//!
//! Coalescing checks source allocation identity, not just the client's wire ID.
//! Retransmission may reuse that ID while an older sealed frame still owns pixels.
//! Comparing the immutable source prevents joining those distinct generations.
//! All checks are independent of the client or image contents; there is no yazi
//! detection, preview-size special case or image-specific sampling threshold.
//! See kitty graphics protocol, "Unicode placeholders" and "displayed image layout".
use super::FrameBuilder;
use crate::frame_snapshot::{FrameSnapshot, images::Placement};
use puzzle_frame::{RasterRef, RectU32, RenderFrame, ZSlot};

/// Emits physical placements and clipped placeholder spans without temporary buffers.
pub(super) fn append(
    builder: &FrameBuilder,
    snapshot: &FrameSnapshot,
    frame: &mut RenderFrame,
    clip: Option<RectU32>,
) {
    // Sessions that never enable graphics have no image publication to traverse.
    let Some(images) = &snapshot.images else {
        return;
    };
    // An empty image set must not add another pass or allocate a source table.
    if images.placements.is_empty() && frame.images.is_none() {
        return;
    }
    // Forward a changed allowance even when no currently visible source survives.
    frame.images.get_or_insert_with(Default::default).budget = images.budget;
    // Borrow the sealed list; its retained allocation remains with the snapshot.
    let mut placements = images.placements.iter().peekable();
    while let Some(placement) = placements.next() {
        // Adjacent text cells share a transform only if their prototype coordinates
        // and actual screen coordinates both advance by exactly one column.
        let mut columns = 1;
        while let Some(next) = placements.peek()
            && contiguous(placement, next, columns)
        {
            columns += 1;
            placements.next();
        }
        emit(builder, placement, columns, frame, clip);
    }
}

/// A row span cannot cross a different crop, prototype, z layer or text location.
fn contiguous(first: &Placement, next: &Placement, columns: i64) -> bool {
    // Ordinary puts are independent placements even if their screen rectangles abut.
    let (Some([row, column]), Some(tile)) = (first.tile, next.tile) else {
        return false;
    };
    // Immutable pixel identity distinguishes retransmitted wire IDs and sources.
    first.source == next.source
        && first.crop == next.crop
        && first.offset == next.offset
        && first.cells == next.cells
        && first.z == next.z
        && next.anchor == [first.anchor[0] + columns, first.anchor[1]]
        && tile == [row, column.saturating_add(columns as u32)]
}

/// The entire crop maps once; only its visible cell window differs between spans.
fn emit(
    builder: &FrameBuilder,
    placement: &Placement,
    columns: i64,
    frame: &mut RenderFrame,
    clip: Option<RectU32>,
) {
    let rect = destination(builder, placement);
    // A user fold translates separate source runs without rescaling the image.
    // Clip each run before placeholder clipping; default images take no extra pass.
    let clip = if let Some(rows) = placement.clip_rows {
        let Some(bounds) = projection_clip(builder, rows, frame.target_size, clip) else {
            return;
        };
        Some(bounds)
    } else {
        clip
    };
    // A scissor crops coverage without resetting the interpolation at each cell.
    // Physical placements use just the ordinary viewport clip.
    let clip = if placement.tile.is_some() {
        let Some(bounds) = tile_clip(builder, placement, columns, frame.target_size, clip) else {
            return;
        };
        Some(bounds)
    } else {
        clip
    };
    // UVs cover the complete admitted crop, regardless of how many cells survived.
    let [x, y, w, h] = placement.crop;
    let [sw, sh] = placement.source.size().map(|value| value as f32);
    let source = RasterRef {
        // Cloning immutable ownership is cheap; this never copies decoded bytes.
        image: placement.source.clone(),
        uv: [
            x as f32 / sw,
            y as f32 / sh,
            (x + w) as f32 / sw,
            (y + h) as f32 / sh,
        ],
    };
    // Kitty's very-low-z boundary is strict; image paper precedes cell backgrounds.
    let slot = if placement.z < i32::MIN / 2 {
        ZSlot::Background
    } else if placement.z < 0 {
        ZSlot::BelowText
    } else {
        ZSlot::AboveText
    };
    // A rectangular union changes only the last scissor, not its sampling transform.
    if !merge_span(frame, &source, rect, slot, clip) {
        frame.append_image(&source, rect, slot, clip);
    }
}

/// Resolves the complete prototype rectangle before applying its printed cell windows.
fn destination(builder: &FrameBuilder, placement: &Placement) -> [f32; 4] {
    let cell = [builder.metrics.width_px, builder.metrics.height_px];
    // Both explicit dimensions are bounds: the protocol requires aspect-preserving fit.
    let size = crate::images::geometry::display_size(
        [placement.crop[2], placement.crop[3]],
        placement.cells,
        cell,
    )
    .map(|value| value as f32);
    // Signed arithmetic retains a prototype origin above/left of the current viewport.
    let [row, column] = placement.tile.unwrap_or([0, 0]).map(i64::from);
    // Subtract in integer cell space first. All cells of a placement then have
    // bit-identical float origins, including negative history rows during scrolling.
    [
        builder.grid_x() as f32
            + (placement.anchor[0] - column) as f32 * cell[0] as f32
            + placement.offset[0] as f32,
        // Fractional viewport scrolling affects coverage and sampling by the same shift.
        builder.padding as f32
            + (placement.anchor[1] - row) as f32 * cell[1] as f32
            + builder.row_shift as f32
            + placement.offset[1] as f32,
        size[0],
        size[1],
    ]
}

/// Shared rounded edges partition a fractionally scrolled grid without pixel gaps.
fn tile_clip(
    builder: &FrameBuilder,
    placement: &Placement,
    columns: i64,
    target: [u32; 2],
    viewport: Option<RectU32>,
) -> Option<RectU32> {
    // Use doubles until the shared-edge rounding, retaining precision for offscreen anchors.
    let cell = [builder.metrics.width_px, builder.metrics.height_px];
    let x = builder.grid_x() as f64 + placement.anchor[0] as f64 * f64::from(cell[0]);
    let y = builder.padding as f64
        + placement.anchor[1] as f64 * f64::from(cell[1])
        + f64::from(builder.row_shift);
    // Clip before converting signed coordinates to u32, preserving partial edge rows.
    // Grid captures normally supply a clip; a standalone frame uses its target.
    // Bounds stay unsigned only after intersection, never before negative scroll.
    let viewport = viewport.unwrap_or(RectU32 {
        x: 0,
        y: 0,
        width: target[0],
        height: target[1],
    });
    // Every span computes the same shared boundary; rounding widths separately could gap.
    let left = x.round().max(f64::from(viewport.x));
    let top = y.round().max(f64::from(viewport.y));
    let right = (x + columns as f64 * f64::from(cell[0]))
        .round()
        .min(f64::from(viewport.x.saturating_add(viewport.width)));
    let bottom = (y + f64::from(cell[1]))
        .round()
        .min(f64::from(viewport.y.saturating_add(viewport.height)));
    // Entirely clipped tiles cannot emit an invalid or zero-area GPU scissor.
    (right > left && bottom > top).then_some(RectU32 {
        x: left as u32,
        y: top as u32,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    })
}

/// Device-pixel row edges crop folded runs without changing texture interpolation.
fn projection_clip(
    builder: &FrameBuilder,
    rows: [i64; 2],
    target: [u32; 2],
    viewport: Option<RectU32>,
) -> Option<RectU32> {
    let viewport = viewport.unwrap_or(RectU32 {
        x: 0,
        y: 0,
        width: target[0],
        height: target[1],
    });
    // Reuse the exact padding, cell height and integral scroll displacement
    // used by text. Independent rounding of image heights could create seams.
    let edge = |row: i64| {
        f64::from(builder.padding)
            + row as f64 * f64::from(builder.metrics.height_px)
            + f64::from(builder.row_shift)
    };
    // Exclusive row bands abut in device pixels; no sampling transform changes.
    let top = edge(rows[0]).round().max(f64::from(viewport.y));
    let bottom = edge(rows[1])
        .round()
        .min(f64::from(viewport.y.saturating_add(viewport.height)));
    // Zero-area bands produce no draw, including a clipped fractional edge.
    // Keeping the full viewport width cannot expose another source row.
    (bottom > top).then_some(RectU32 {
        x: viewport.x,
        y: top as u32,
        width: viewport.width,
        height: (bottom - top) as u32,
    })
}

/// Whole neighbouring rows need one quad, not a draw call per placeholder cell.
fn merge_span(
    frame: &mut RenderFrame,
    source: &RasterRef,
    rect: [f32; 4],
    slot: ZSlot,
    clip: Option<RectU32>,
) -> bool {
    // Only the immediate previous draw may extend; searching backwards would reorder ink.
    let Some(batch) = frame.batches.last_mut() else {
        return false;
    };
    let puzzle_frame::PipelineKind::Image(index) = batch.pipeline else {
        return false;
    };
    // The previous instance supplies the full transform that a union must preserve.
    let Some(previous) = frame.rasters.get(batch.instances.start as usize) else {
        return false;
    };
    // Never merge across another source, layer, transform or painter command.
    if batch.slot != slot
        || batch.instances.len() != 1
        || previous.rect != rect
        || previous.uv != source.uv
        || frame
            .images
            .as_ref()
            .and_then(|i| i.sources.get(index as usize))
            != Some(&source.image)
    {
        return false;
    }
    // An unclipped physical draw has no cell window to union with a later tile.
    let (Some(mut old), Some(next)) = (batch.scissor, clip) else {
        return false;
    };
    // Equal widths and abutting edges guarantee the union contains no unprinted cell.
    if old.x != next.x || old.width != next.width || old.y + old.height != next.y {
        return false;
    }
    // No new instance or source-table entry is needed for another identical row.
    old.height += next.height;
    batch.scissor = Some(old);
    true
}
