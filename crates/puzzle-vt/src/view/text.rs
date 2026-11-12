//! Copyable text rules shared by selection and external contents reads.
//!
//! A `WRAPPED` row is physically separate but logically continuous with
//! its successor. Joining those rows without a newline is essential for
//! copying a long shell command and for literal search across a wrap.
//! A block selection deliberately keeps row boundaries and fixed columns.
//!
//! The `WRAPPED` flag belongs to the row that continues, not the next
//! row. This is why newline insertion examines the previous row. Logical
//! line lookup walks the borrowed ring directly: creating a view and
//! locating a line must not copy every retained row just to return IDs.
use super::*;

/// Rectangular payload extraction is isolated from ordinary cell copies.
mod multicell;

/// Controls what a text extraction does at row boundaries.
///
/// Block mode takes precedence over joining because a rectangular copy
/// must preserve one physical slice per visual row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextOptions {
    /// Join soft-wrapped physical rows into logical lines.
    pub join_soft_wraps: bool,
    /// Remove trailing default blank columns from each emitted line.
    ///
    /// Background-colored blanks are visible content and need separate
    /// handling by callers that preserve styling.
    pub trim_trailing: bool,
    /// Keep every physical row as one block-selection output row.
    pub block: bool,
    /// First column of a rectangular extraction, inclusive.
    ///
    /// A non-block selection can also use it for a partial first row.
    pub first_column: usize,
    /// Last column of a rectangular extraction, exclusive.
    ///
    /// `None` means the row's current full width, including blank cells.
    pub end_column: Option<usize>,
}

impl Default for TextOptions {
    /// Normal copy joins soft wraps and trims visual padding (§6.9).
    fn default() -> Self {
        Self {
            join_soft_wraps: true,
            trim_trailing: true,
            block: false,
            first_column: 0,
            end_column: None,
        }
    }
}

impl BufferView<'_> {
    /// Copies one half-open cell span across physical and soft-wrapped rows.
    ///
    /// OSC 133 boundaries are cursor positions *between* cells, while a
    /// drag endpoint is inclusive. Both callers convert to this one rule.
    pub(crate) fn text_span(self, first: (LineId, usize), end: (LineId, usize)) -> String {
        // Existing contents callers retain default trim behavior through this wrapper.
        self.text_span_options(first, end, true)
    }

    /// Copies a half-open span with the configured trailing-space policy.
    pub fn text_span_options(
        self,
        first: (LineId, usize),
        end: (LineId, usize),
        trim: bool,
    ) -> String {
        if self.term.screen().ring.multicells.is_some() {
            // Feature dispatch is once per requested span, never per ordinary cell.
            return multicell::extract(
                self,
                first.0..=end.0,
                TextOptions {
                    trim_trailing: trim,
                    ..TextOptions::default()
                },
                Some((first, end)),
            );
        }
        // Only an explicit copy allocates; line inspection remains borrowed.
        let mut output = String::new();
        let mut previous_wrapped = false;
        for (index, line) in self.lines(first.0..=end.0).enumerate() {
            // Newline belongs to the predecessor’s hard boundary, not current wrapping.
            if index > 0 && !previous_wrapped {
                output.push('\n');
            }
            // First and last rows can each be partial while intervening rows are full.
            // The end column is a cursor boundary, allowing OSC D at column zero.
            let options = TextOptions {
                first_column: if line.id() == first.0 { first.1 } else { 0 },
                end_column: Some(if line.id() == end.0 {
                    end.1
                } else {
                    line.cells().len()
                }),
                ..TextOptions::default()
            };
            // Cluster extraction skips spacers and preserves complete Unicode sequences.
            let mut piece = line_text(line, options);
            // A wrapped row’s trailing spaces remain part of its continued logical text.
            if trim && !line.wrapped() {
                piece.truncate(piece.trim_end_matches(' ').len());
            }
            output.push_str(&piece);
            // Carry continuity into the next physical row without inventing a newline.
            previous_wrapped = line.wrapped();
        }
        output
    }

    /// Finds the first and last physical rows of one logical line.
    ///
    /// It returns `None` for an evicted ID instead of treating the same
    /// row allocation's new identity as the old line.
    #[must_use]
    pub fn logical_line(self, id: LineId) -> Option<RangeInclusive<LineId>> {
        let mut first = None;
        let mut found = false;
        let mut last = None;
        for row in self.term.screen().ring.iter() {
            if first.is_none() {
                first = Some(row.id);
            }
            if row.id == id {
                found = true;
            }
            last = Some(row.id);
            // A hard line boundary closes the candidate group. A match
            // inside it can return now without scanning later history.
            if !LineView::new(row, self.term).continues() {
                if found {
                    return Some(first?..=row.id);
                }
                first = None;
            }
        }
        if found { Some(first?..=last?) } else { None }
    }

    /// Copies text from retained rows with explicit wrap and block policy.
    ///
    /// The output is intentionally allocated only on this explicit copy
    /// operation, never when a row is merely inspected for rendering.
    #[must_use]
    pub fn text(self, range: RangeInclusive<LineId>, options: TextOptions) -> String {
        if self.term.screen().ring.multicells.is_some() {
            // The sparse extractor can start on a lower fragment and emit it once.
            return multicell::extract(self, range, options, None);
        }
        let mut result = String::new();
        let mut previous_wrapped = false;
        let mut had_row = false;
        for line in self.lines(range) {
            if had_row && (options.block || !options.join_soft_wraps || !previous_wrapped) {
                result.push('\n');
            }
            // Cluster extraction skips spacers and preserves complete Unicode sequences.
            let mut piece = line_text(line, options);
            // For a soft-wrapped line, spaces before the next physical
            // row are real text. Trim only when this logical line ends.
            if options.trim_trailing && (options.block || !line.wrapped()) {
                let trimmed = piece.trim_end_matches(' ').len();
                piece.truncate(trimmed);
            }
            result.push_str(&piece);
            // Carry continuity into the next physical row without inventing a newline.
            previous_wrapped = line.wrapped();
            had_row = true;
        }
        // A rectangular copy contains complete physical slices; its
        // final row keeps a terminator just like every preceding row (§6.9).
        if options.block && had_row {
            result.push('\n');
        }
        result
    }
}

/// Copies one physical row while omitting wide spacer cells.
///
/// A cluster contributes its full original Unicode sequence, while its
/// placeholder columns contribute nothing to the returned string.
fn line_text(line: LineView<'_>, options: TextOptions) -> String {
    let mut text = String::new();
    let end = options
        .end_column
        .unwrap_or(line.cells().len())
        .min(line.cells().len());
    for column in options.first_column.min(end)..end {
        let Some(cell) = line.cell(column) else {
            continue;
        };
        if cell.width == 0 {
            continue;
        }
        match cell.text {
            CellText::Scalar(ch) => text.push(ch),
            CellText::Cluster(cluster) => text.push_str(cluster),
            CellText::Empty => text.push(' '),
        }
    }
    text
}
