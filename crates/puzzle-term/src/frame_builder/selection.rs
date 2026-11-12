//! Cell-aligned selection fills and bounded recolor clipping (§10.5 rule 6).
//!
//! The Slug shader is unchanged. Text draws use complementary scissors so a
//! ligature crossing a selection edge retains its exact coverage on both sides.
//! Drawing recolored ink over the original would blend edge coverage twice;
//! partitioning avoids that dark fringe. A linear selection has at most three
//! rectangular regions (first row, middle rows, last row); a block has one.
//! Draw count is independent of the number of selected rows or glyphs (P5).
//!
//! Fills use the snapshot's expanded cell range and current device metrics.
//! One rectangle is emitted for each selected row, including clipped edge
//! rows during fractional scrolling. The existing background batch paints
//! these rectangles after application cell paper and before all glyph ink.
//! Their coordinates share exactly the row displacement used by assembly.
//!
//! Recoloring is an assembly operation, never a row-cache key. Each prepared
//! glyph has both ordinary and selected colors resolved from the same cell
//! policy. The latter applies selection before minimum-contrast adjustment,
//! so an absent foreground token still compares against the selected paper.
//! Blink filtering preserves the matching assembly-order color stream.
//!
//! The selected regions merge adjacent rows with identical column intervals.
//! A linear range has a partial first row, a full middle band and a partial
//! last row; any empty or adjacent equal regions disappear in that merge.
//! A rectangular range has one vertical band. Both are bounded shapes even
//! when a user selects thousands of retained lines through the contents API.
//!
//! Subtracting those regions from the grid gives complementary ordinary-ink
//! scissors. All scissors have integer device edges and never overlap within
//! their own layer. Both color copies therefore shade each pixel exactly once.
//! This matters at antialiased edges and for a ligature crossing a cell edge.
//! Shader dilation and atlas lookup remain the unchanged Slug implementation.
//! Stack arrays hold the bounded region algebra without allocating per frame.
//! Glyph/instance Vec capacities are reused after the first selected frame.

use super::FrameBuilder;
use crate::frame_snapshot::FrameSnapshot;
use puzzle_frame::{Batch, PipelineKind, RectInstance, RectU32, RenderFrame, ZSlot};

