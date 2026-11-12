//! Backend-neutral shaping and placement on terminal cells (design §8.4).
//!
//! The font chooses ink, while cell metrics choose the advance. A shaped
//! ligature is anchored to the first source cluster and may cover several
//! cells, but neither its font advance nor its hinter can move later cells.
//! Cursor editing splits the source into separate shaping runs so a coding
//! ligature cannot hide the individual character under the cursor.
//!
//! Coordinates have three units in this module. `size` and DPR combine
//! into device pixels per em. The font outline uses em coordinates and
//! Harfrust mark offsets also arrive in ems. Cell widths, baselines and
//! final glyph origins are integer device pixels. Conversion happens once
//! when a placement is produced, before a backend sees the glyph.
//!
//! The chosen face can change inside a line, but the cell dimensions
//! cannot: they always come from the primary style face. Otherwise a
//! fallback icon with a wider advance would shift every later terminal
//! column, breaking selection and cursor placement.
//!
//! The placement retains an ink limit separately from its advance. A Nerd
//! Font icon may paint into an explicitly blank neighbour, but the next
//! occupied column still begins at its original integer position. A bitmap
//! backend can apply the same limit even though it does not use Slug bounds.
use crate::{
    font_set::{FontSet, FontStyle},
    glyph::{GlyphKey, GlyphPlacement},
    hinting::{self, HintingSetting},
    metrics::{CellMetrics, CellOptions, MetricAdjustment},
    shape::{FeatureSetting, ShapingOptions, TextShaper},
};
mod clusters;
mod procedural;
use clusters::Cluster;

/// Counts source grid cells without loading a font or shaping a run.
///
/// The harness uses the same Unicode-width rule to bound its target before
/// GPU allocation; fonts may change ink but never this cell count (§8.4).
pub fn cell_count(text: &str) -> Result<u32, String> {
    clusters::cell_count(text)
}

/// One text request before glyph backend selection.
#[derive(Clone, Copy)]
pub struct LayoutOptions<'a> {
    /// Logical pixels per em.
    pub size: f32,
    /// Device pixels per logical pixel.
    pub dpr: f32,
    /// Top-left cell origin, already in device pixels.
    pub origin_px: [i32; 2],
    /// Primary face style used before fallback.
    pub style: FontStyle,
    /// User-controlled outline fitting policy.
    pub hinting: HintingSetting,
    /// Coding substitutions are enabled unless a cursor splits the run.
    pub ligatures: bool,
    /// Additional OpenType substitutions, applied only inside a face run.
    pub features: &'a [FeatureSetting],
    /// Optional UTF-8 byte offset where a cursor breaks a shaping run.
    pub cursor_byte: Option<usize>,
    /// Affine effects use original outlines even on a low-DPR grid.
    pub transformed: bool,
    /// A PUA symbol may occupy the following blank cell without advancing it.
    /// The source row decides this before shaping discards spaces.
    pub symbol_overflow: bool,
    /// Procedural joining glyphs take priority over approximate font glyphs.
    pub builtin_glyphs: bool,
    /// Straight RGBA8 color to apply after glyph preparation.
    pub color: u32,
}

/// Text output that remains unchanged when a backend is switched.
pub struct TextLayout {
    /// Integer geometry shared by glyphs, cursors and decorations.
    pub metrics: CellMetrics,
    /// Drawable glyphs in source order; spaces still advance the cell pen.
    pub glyphs: Vec<GlyphPlacement>,
    /// Number of terminal cells occupied by the source line.
    /// Ligature substitution never reduces this count.
    pub cells: u32,
}

/// Ordered boundaries of a backend-neutral line layout; no platform timer lives here.
#[derive(Clone, Copy)]
pub enum LayoutPhase {
    /// Primary metrics and hinting selection before fallback.
    Metrics,
    /// Grapheme segmentation, cell spans and whole-cluster fallback selection.
    Clusters,
    /// Harfrust run shaping and placement on integer grid coordinates.
    ShapePlacement,
}

/// Reusable grapheme and placement storage for changed terminal rows (P1).
#[derive(Default)]
pub struct RowLayout {
    /// Source byte and cell map kept across rows instead of reallocated.
    clusters: Vec<Cluster>,
    /// Backend-neutral output whose glyph capacity survives each redraw.
    output: Option<TextLayout>,
}

/// Layout of authoritative grid cells and ordinary Unicode strings.
mod rows;
pub use rows::GridCell;

