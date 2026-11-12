//! Styled cell copy reuses the existing serializer’s rendition helpers (§6.9).
//!
//! Clipboard snippets contain selected cells in reading order, without the
//! persistence serializer’s cursor/mode restoration. Soft wraps join, hard
//! breaks remain, wide spacers disappear and grapheme clusters stay whole.
//! Strings allocate only for explicit copy, never during frame assembly.
//! The HTML run writer spans rows so soft-wrapped text keeps contiguous style
//! spans. Hard boundaries flush that writer before adding an unstyled newline.
//! Plain, HTML and VT share half-open physical selection limits; the optional
//! rectangular-text path changes logical ownership, not those gesture limits.
//! Reusing the existing SGR encoder keeps both exporters in agreement about
//! underline colour, tagged palette colours and the full attribute set.
// Both rich-copy paths use the same range policy and rendition helpers.
use super::*;

/// Half-open retained span with the selection’s extraction preferences.
#[derive(Clone, Copy, Debug)]
pub struct CellRange {
    /// First row and inclusive column boundary.
    pub start: (LineId, usize),
    /// Last row and exclusive column boundary.
    pub end: (LineId, usize),
    /// Constant columns and a newline per physical row.
    pub block: bool,
    /// Trim spaces at hard ends, matching plain copy.
    pub trim_trailing: bool,
    /// Completed shell output can end beyond the final painted cell.
    pub final_newline: bool,
}

/// Exports selected snippets without altering whole-screen serialization.
impl BufferView<'_> {
    /// Copies selected cells as plain text, styled VT text or escaped HTML.
    pub fn serialize_cells(self, range: CellRange, format: Format) -> String {
        // Plain extraction remains the Unicode engine’s source of truth.
        // Rich paths share its boundaries but must retain each cell’s rendition.
        if format == Format::PlainText {
            return plain(self, range);
        }
        if self.term.screen().ring.multicells.is_some() {
            return super::multicell::selection::rich(self, range, format);
        }
        // HTML owns one preformatted container; VT starts in default rendition.
        let mut output = if format == Format::Html {
            "<pre>".into()
        } else {
            String::new()
        };
        // The first selected row has no preceding boundary in this snippet.
        // After that, only a hard predecessor inserts an actual newline.
        let mut previous_wrapped = true;
        // One writer spans the entire selection, including joined soft wraps.
        // The same writer serves full-buffer HTML so CSS/escaping cannot drift.
        let mut runs = html::Runs::default();
        for line in self.lines(range.start.0..=range.end.0) {
            // The predecessor owns the hard break, including trimmed empty rows.
            if !previous_wrapped {
                runs.finish(&mut output);
                output.push('\n');
            }
            // Partial outer rows exclude cells that were never selected.
            // Intermediate simple rows cover the full retained physical width.
            let (first, end) = columns(line, range);
            write_row(self, line, first..end, format, &mut output, &mut runs);
            // A rectangular copy never joins physical rows through soft wrapping.
            previous_wrapped = !range.block && line.wrapped();
        }
        // Close before the trailing newline, keeping hard separators unstyled.
        // finish is inert for VT because no HTML span was opened.
        runs.finish(&mut output);
        if range.block || range.final_newline {
            output.push('\n');
        }
        // Close markup or reset rendition so clipboard snippets do not leak state.
        output.push_str(if format == Format::Html {
            "</pre>"
        } else {
            "\x1b[0m"
        });
        output
    }
}

/// Shares established Unicode and line-boundary rules with ordinary contents copy.
fn plain(view: BufferView<'_>, range: CellRange) -> String {
    // Rectangles use the identical column slice on every physical row.
    // Their rows each terminate even if the original document soft-wrapped.
    let mut text = if range.block {
        view.text(
            range.start.0..=range.end.0,
            TextOptions {
                block: true,
                trim_trailing: range.trim_trailing,
                first_column: range.start.1,
                end_column: Some(range.end.1),
                ..TextOptions::default()
            },
        )
    } else {
        view.text_span_options(range.start, range.end, range.trim_trailing)
    };
    // This extra boundary belongs to OSC D, outside the inclusive paint interval.
    if range.final_newline {
        text.push('\n');
    }
    text
}

/// Computes partial outer rows or constant block columns before hard-end trimming.
fn columns(line: LineView<'_>, range: CellRange) -> (usize, usize) {
    // A linear range starts later only on its first physical row.
    // A block keeps that first column throughout the rectangle.
    let first = if range.block || line.id() == range.start.0 {
        range.start.1
    } else {
        0
    };
    // The half-open end excludes the next cursor cell in semantic output.
    // Full middle rows remain available for untrimmed soft continuity.
    let mut end = if range.block || line.id() == range.end.0 {
        range.end.1
    } else {
        line.cells().len()
    };
    // Public callers can supply oversized columns; clamp before indexing.
    end = end.min(line.cells().len());
    if range.trim_trailing && (range.block || !line.wrapped()) {
        // Only spaces and untouched blanks trim; a combining cluster remains whole.
        while end > first
            && line
                .cell(end - 1)
                .is_some_and(|cell| matches!(cell.text, CellText::Empty | CellText::Scalar(' ')))
        {
            end -= 1;
        }
    }
    (first.min(end), end)
}

/// Writes one ordinary selected row through the shared style and HTML writers.
fn write_row(
    view: BufferView<'_>,
    line: LineView<'_>,
    columns: std::ops::Range<usize>,
    format: Format,
    output: &mut String,
    runs: &mut html::Runs,
) {
    for column in columns {
        // Wide spacers have no independent Unicode content or style.
        let Some(cell) = line.cell(column) else {
            continue;
        };
        if cell.width == 0 {
            continue;
        }
        if format == Format::Vt {
            // Existing SGR preserves underline color and the full attribute set.
            output.push_str(&sgr(cell.raw, cell.underline_color));
            match cell.text {
                CellText::Empty => output.push(' '),
                text => push_text(output, text),
            }
        } else {
            runs.cell(output, cell, view.term);
        }
    }
}