/// Appends exactly one background rectangle per selected visible row.
pub(super) fn append_fills(
    builder: &FrameBuilder,
    snapshot: &FrameSnapshot,
    frame: &mut RenderFrame,
) {
    let Some(paint) = snapshot.paint() else {
        return;
    };
    // An absent model emits neither background paper nor region algebra.
    let Some(range) = snapshot.selection() else {
        return;
    };
    // Include both clipped edge rows from a fractional viewport capture.
    // Their fills use the same grid scissor as ordinary cached backgrounds.
    for index in 0..snapshot.rows() {
        // A vacant snapshot slot has no document identity to select.
        let Some(id) = snapshot.row_meta(index).and_then(|row| row.id) else {
            continue;
        };
        // The interval is half-open, so its width is end minus first.
        // This handles a partial first/last row and a rectangular slice uniformly.
        let Some((first, end)) = range.columns(id, snapshot.columns()) else {
            continue;
        };
        // The same rounded y displacement is used for glyphs and their cells.
        frame.rects.push(RectInstance {
            rect: [
                builder.grid_x() as f32 + first as f32 * builder.metrics.width_px as f32,
                builder.padding as f32
                    + index as f32 * builder.metrics.height_px as f32
                    + builder.row_shift as f32,
                (end - first) as f32 * builder.metrics.width_px as f32,
                builder.metrics.height_px as f32,
            ],
            // OSC 17/21 paper belongs to the publication containing this range,
            // so a later parser change cannot recolour a pending selection frame.
            color: builder.colors.selection_background(paint),
            // Cell fills have square, integral edges; no shape smoothing is needed.
            corner_radius: 0.0,
            stroke_width: 0.0,
            flags: 0,
        });
    }
}
/// Replaces the ordinary text draw with disjoint original/selection scissors.
pub(super) fn partition_ink(
    builder: &FrameBuilder,
    snapshot: &FrameSnapshot,
    frame: &mut RenderFrame,
    clip: Option<RectU32>,
) {
    let Some(paint) = snapshot.paint() else {
        return;
    };
    // Original SGR colors require no second glyph stream when contrast is off.
    // Even in that case append_fills has already supplied the selected paper.
    // The sealed OSC 19 state decides whether selected ink needs a separate draw.
    if !builder.colors.selection_recolors(paint) {
        return;
    }
    // Selection affects only grid pixels, never surrounding padding.
    let Some(grid) = clip else {
        return;
    };
    // Merge the cell intervals before issuing draws: selected row count
    // must not become draw count when an API selects the entire scrollback.
    let (selected, count) = regions(builder, snapshot, grid);
    if count == 0 {
        return;
    }
    // Locate the complete ordinary Slug range before replacing its scissors.
    // Rectangular underlines remain in their separate decoration range.
    let Some(batch_index) = frame
        .batches
        .iter()
        .position(|batch| batch.slot == ZSlot::Text && batch.pipeline == PipelineKind::Slug)
    else {
        return;
    };
    // Remove the unrestricted draw so antialias coverage is never blended twice.
    let original = frame.batches.remove(batch_index);
    let end = frame.slugs.len();
    // Copy prepared instances, changing only color. No glyph is shaped again.
    for index in original.instances.clone() {
        let mut ink = frame.slugs[index as usize];
        // Colors were resolved from the source cell before its ordinary contrast.
        // The parallel stream includes curly underline instances and blink filtering.
        ink.color =
            (builder.selection_colors[index as usize] & 0x00ffffff) | (ink.color & 0xff000000);
        frame.slugs.push(ink);
    }
    // Recolored geometry occupies one appended range shared by all selected clips.
    let recolored = end as u32..frame.slugs.len() as u32;
    // The ordinary complement and selected regions partition the whole grid.
    // Shared boundaries are integer pixel edges, so neither side loses coverage.
    let (ordinary, remaining) = complement(grid, &selected[..count]);
    // Each ordinary rectangle references the same unchanged instance stream.
    for rect in ordinary.into_iter().take(remaining) {
        frame.batches.push(Batch {
            scissor: Some(rect),
            ..original.clone()
        });
    }
    // The selected stream changes only ink; origins, atlas addresses and bands agree.
    for rect in selected.into_iter().take(count) {
        frame.batches.push(Batch {
            slot: ZSlot::Text,
            pipeline: PipelineKind::Slug,
            instances: recolored.clone(),
            scissor: Some(rect),
        });
    }
}
/// Coalesces equal column intervals vertically into at most three regions.
pub(super) fn regions(
    builder: &FrameBuilder,
    snapshot: &FrameSnapshot,
    grid: RectU32,
) -> ([RectU32; 3], usize) {
    // Linear selections have at most three column bands; blocks have one.
    // Fixed storage therefore covers any number of physical selected rows (P1).
    let mut result = [RectU32 {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }; 3];
    let mut count = 0;
    // An absent model emits neither background paper nor region algebra.
    let Some(range) = snapshot.selection() else {
        return (result, count);
    };
    // Traversal is in screen order, permitting adjacent equal-width bands to merge.
    for row in 0..snapshot.rows() {
        let Some(id) = snapshot.row_meta(row).and_then(|meta| meta.id) else {
            continue;
        };
        // The interval is half-open, so its width is end minus first.
        // This handles a partial first/last row and a rectangular slice uniformly.
        let Some((first, end)) = range.columns(id, snapshot.columns()) else {
            continue;
        };
        // Convert this interval to a clipped device rectangle before coalescing.
        let Some(rect) = row_region(builder, grid, row, first, end) else {
            continue;
        };
        // Coalescing requires identical columns and an exactly touching lower edge.
        // A partial first row cannot merge with a differently sized middle band.
        if count > 0
            && result[count - 1].x == rect.x
            && result[count - 1].width == rect.width
            && result[count - 1].y + result[count - 1].height == rect.y
        {
            result[count - 1].height += rect.height;
        } else if count < result.len() {
            result[count] = rect;
            count += 1;
        }
    }
    (result, count)
}
/// Subtracts the selected regions with stack storage, preserving exact coverage.
pub(super) fn complement(grid: RectU32, selected: &[RectU32]) -> ([RectU32; 8], usize) {
    // A three-band linear selection leaves at most four complement rectangles.
    // Eight slots also cover temporary splits while subtracting each disjoint band.
    let mut pieces = [RectU32 {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }; 8];
    pieces[0] = grid;
    let mut count = 1;
    for &cut in selected {
        // Rebuild from a value copy so a newly emitted piece is not cut twice.
        let old = pieces;
        let old_count = count;
        // Compact only surviving nonempty pieces into the reused fixed storage.
        // This count is the range later submitted to ordinary-ink drawing.
        count = 0;
        for rect in old.into_iter().take(old_count) {
            // Each disjoint selected band splits a surviving piece into at most four.
            for part in subtract(rect, cut) {
                // Omit empty strips along coincident boundaries. The bounded
                // linear/block geometry cannot exceed this stack array’s capacity.
                if part.width > 0 && part.height > 0 && count < pieces.len() {
                    pieces[count] = part;
                    count += 1;
                }
            }
        }
    }
    (pieces, count)
}