/// Shared row geometry and outline policy for a sequence of shaping runs.
#[derive(Clone, Copy)]
struct RunContext<'a> {
    /// Integer cell measurements fixed before any fallback or hinting.
    pub(super) metrics: CellMetrics,
    /// Resolved ppem suffix shared by one style and device size.
    pub(super) hint: hinting::HintChoice,
    /// Caller settings retained while each face run is shaped.
    pub(super) options: LayoutOptions<'a>,
}

/// Shapes a complete line and places its glyphs on integer grid cells.
pub fn line(
    fonts: &mut FontSet,
    shaper: &mut TextShaper,
    text: &str,
    options: LayoutOptions<'_>,
) -> Result<TextLayout, String> {
    let mut row = RowLayout::default();
    row.line(fonts, shaper, text, options)?;
    row.output.ok_or("row output was not created".into())
}

/// Walks equal-face runs after source clusters and grid metrics are fixed.
fn place_line(
    fonts: &mut FontSet,
    shaper: &mut TextShaper,
    text: &str,
    clusters: &[Cluster],
    options: LayoutOptions<'_>,
    hint: hinting::HintChoice,
    output: &mut TextLayout,
) -> Result<(), String> {
    // Runs share the one coordinate system established above. No backend
    // data enters this stage, which is the D-28 placement invariant.
    // In particular, a fallback face cannot import its design advance.
    let context = RunContext {
        metrics: output.metrics,
        hint,
        options,
    };
    let mut start = 0;
    while start < clusters.len() {
        // Source and procedural clusters need no Harfrust call. Their
        // neutral geometry still uses this row's primary cell metrics.
        if let Some(placement) = source::special(fonts, &clusters[start], text, &context)? {
            output.glyphs.push(placement);
            start += 1;
            continue;
        }
        // A face change or cursor boundary terminates a substitution run.
        // The next run receives its own byte-relative cluster indices.
        // In particular, a cursor inside `->` splits the pair before
        // Harfrust can replace it with an arrow-shaped coding ligature.
        let mut end = start + 1;
        while end < clusters.len()
            && clusters[end].face == clusters[start].face
            && options.cursor_byte != Some(clusters[end].byte)
        {
            end += 1;
        }
        // Advancing by complete clusters ensures a combining mark never
        // begins a new run merely because its UTF-8 byte width differs.
        place_run(
            fonts,
            shaper,
            text,
            &clusters[start..end],
            &context,
            &mut output.glyphs,
        )?;
        start = end;
    }
    // Returned glyphs contain no Slug handle or bitmap atlas address.
    // The renderer may select a different backend without re-layout.
    Ok(())
}

/// Fixes one grid geometry before any fallback face or shaper is consulted.
fn row_metrics(
    fonts: &FontSet,
    options: LayoutOptions<'_>,
) -> Result<(CellMetrics, hinting::HintChoice), String> {
    // Fallback glyphs may have different advances, but the terminal grid
    // always inherits the primary face's measured zero advance.
    let primary = fonts
        .face(fonts.primary_face(options.style))
        .ok_or("primary font face is missing")?;
    // Hinting is a property of the grid transform. Its resolved ppem
    // becomes part of each outline key, not a change to cell positions.
    let hint = hinting::choose(
        options.hinting,
        options.size * options.dpr,
        options.dpr,
        options.transformed,
    )?;
    // A hinted outline's blue zones already align horizontal strokes;
    // cap snapping would choose a second, unrelated device size.
    let metrics = CellMetrics::from_face(
        primary,
        CellOptions {
            size: options.size,
            dpr: options.dpr,
            line_height: 1.0,
            width: MetricAdjustment::Px(0.0),
            height: MetricAdjustment::Px(0.0),
            baseline: MetricAdjustment::Px(0.0),
            snap_cap_height: true,
            hinted: hint.ppem != 0,
        },
    )?;
    Ok((metrics, hint))
}

