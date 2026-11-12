//! Rebuilds only VT rows whose identity, content or style changed (§10.4).
//!
//! Text is split where typeface or resolved ink color changes. Each run
//! retains its source column, including preceding blanks and wide-cell
//! spacers, so Harfrust substitutions cannot move later terminal cells.
//! Slug handles are cached after shaping; the renderer sees only neutral
//! placements and never reads a font or the VT screen itself (D-28).
//!
//! The parser stores one cell for each terminal column, including a second
//! placeholder cell after wide text. The text shaper instead needs the
//! visible Unicode string. A wide placeholder contributes no source
//! character because the preceding scalar already has a two-cell span.
//! Unwritten cells do contribute a space when they are inside a run: this
//! preserves a later glyph's column after a gap in the row.
//!
//! SGR style changes delimit shaping runs. A coding ligature cannot cross
//! colors or typefaces because one resulting outline would have no single
//! correct color. The run's starting column is retained separately from
//! its source bytes; Harfrust may join several scalars into one glyph,
//! but the terminal's column grid remains authoritative for placement.
//!
//! Resolved backgrounds are gathered even from wide placeholders and
//! hidden glyphs. A BCE erase can leave colored blank cells, and inverse
//! text can produce a background while the source character is invisible.
//! Equal adjacent backgrounds merge into one rect, reducing GPU instance
//! count for full-screen applications with uniform status-bar colors.
//!
//! Decoration positions use integer device-cell geometry. A line is
//! attached to the cell span rather than to a font glyph, so spaces can be
//! underlined and adjoining box cells can retain continuous decorations.
//! The cache stores every rectangle at relative y=0; the frame assembler
//! applies the row's current screen y coordinate after scrolling.
//!
//! A parallel byte/cell map preserves the VT's cluster ownership, including
//! zero-width attachments that UAX segmentation alone would detach at wrap.
//! Diagnostic phase clocks are optional; ordinary builds retain only reusable
//! text/map buffers and perform no per-run measurement allocation.
use super::{CachedRow, FrameBuilder, colors, context::TextContext};
use crate::frame_snapshot::PaintState;
use puzzle_text::{
    font_set::{FontSet, FontStyle},
    glyph::slug::SlugBackend,
    hinting::HintingSetting,
    layout::{LayoutOptions, RowLayout},
    shape::TextShaper,
};
use puzzle_vt::{cell::Attrs, row::Row, view::CellText};
mod cell;
/// Row summaries select a placeholder-free monomorphized cell walk.
mod walk;
#[cfg(test)]
pub(super) use cell::{decoration_cells, placeholder_checks};
/// Ordered COLR and ordinary vector ink share neutral placements.
pub(super) mod glyphs;
/// Pack-only placement uses the same glyph backend before TTF takeover.
mod pack;
mod paint;
mod wave;
use glyphs::append_glyphs;

/// Appearance facts that a shaped run must share throughout its cells.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct RunStyle {
    /// Real bold and italic font selection precedes shaping.
    pub(super) face: FontStyle,
    /// Resolved ink color after palette and inverse policy.
    pub(super) ink: [u8; 3],
    /// Selection contrast is resolved against selection paper, before caching.
    pub(super) selection_ink: u32,
    /// SGR 8 retains copyable text but creates no visible glyph.
    pub(super) hidden: bool,
    /// SGR 5 toggles only ink on the shared blink timer.
    pub(super) blink: bool,
}

