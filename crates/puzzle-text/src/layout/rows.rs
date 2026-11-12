//! Reusable layouts for free strings and already segmented terminal cells.
//! VT clusters also include zero-width attachments in legacy mode. Joining the
//! row into a string and segmenting again can detach those attachments and move
//! them beyond the grid. Grid callers supply authoritative byte/cell ownership;
//! free text continues to use Unicode segmentation and Unicode widths.
//! A cell's bytes need not be one UAX grapheme: legacy attachments, cursor
//! writes and margin handling preserve the terminal's historical cell unit.
//! Font fallback validates that complete stored unit before any shape is built.
//! Byte offsets locate Harfrust output; columns locate the resulting ink.
//! The two maps are deliberately independent of the font's natural advances.
//! A wide spacer is absent from this input, because its leading cell owns it.
//! Explicit blank cells remain present so gaps and PUA overflow stay correct.
//! Source records and output vectors retain their capacity between dirty rows.
//! Free annotations call the string path and keep their existing segmentation.
//! Neither path owns a clock: observers expose boundaries to the host only.
//! Invalid external maps are rejected before the shared shaper slices a run.
use super::*;

/// One caller-owned cluster with its physical grid ownership, before shaping.
#[derive(Clone, Copy)]
pub struct GridCell {
    /// Inclusive UTF-8 offset within the homogeneous source run.
    pub byte: usize,
    /// Exclusive UTF-8 offset; checked before borrowing source text.
    pub end: usize,
    /// Physical column relative to the beginning of this run.
    pub column: u32,
    /// Occupied grid columns, assigned by the terminal rather than the font.
    pub span: u8,
}

/// Gives a caller ownership of row scratch storage across frame builds.
impl RowLayout {
    /// Rebuilds one row while retaining both source and placement buffers.
    pub fn line<'a>(
        &'a mut self,
        fonts: &mut FontSet,
        shaper: &mut TextShaper,
        text: &str,
        options: LayoutOptions<'_>,
    ) -> Result<&'a TextLayout, String> {
        // The empty generic observer eliminates diagnostic work in ordinary text callers.
        self.line_observed(fonts, shaper, text, options, |_| {})
    }

    /// Lays out stored terminal cells without re-segmenting their source string.
    /// Invalid UTF-8 boundaries or zero spans are rejected without indexing panic.
    #[allow(
        clippy::too_many_arguments,
        reason = "grid ownership accompanies the ordinary text layout request"
    )]
    pub fn grid_line<'a>(
        &'a mut self,
        fonts: &mut FontSet,
        shaper: &mut TextShaper,
        text: &str,
        cells: &[GridCell],
        options: LayoutOptions<'_>,
        mut observed: impl FnMut(LayoutPhase),
    ) -> Result<&'a TextLayout, String> {
        let (metrics, hint) = row_metrics(fonts, options)?;
        observed(LayoutPhase::Metrics);
        // Clear only length: the same source map serves every changed row (P1).
        self.clusters.clear();
        let mut previous = (0, 0);
        for cell in cells {
            // Equal-face runs slice from first byte to last end. Unsorted records
            // would make that slice invalid even if each record were valid alone.
            let end_column = cell
                .column
                .checked_add(u32::from(cell.span))
                .ok_or("grid column overflow")?;
            if cell.byte < previous.0 || cell.column < previous.1 {
                return Err("overlapping grid clusters".into());
            }
            previous = (cell.end, end_column);
            // str::get checks both bounds and UTF-8 scalar boundaries (R8).
            let cluster = text
                .get(cell.byte..cell.end)
                .ok_or("invalid grid cluster bytes")?;
            // Stored cells always occupy columns; attachments share their base cell.
            if cell.span == 0 {
                return Err("zero-span grid cell".into());
            }
            // Blank property-only cells retain their span. Face selection returns
            // a loaded face without fallback, and placement emits no ink.
            // A cluster cannot borrow a face covering only its first visible scalar.
            let face =
                clusters::choose_face(fonts, cluster, options.style, options.builtin_glyphs, None)?;
            // Preserve VT ownership even when a trailing Prepend scalar would
            // form a separate grapheme in a newly concatenated string.
            self.clusters.push(Cluster {
                byte: cell.byte,
                end: cell.end,
                face,
                cell: cell.column,
                span: u32::from(cell.span),
                overflow: false,
            });
        }
        // Overflow only changes safe ink room; the supplied columns never move.
        clusters::overflow(text, options.symbol_overflow, &mut self.clusters);
        observed(LayoutPhase::Clusters);
        self.finish_line(fonts, shaper, text, options, metrics, hint, observed)
    }

    /// Reports layout boundaries to a generic diagnostic observer, without clocks.
    /// Ordinary callers use an empty closure which compiles away (R9, P1).
    pub fn line_observed<'a>(
        &'a mut self,
        fonts: &mut FontSet,
        shaper: &mut TextShaper,
        text: &str,
        options: LayoutOptions<'_>,
        mut observed: impl FnMut(LayoutPhase),
    ) -> Result<&'a TextLayout, String> {
        let (metrics, hint) = row_metrics(fonts, options)?;
        observed(LayoutPhase::Metrics);
        // Fallback must see whole graphemes, including attached marks.
        clusters::collect_into(
            fonts,
            text,
            options.style,
            options.symbol_overflow,
            options.builtin_glyphs,
            &mut self.clusters,
            None,
        )?;
        observed(LayoutPhase::Clusters);
        self.finish_line(fonts, shaper, text, options, metrics, hint, observed)
    }
    /// Reuses placement storage after either source collector establishes ownership.
    #[allow(
        clippy::too_many_arguments,
        reason = "shared layout completion retains resolved metrics and hint policy"
    )]
    fn finish_line<'a>(
        &'a mut self,
        fonts: &mut FontSet,
        shaper: &mut TextShaper,
        text: &str,
        options: LayoutOptions<'_>,
        metrics: CellMetrics,
        hint: hinting::HintChoice,
        mut observed: impl FnMut(LayoutPhase),
    ) -> Result<&'a TextLayout, String> {
        // Create output once; following rows recycle its glyph allocation.
        let output = self.output.get_or_insert_with(|| TextLayout {
            metrics,
            glyphs: Vec::new(),
            cells: 0,
        });
        // Font/DPR changes update geometry while retained storage remains reusable.
        output.metrics = metrics;
        // The terminal map is validated above; free-string spans were checked on collection.
        output.cells = self.clusters.last().map_or(0, |last| last.cell + last.span);
        // No row-relative glyph may survive from the preceding layout request.
        output.glyphs.clear();
        // Both collectors now feed exactly the same backend-neutral shaping seam.
        place_line(fonts, shaper, text, &self.clusters, options, hint, output)?;
        // The caller can now attribute preparation separately from text layout.
        observed(LayoutPhase::ShapePlacement);
        Ok(output)
    }
}