/// Converts one equal-face run from byte clusters to grid coordinates.
fn place_run(
    fonts: &FontSet,
    shaper: &mut TextShaper,
    text: &str,
    clusters: &[Cluster],
    context: &RunContext<'_>,
    output: &mut Vec<GlyphPlacement>,
) -> Result<(), String> {
    let options = context.options;
    // Run slices start and end at grapheme boundaries. Harfrust may join
    // adjacent clusters within that slice, but not cross a face change.
    let first = clusters.first().ok_or("empty shaping run")?;
    let last = clusters.last().ok_or("empty shaping run")?;
    let run = &text[first.byte..last.end];
    // The face is immutable during shaping. Its hint-engine state is used
    // only later when the vector backend asks for a specific outline.
    let face = fonts.face(first.face).ok_or("shaping face is missing")?;
    let shaped = shaper.shape(
        first.face,
        face,
        run,
        ShapingOptions {
            ligatures: options.ligatures,
            features: options.features,
        },
    )?;
    for glyph in shaped {
        // Harfrust offsets are relative to the run slice; restore the
        // original string offset before locating the owning grid cell.
        let byte = first
            .byte
            .checked_add(glyph.cluster)
            .ok_or("shaper cluster overflow")?;
        // A ligature reports the beginning of its source cluster. Looking
        // backward finds its first occupied cell even when source bytes
        // have unequal UTF-8 lengths.
        // Harfrust reports a byte offset. Binary search avoids scanning
        // every preceding cell for every glyph on a wide terminal row.
        let owner = clusters.partition_point(|cluster| cluster.byte <= byte);
        let source = owner
            .checked_sub(1)
            .and_then(|index| clusters.get(index))
            .ok_or("shaper cluster before run")?;
        // Spaces can carry a glyph ID but no visible outline. Their cells
        // remain in the run, preserving the next glyph's fixed origin.
        if crate::font_set::is_blank_cluster(&text[source.byte..source.end]) {
            continue;
        }
        // Grid geometry is created only after substitutions are complete;
        // shaper advances have no opportunity to move later glyphs.
        output.push(make_placement(first.face, *glyph, source, context)?);
    }
    Ok(())
}

/// Anchors one shaped glyph to its source cell without using font advances.
fn make_placement(
    face: u32,
    glyph: crate::shape::ShapedGlyph,
    source: &Cluster,
    context: &RunContext<'_>,
) -> Result<GlyphPlacement, String> {
    let RunContext {
        metrics,
        hint,
        options,
    } = *context;
    let (origin, advance, ink_limit) = grid_geometry(source, glyph, context)?;
    // `instance` names the selected variation slot, while `hint_ppem`
    // distinguishes a fitted outline from the reusable em-space shape.
    Ok(GlyphPlacement {
        cell_column: source.cell + (options.origin_px[0].max(0) as u32 / metrics.width_px.max(1)),
        key: GlyphKey {
            face,
            glyph_id: glyph.glyph_id,
            instance: face,
            hint_ppem: hint.ppem,
            hint_mode: hint.mode,
        },
        // The raster or vector backend can fit ink inside `ink_limit_px`
        // without altering the source cluster's cell advance.
        origin_px: origin,
        baseline_px: metrics.baseline_px as f32,
        size_px: metrics.device_size,
        advance_px: advance,
        ink_limit_px: ink_limit,
        cell_height_px: metrics.height_px,
        cell_span: u8::try_from(source.span).map_err(|_| "cluster exceeds 255 cells")?,
        color: options.color,
        effect: options.transformed,
        color_glyph: false,
    })
}

/// Converts source cells and Harfrust offsets to bounded device pixels.
fn grid_geometry(
    source: &Cluster,
    glyph: crate::shape::ShapedGlyph,
    context: &RunContext<'_>,
) -> Result<([i32; 2], u32, u32), String> {
    let metrics = context.metrics;
    let options = context.options;
    // Checked arithmetic prevents a large malicious text request from
    // wrapping around to an unrelated place in the offscreen target.
    let cell_x = source
        .cell
        .checked_mul(metrics.width_px)
        .ok_or("text x coordinate overflow")?;
    let x = options.origin_px[0]
        .checked_add(i32::try_from(cell_x).map_err(|_| "text x exceeds device range")?)
        .ok_or("text x exceeds device range")?;
    let y = options.origin_px[1]
        .checked_add(metrics.baseline_px)
        .ok_or("text baseline overflow")?;
    // Marks retain their Harfrust offsets relative to the cell origin.
    // The base advance remains the Unicode cell span, not `x_advance`.
    // A zero-width mark still gets one cell of ink room for attachment;
    // its zero advance leaves the following base at the same column.
    let advance = metrics
        .width_px
        .checked_mul(source.span)
        .ok_or("text advance overflow")?;
    let ink_limit = metrics
        .width_px
        .checked_mul(source.span.max(1) + u32::from(source.overflow))
        .ok_or("text ink span overflow")?;
    // The origin is rounded once, before either backend receives it;
    // backend-specific outlines never move the text grid (D-28).
    Ok((
        [
            x + (glyph.x_offset_em * metrics.device_size).round() as i32,
            y - (glyph.y_offset_em * metrics.device_size).round() as i32,
        ],
        advance,
        ink_limit,
    ))
}

#[cfg(test)]
mod tests;

/// Neutral placement of complete emoji or platform requests.
mod source;

/// Natural advances for annotations share faces and fallback with grid rows.
pub mod free;