/// Mutable owners crossed while shaping a single colored terminal row.
struct RowShape<'a> {
    /// Font fallback and outline source for this session.
    fonts: &'a mut Option<FontSet>,
    /// Validated startup cmap, absent in native contexts.
    pack: Option<&'a puzzle_text::pack::FontPack>,
    /// Primary baseline and em size shared by both loading phases.
    metrics: puzzle_text::metrics::CellMetrics,
    /// Reused Harfrust plan and run cache.
    shaper: &'a mut TextShaper,
    /// Atlas entries prepared only when a glyph key is first encountered.
    backend: &'a mut SlugBackend,
    /// Shared shelf store for browser/native bitmap samples.
    raster: &'a mut puzzle_text::raster::RasterAtlas,
    /// Backend-neutral placement scratch across appearance runs.
    layout: &'a mut RowLayout,
    /// Relative Slug instances retained after the current rebuild.
    cache: &'a mut CachedRow,
    /// Source bytes from adjacent cells with equal appearance.
    text: &'a mut String,
    /// Stored VT ownership accompanies source bytes without re-segmentation.
    cells: &'a mut Vec<puzzle_text::layout::GridCell>,
    /// Optional stage scalars supplied only by an explicit host row probe.
    row_metrics: &'a mut Option<[u64; 4]>,
    /// First column and style of the currently open run.
    active: Option<(usize, RunStyle)>,
    /// Logical font size and device scale for D-27 outline selection.
    size: f32,
    /// Physical pixels per logical pixel.
    dpr: f32,
    /// Integer primary-face column advance.
    cell_width: u32,
    /// Cursor cell isolated so Harfrust cannot hide it in a ligature.
    cursor_column: Option<usize>,
}

/// Replaces cached relative geometry for one changed visible row.
pub(super) fn rebuild(
    builder: &mut FrameBuilder,
    context: &mut TextContext,
    line: &Row,
    paint: &PaintState,
    index: usize,
    cursor_column: Option<usize>,
) -> Result<(), String> {
    // The current grid position is transient, while `line.id` follows
    // content into scrollback. The buffer view validates that identity
    // before exposing sparse cluster and underline metadata (§6.13).
    let cache = &mut builder.rows[index];
    // Clearing retains each backing allocation. A changed row with a
    // familiar glyph set normally needs no new heap storage (P1).
    cache.placements.clear();
    cache.sources.clear();
    cache.block_transforms.clear();
    cache.rasters.clear();
    cache.raster_blink.clear();
    cache.raster_selection_ink.clear();
    cache.cursor_rasters.clear();
    cache.backgrounds.clear();
    cache.glyphs.clear();
    cache.glyph_blink.clear();
    cache.glyph_palette.clear();
    cache.glyph_selection_ink.clear();
    cache.wave_selection_ink.clear();
    cache.waves.clear();
    cache.cursor_glyphs.clear();
    cache.cursor_palette.clear();
    cache.decorations.clear();
    let atlas_generation = context.atlas_generation();
    context.run_text.clear();
    context.run_cells.clear();
    // These mutable owners are borrowed together for the duration of one
    // row. `RowShape` avoids rebuilding the backend or Harfrust cache on
    // each SGR boundary and keeps the source String reusable.
    let mut shape = RowShape {
        fonts: &mut context.fonts,
        pack: context.pack.as_ref(),
        metrics: builder.metrics,
        shaper: &mut context.shaper,
        backend: &mut context.backend,
        raster: &mut context.raster,
        layout: &mut context.layout,
        cache,
        text: &mut context.run_text,
        cells: &mut context.run_cells,
        active: None,
        row_metrics: &mut context.row_metrics,
        size: builder.font_size,
        dpr: builder.dpr,
        cell_width: builder.metrics.width_px,
        cursor_column,
    };
    // The row summary is maintained by VT stores/reflow. Ordinary rows never
    // inspect scalar or cluster strings for kitty image metadata (M15.7/P11).
    if line
        .flags
        .contains(puzzle_vt::row::RowFlags::IMAGE_PLACEHOLDERS)
    {
        walk::paint::<true>(&mut shape, line, paint, builder.metrics, &builder.colors)?;
    } else {
        walk::paint::<false>(&mut shape, line, paint, builder.metrics, &builder.colors)?;
    }
    // The final run has no following SGR boundary to flush it, so close
    // it explicitly before publishing the cache's new version key.
    shape.finish()?;
    shape.cache.id = Some(line.id);
    shape.cache.version = line.version;
    shape.cache.style_epoch = builder.style_epoch;
    shape.cache.atlas_generation = atlas_generation;
    shape.cache.cursor_column = shape.cursor_column;
    Ok(())
}