/// Converts one selected physical row interval to the fixed grid’s integer scissor.
fn row_region(
    builder: &FrameBuilder,
    grid: RectU32,
    row: usize,
    first: usize,
    end: usize,
) -> Option<RectU32> {
    // Keep signed y until clipping: the first fractional row can begin above grid.
    let y = builder.padding as i64
        + row as i64 * i64::from(builder.metrics.height_px)
        + i64::from(builder.row_shift);
    // Clamp fractional edge rows to the fixed grid, including a surface crop.
    let top = y.max(i64::from(grid.y));
    let bottom = (y + i64::from(builder.metrics.height_px)).min(i64::from(grid.y + grid.height));
    // Horizontal endpoints already name whole cells in the snapshot’s width.
    let x = builder.grid_x() + first as u32 * builder.metrics.width_px;
    let right = (builder.grid_x() + end as u32 * builder.metrics.width_px).min(grid.x + grid.width);
    // A wholly clipped edge row must not yield a zero-size GPU scissor.
    if bottom <= top || right <= x {
        return None;
    }
    Some(RectU32 {
        x,
        y: top as u32,
        width: right - x,
        height: (bottom - top) as u32,
    })
}

/// Subtracts one rectangle into nonoverlapping top, bottom, left and right strips.
fn subtract(rect: RectU32, cut: RectU32) -> [RectU32; 4] {
    // Intersection coordinates are integer pixel boundaries, not coverage samples.
    let x = rect.x.max(cut.x);
    let y = rect.y.max(cut.y);
    let right = (rect.x + rect.width).min(cut.x + cut.width);
    let bottom = (rect.y + rect.height).min(cut.y + cut.height);
    if x >= right || y >= bottom {
        // No overlap preserves the rectangle; the other entries are empty sentinels.
        return [
            rect,
            RectU32 {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            RectU32 {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            RectU32 {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
        ];
    }
    // Top/bottom span full width; side strips cover only the intersected vertical band.
    [
        RectU32 {
            height: y - rect.y,
            ..rect
        },
        RectU32 {
            y: bottom,
            height: rect.y + rect.height - bottom,
            ..rect
        },
        RectU32 {
            y,
            width: x - rect.x,
            height: bottom - y,
            ..rect
        },
        RectU32 {
            x: right,
            y,
            width: rect.x + rect.width - right,
            height: bottom - y,
        },
    ]
}
