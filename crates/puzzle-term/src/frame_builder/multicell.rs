//! OSC 66 ink joins existing instanced row streams, not per-glyph draws.
//!
//! Each rectangle is shaped once on its first visible fragment. Its whole
//! transform includes the original hidden rows; viewport scissors clip it.
//! Selection and cursor tint therefore reuse one transform across fragments.
//! Outline-backed text is always unhinted. Pixel-only sources are prepared at
//! the effective device size before returning to neutral placement geometry.
//!
//! Ordinary row painting emits the reserved cells' paper and SGR decorations,
//! then breaks its shaping run at the footprint. This module fills only sparse
//! ink owners after that row pass; continuation cells cannot emit extra glyphs.
//! Natural placements remain saved for effects, while the cached vector/raster
//! instances already contain the complete sizing map. A provider rebuild uses
//! those saved sources and composes its map after sizing instead of multiplying
//! an already transformed instance a second time.
//!
//! Three boundaries distinguish newly emitted vector, raster and hit geometry.
//! Parallel blink, palette and selection arrays follow their corresponding ink.
//! Cursor isolation copies the owner's complete stream even when the physical
//! caret addresses a lower fragment. Normal assembly adds the final row origin.
//! A clipped root subtracts original hidden rows before that assembly translation.
//!
//! The natural layout uses unhinted glyphs at every DPR. Bitmap-only fallback
//! cannot supply outlines, so it receives the effective magnification before
//! source dispatch and retains the existing premultiplied bilinear effect map.
//! An ink-union fit may reduce that initial magnification; the source then merely
//! oversamples the final map, preserving coverage rather than stretching texels.
//! Neutral hit boxes and host extents remain logical pixels after DPR conversion.
//! No glyph state or source payload is borrowed from the mutable Session.
use super::{FrameBuilder, context::TextContext, overlays::ink, row::RunStyle};
use crate::frame_snapshot::{FrameSnapshot, multicell::Block};
use kurbo::{Affine, Point};
use puzzle_vt::cell::Attrs;

/// Startup outlines remain drawable before the complete shaping font arrives.
mod pack;
/// One natural ink union is fitted into the protocol's aligned render area.
mod placement;

/// One block's source interval and immutable natural-to-grid map.
#[derive(Clone)]
pub(super) struct SourceMap {
    /// Consecutive requests emitted by the one natural layout.
    pub sources: std::ops::Range<usize>,
    /// Whole-block transform, shared by every glyph and fragment.
    pub map: Affine,
    /// Physical occupied first column and width for effect picking.
    pub cells: [usize; 2],
    /// At most seven retained fragments; effects on a continuation select the owner.
    pub lines: [Option<puzzle_vt::LineId>; 7],
}

/// Appends only the sparse records owned by this rebuilt row.
/// `index` is the snapshot row whose ordinary paper and text were rebuilt first.
/// Missing sealed metadata returns a local build error; no live VT lookup is used.
pub(super) fn append(
    builder: &mut FrameBuilder,
    context: &mut TextContext,
    snapshot: &FrameSnapshot,
    index: usize,
) -> Result<(), String> {
    // Observe dispatch as well as successful ink: an empty sparse call on an
    // ordinary row would otherwise disappear from the no-unused-work proof.
    #[cfg(test)]
    super::testing_work::charge(5, 1);
    // Paint policy and source attributes belong to the same sealed publication.
    let paint = snapshot.paint().ok_or("text block paint missing")?;
    // Only the first visible fragment owns ink; later rows still paint their cells.
    for block in snapshot.row_blocks(index) {
        // The first visible fragment carries the same style as the original root.
        let cell = snapshot
            .retained_row(index)
            .and_then(|row| row.cells.get(block.value.column))
            .ok_or("text block source cell missing")?;
        // Hidden text retains storage and selection extents without fallback work.
        if cell.attrs.contains(Attrs::HIDDEN) {
            continue;
        }
        // Match ordinary §10.5 color precedence; selection tint stays parallel.
        let (base, _) = builder
            .colors
            .resolve_uncontrasted(paint, cell.fg, cell.bg, cell.attrs);
        let (color, _) = builder.colors.resolve(paint, cell.fg, cell.bg, cell.attrs);
        // Keep these alternatives parallel so selection does not reshape the block.
        let style = RunStyle {
            face: super::row::font_style(cell.attrs),
            ink: color,
            selection_ink: builder
                .colors
                .selection_ink(paint, super::colors::rgba(base)),
            hidden: false,
            blink: cell.attrs.contains(Attrs::BLINK),
        };
        // Cursor association uses the owned block key, not a guessed scalar span.
        prepare(
            builder,
            context,
            block,
            style,
            snapshot
                .cursor_block()
                .is_some_and(|cursor| cursor.key == block.key),
        )?;
    }
    Ok(())
}