impl RowShape<'_> {
    /// Extends a compatible run or ends it before a new SGR appearance.
    fn visit(
        &mut self,
        column: usize,
        style: RunStyle,
        text: CellText<'_>,
        span: u8,
    ) -> Result<(), String> {
        if self.fonts.is_none() {
            return pack::visit(self, column, style, text);
        }
        // A cursor cell forms its own run, so an editing caret cannot sit
        // inside an `->` ligature selected by its neighbours (§8.4).
        if self.cursor_column == Some(column) {
            self.finish()?;
        }
        // A color or face change ends Harfrust's substitution context;
        // keeping it open could tint one ligature with two SGR colors.
        if self.active.is_some_and(|(_, old)| old != style) {
            self.finish()?;
        }
        // The first column is physical grid state, not the count of glyphs
        // emitted by any previous run on this same row.
        if self.active.is_none() {
            self.active = Some((column, style));
        }
        // A leading unwritten column adds a space so the next source
        // cluster retains its offset inside this run.
        let byte = self.text.len();
        append_text(self.text, text);
        // Source byte offsets and physical columns survive Harfrust substitutions.
        self.cells.push(puzzle_text::layout::GridCell {
            byte,
            end: self.text.len(),
            column: (column - self.active.map_or(column, |(start, _)| start)) as u32,
            span,
        });
        if self.cursor_column == Some(column) {
            self.finish()?;
        }
        Ok(())
    }

    /// Passes a complete homogeneous run through the D-28 text seam.
    fn finish(&mut self) -> Result<(), String> {
        if let Some((start, style)) = self.active.take() {
            // The neutral layout owns advances; Slug shape preparation
            // below may change ink bounds but cannot shift later cells.
            flush_run(
                self.fonts.as_mut().ok_or("full font missing")?,
                self.shaper,
                self.backend,
                self.raster,
                self.layout,
                self.text,
                self.cells,
                self.cache,
                start,
                style,
                self.size,
                self.dpr,
                self.cell_width,
                self.cursor_column,
                self.row_metrics,
            )?;
            // Keep the capacity for a following run or changed row.
            self.text.clear();
            self.cells.clear();
        }
        Ok(())
    }
}

/// Writes a stored scalar or cluster while respecting wide placeholders.
fn append_text(output: &mut String, text: CellText<'_>) {
    // The parser already joined combining marks into `Cluster`. Keeping
    // its borrowed string intact lets Harfrust position the mark correctly.
    match text {
        CellText::Scalar(ch) => output.push(ch),
        CellText::Cluster(cluster) => output.push_str(cluster),
        // An unwritten grid column still advances later text in a run.
        CellText::Empty => output.push(' '),
    }
}