/// Natural layout produces one source run before its rectangle fit is applied.
/// `at_cursor` isolates the entire run for the existing cursor recolour stream.
/// The source boundary is saved before shaping so adjacent ordinary ink is untouched.
/// The caller already resolved visibility and colours from the snapshot cell.
fn prepare(
    builder: &mut FrameBuilder,
    context: &mut TextContext,
    block: &Block,
    style: RunStyle,
    at_cursor: bool,
) -> Result<(), String> {
    // Preparation is distinct from row dispatch: one owner can paint several
    // fragments, but should shape and resolve its retained payload only once.
    #[cfg(test)]
    super::testing_work::charge(6, 1);
    // The owner row remains the sole cache of this rectangle's transformed ink.
    let cache = &mut builder.rows[block.row];
    // Natural requests are retained separately for later provider variant rebuilds.
    let first_source = cache.sources.len();
    // New ink can coexist with ordinary text already emitted earlier in this row.
    let first = [
        cache.glyphs.len(),
        cache.rasters.len(),
        cache.placements.len(),
    ];
    // Both loading phases retain natural sources before one sizing transform.
    let map = if context.fonts.is_some() {
        loaded(
            builder.metrics,
            builder.font_size,
            builder.dpr,
            context,
            cache,
            block,
            style,
        )?
    } else {
        // Pack outlines are imported and unhinted, so they can use the same fit.
        let width = pack::append(context, cache, &block.value.text, style, builder.metrics)?;
        placement::map(
            builder.metrics,
            block,
            width,
            builder.metrics.height_px,
            &cache.placements[first[2]..],
        )
    };
    // Raster effect metadata needs magnification in addition to a quad origin.
    let scale = ink::raster_scale(map, builder.font_size * builder.dpr)?;
    // Sparse maps keep ordinary Source records unchanged in size and work.
    cache.block_transforms.push(SourceMap {
        sources: first_source..cache.sources.len(),
        map,
        cells: [block.value.column, block.value.columns],
        lines: std::array::from_fn(|i| block.value.fragments.get(i).map(|f| f.line)),
    });
    // Apply one map to all layers, selection paints and neutral hit geometry.
    transform(cache, &mut context.backend, map, scale, first, block, style)?;
    if at_cursor {
        isolate_cursor(cache, first);
    }
    Ok(())
}

/// Full fonts share natural shaping, fallback and sized bitmap source selection.
/// `size` and `dpr` describe the context; sizing is an affine, never a font setting.
/// Returns one fit of the natural advance/ink union into the occupied rectangle.
/// The common emitter reports source preparation errors without partial publication.
#[allow(
    clippy::too_many_arguments,
    reason = "the sparse emitter borrows existing glyph owners and immutable geometry"
)]
fn loaded(
    metrics: puzzle_text::metrics::CellMetrics,
    size: f32,
    dpr: f32,
    context: &mut TextContext,
    cache: &mut super::CachedRow,
    block: &Block,
    style: RunStyle,
) -> Result<Affine, String> {
    // Font ownership stays on the render thread; the snapshot contains text only.
    let fonts = context.fonts.as_mut().ok_or("text sizing fonts missing")?;
    // Grid hinting and procedural cell geometry must not distort scaled natural text.
    let options = puzzle_text::layout::LayoutOptions {
        hinting: puzzle_text::hinting::HintingSetting::None,
        transformed: true,
        builtin_glyphs: false,
        symbol_overflow: false,
        ..super::row::grid_options(style, size, dpr, 0, true)
    };
    // A single natural line gives every fragment the same advances and fit.
    let output =
        context
            .free_layout
            .line(fonts, &mut context.shaper, &block.value.text, options, None)?;
    // The first fit supplies a source-size bound before bitmap fallback dispatch.
    let map = placement::map(metrics, block, output.width, output.metrics.height_px, &[]);
    // Use the largest affine magnification for a pixel-only source's request.
    let scale = ink::raster_scale(map, size * dpr)?;
    // Existing ordinary placements must not enlarge this block's ink union.
    let first = cache.placements.len();
    // Shared dispatch handles native outlines, COLR layers and bitmap-only sources.
    super::row::glyphs::append_glyphs(
        fonts,
        &mut context.backend,
        &mut context.raster,
        cache,
        &output.glyphs,
        style,
        fonts.needs_synthetic_italic(style.face),
        dpr,
        scale,
    )?;
    // Ink overhangs enlarge the same natural rectangle, never a fragment's fit.
    // The preliminary source size can only oversample this final smaller map.
    Ok(placement::map(
        metrics,
        block,
        output.width,
        output.metrics.height_px,
        &cache.placements[first..],
    ))
}

/// Transforms newly emitted ink and neutral hits together; prior row ink is unchanged.
/// `first` bounds vector, raster and geometry suffixes belonging to this block.
/// Selection paints receive the identical linear map used by ordinary bitmap ink.
/// Shared geometry then maps each internal glyph back to the indivisible block.
/// Cursor copies happen afterward so their origin and sampling match normal ink.
#[allow(
    clippy::too_many_arguments,
    reason = "one sparse block crosses existing row and glyph store owners"
)]
fn transform(
    cache: &mut super::CachedRow,
    backend: &mut puzzle_text::glyph::slug::SlugBackend,
    map: Affine,
    scale: f32,
    first: [usize; 3],
    block: &Block,
    style: RunStyle,
) -> Result<(), String> {
    // Slug transforms remain analytic; there is no enlarged grid bitmap here.
    for draw in &mut cache.glyphs[first[0]..] {
        ink::vector(draw, map);
    }
    // Pixel-only layers carry their source-size correction through the common helper.
    for index in first[1]..cache.rasters.len() {
        let draw = &mut cache.rasters[index];
        ink::bitmap(
            backend,
            draw,
            map,
            scale,
            super::colors::rgba(style.ink),
            false,
        )?;
        // Selection reuses the same forward map, with its own resolved tint.
        // Translation is already in the quad; the paint stores the linear inverse.
        let c = map.as_coeffs();
        cache.raster_selection_ink[index] = backend.prepare_raster_effect(
            [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32],
            cache.raster_selection_ink[index],
        )?;
    }
    // Neutral hit geometry describes this whole block, rather than internal text cells.
    for glyph in &mut cache.placements[first[2]..] {
        // A natural internal glyph maps to the whole indivisible physical footprint.
        glyph.cell.column = block.value.column;
        glyph.cells = block.value.columns;
        // Ink boxes and baselines must agree with the exact instances used for drawing.
        let origin = map
            * Point::new(
                f64::from(glyph.ink.origin[0]),
                f64::from(glyph.ink.origin[1]),
            );
        let baseline = map * Point::new(f64::from(glyph.baseline[0]), f64::from(glyph.baseline[1]));
        // This sizing map is uniform and axis-aligned, so box dimensions scale directly.
        glyph.ink.origin = [origin.x as f32, origin.y as f32];
        glyph.ink.size = glyph.ink.size.map(|v| v * map.as_coeffs()[0] as f32);
        glyph.baseline = [baseline.x as f32, baseline.y as f32];
    }
    Ok(())
}

/// Publishes sparse block extents beside ordinary physical cells for host picking.
/// The snapshot's visible order defines y; original fragment offsets affect ink only.
/// Extents are clipped once in device pixels and exposed in logical coordinates.
/// No table is allocated or scanned on a never-used ordinary screen.
pub(super) fn geometry(builder: &mut FrameBuilder, snapshot: &FrameSnapshot) {
    use puzzle_overlay::geometry::{BlockExtent, Cell, Range, Rect};
    // FrameBuilder stores device-pixel metrics; public geometry uses logical points.
    let dpr = builder.dpr;
    // Bounded visible records are already distinct; never walk cells to rediscover them.
    for block in snapshot.blocks() {
        // Count geometry records independently of glyph preparation so cached
        // ink cannot hide an accidental sparse walk on every ordinary frame.
        #[cfg(test)]
        super::testing_work::charge(7, 1);
        // Visible fragments may begin below the original root or end at the grid edge.
        let count = snapshot.block_rows(block);
        // Stable row IDs can be reordered by IL/DL; snapshot order defines geometry.
        let (Some(first), Some(last)) = (
            snapshot.row_meta(block.row),
            snapshot.row_meta(block.row + count.saturating_sub(1)),
        ) else {
            continue;
        };
        // Stable row IDs can be reordered by IL/DL; snapshot order defines geometry.
        let (Some(first), Some(last)) = (first.id, last.id) else {
            continue;
        };
        // Cell columns stay physical even when one source contains many scalars.
        let x =
            builder.grid_x() as f32 + block.value.column as f32 * builder.metrics.width_px as f32;
        // Fractional viewport motion uses the same row shift as ordinary assembly.
        let y = builder.padding as f32
            + block.row as f32 * builder.metrics.height_px as f32
            + builder.row_shift as f32;
        // The public extent describes only visible cells, never padding or hidden rows.
        let grid = builder.geometry.grid.device(dpr);
        // Intersect before converting to points to avoid inconsistent edge rounding.
        let lo = [x.max(grid.origin[0]), y.max(grid.origin[1])];
        let hi = [
            (x + block.value.columns as f32 * builder.metrics.width_px as f32)
                .min(grid.origin[0] + grid.size[0]),
            (y + count as f32 * builder.metrics.height_px as f32)
                .min(grid.origin[1] + grid.size[1]),
        ];
        // Picking returns both this block rectangle and the addressed physical cell.
        builder.geometry.blocks.push(BlockExtent {
            cells: Range {
                start: Cell {
                    line: first,
                    column: block.value.column,
                },
                end: Cell {
                    line: last,
                    column: block.value.column + block.value.columns - 1,
                },
            },
            rect: Rect {
                origin: lo.map(|v| v / dpr),
                size: [(hi[0] - lo[0]) / dpr, (hi[1] - lo[1]) / dpr],
            },
        });
    }
}

/// Reuses the transformed owner ink when a caret addresses any block fragment.
fn isolate_cursor(cache: &mut super::CachedRow, first: [usize; 3]) {
    // The physical caret can be on a lower fragment, but no duplicate ink lives there.
    cache
        .cursor_glyphs
        .extend_from_slice(&cache.glyphs[first[0]..]);
    cache
        .cursor_palette
        .extend_from_slice(&cache.glyph_palette[first[0]..]);
    cache
        .cursor_rasters
        .extend_from_slice(&cache.rasters[first[1]..]);
}