/// Shapes one homogeneous run and prepares each nonempty vector outline.
#[allow(
    clippy::too_many_arguments,
    reason = "a run crosses the font, shape and cache owners without a trait object"
)]
fn flush_run(
    fonts: &mut FontSet,
    shaper: &mut TextShaper,
    backend: &mut SlugBackend,
    raster: &mut puzzle_text::raster::RasterAtlas,
    layout: &mut RowLayout,
    text: &str,
    cells: &[puzzle_text::layout::GridCell],
    cache: &mut CachedRow,
    column: usize,
    style: RunStyle,
    size: f32,
    dpr: f32,
    cell_width: u32,
    cursor_column: Option<usize>,
    row_metrics: &mut Option<[u64; 4]>,
) -> Result<(), String> {
    // A hidden or whitespace-only run contributes no Slug instance, while
    // its background and decorations were already placed above.
    if style.hidden || text.trim().is_empty() {
        return Ok(());
    }
    let x = column_x(column, cell_width)?;
    // A synthetic italic face is a shader transform, so §8.5 forbids a
    // ppem-hinted outline here even when the display DPR is below 1.5.
    let transformed = fonts.needs_synthetic_italic(style.face);
    // Layout remains backend-neutral. The run's origin is the VT column,
    // and shaping ignores font advances in favor of fixed grid advances.
    // Ordinary row builds have no timer; the generic empty observer stays inert.
    let mut started = row_metrics.is_some().then(web_time::Instant::now);
    let output = layout.grid_line(
        fonts,
        shaper,
        text,
        cells,
        grid_options(style, size, dpr, x, transformed),
        |phase| record_row_phase(row_metrics, &mut started, phase as usize),
    )?;
    let first = cache.glyphs.len();
    let first_raster = cache.rasters.len();
    append_glyphs(
        fonts,
        backend,
        raster,
        cache,
        &output.glyphs,
        style,
        transformed,
        dpr,
        1.0,
    )?;
    record_row_phase(row_metrics, &mut started, 3);
    if cursor_column == Some(column) {
        cache
            .cursor_rasters
            .extend_from_slice(&cache.rasters[first_raster..]);
    }
    retain_cursor_ink(cache, first, cursor_column == Some(column));
    Ok(())
}

/// Records a disjoint row stage only when the host explicitly enabled observation.
fn record_row_phase(
    metrics: &mut Option<[u64; 4]>,
    started: &mut Option<web_time::Instant>,
    phase: usize,
) {
    if let (Some(metrics), Some(started)) = (metrics, started) {
        let now = web_time::Instant::now();
        // Formatting and report allocation are deferred until after the workload.
        metrics[phase] += now.duration_since(*started).as_nanos() as u64;
        *started = now;
    }
}

/// Applies grid-only shaping policy while keeping Slug selection downstream.
pub(super) fn grid_options(
    style: RunStyle,
    size: f32,
    dpr: f32,
    x: i32,
    transformed: bool,
) -> LayoutOptions<'static> {
    // D-27 chooses light auto-hinting only for untransformed 1× text;
    // the backend-neutral layout resolves this into the glyph cache key.
    LayoutOptions {
        size,
        dpr,
        origin_px: [x, 0],
        style: style.face,
        hinting: HintingSetting::Auto,
        ligatures: true,
        features: &[],
        cursor_byte: None,
        transformed,
        symbol_overflow: true,
        builtin_glyphs: true,
        // Color follows the placement rather than the prepared outline,
        // so one shape may serve differently colored terminal cells.
        color: colors::rgba(style.ink),
    }
}

/// Retains only the isolated cursor cell for block-cursor recoloring.
fn retain_cursor_ink(cache: &mut CachedRow, first: usize, at_cursor: bool) {
    if at_cursor {
        // The selected Slug instances share prepared atlas data with the
        // text slot; the cursor slot changes only their colors and offsets.
        cache
            .cursor_palette
            .extend_from_slice(&cache.glyph_palette[first..]);
        cache
            .cursor_glyphs
            .extend_from_slice(&cache.glyphs[first..]);
    }
}

/// Rejects grid sizes whose pixel column cannot fit a placement origin.
fn column_x(column: usize, cell_width: u32) -> Result<i32, String> {
    // Checked multiplication prevents a hostile resize from wrapping a
    // right-edge glyph back to the left side of the surface (R8).
    i32::try_from(
        column
            .checked_mul(cell_width as usize)
            .ok_or("run x overflow")?,
    )
    .map_err(|_| "run x exceeds device range".into())
}

/// Maps the two VT style bits to the four primary face locations.
pub(super) fn font_style(attrs: Attrs) -> FontStyle {
    // The source cell's two independent SGR flags select one of the four
    // primary style slots before fallback and shaping are considered.
    match (attrs.contains(Attrs::BOLD), attrs.contains(Attrs::ITALIC)) {
        (false, false) => FontStyle::Regular,
        (true, false) => FontStyle::Bold,
        (false, true) => FontStyle::Italic,
        (true, true) => FontStyle::BoldItalic,
    }
}
