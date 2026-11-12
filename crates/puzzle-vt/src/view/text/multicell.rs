//! Explicit text extraction emits each intersected rectangular payload once.
//!
//! This cold copy path is selected at the operation boundary. Ordinary text
//! keeps its existing extractor; lower reservations never manufacture spaces.
//!
//! A cell range can start in the middle of a rectangle. Its payload belongs to
//! the first selected intersection, not necessarily the original top-left cell.
//! `seen` therefore belongs to the whole copy operation rather than one row.
//! It stores keys only for the duration of the immutable BufferView borrow.
//!
//! Physical padding is separate from logical text. A previously emitted block's
//! lower-only row contributes neither its blank neighbours nor a hard newline.
//! Rows containing ordinary output still follow the caller's trim and wrap rules.
//! Rectangular copies deliberately retain physical row boundaries and terminate
//! their final slice, even when an earlier slice already emitted a block's text.
//!
//! First/last edges describe a normal half-open interval. Without edges, the
//! column options apply to every physical row, as required by rectangular copy.
//! Clamping both forms prevents an untrusted API range from indexing past a row.
//! The returned String is an explicit cold copy; borrowed cell access allocates
//! nothing and the ordinary extractor never constructs this temporary key set.
use super::{BufferView, CellFlags, CellText, LineId, TextOptions};

/// Copies a range with optional first/last partial-row boundaries.
pub(super) fn extract(
    view: BufferView<'_>,
    range: std::ops::RangeInclusive<LineId>,
    options: TextOptions,
    edges: Option<((LineId, usize), (LineId, usize))>,
) -> String {
    let mut result = String::new();
    let mut seen = Vec::new();
    let mut previous_wrapped = false;
    let mut emitted_row = false;
    for line in view.lines(range) {
        // Already emitted reservations carry no logical padding, even if their
        // physical row was wrapped while finding room for a taller neighbour.
        if !options.block
            && line.reservation_only()
            && line.multicells().all(|(key, _)| seen.contains(&key))
        {
            continue;
        }
        let start = edges.map_or(options.first_column, |(first, _)| {
            if first.0 == line.id() { first.1 } else { 0 }
        });
        let end = edges
            .map_or(
                options.end_column.unwrap_or(line.cells().len()),
                |(_, end)| {
                    if end.0 == line.id() {
                        end.1
                    } else {
                        line.cells().len()
                    }
                },
            )
            .min(line.cells().len());
        // De-duplicate before trimming: payload spaces obey the user's copy policy.
        let mut piece = row_text(line, start, end, &mut seen);
        // The piece may contain payload spaces. Trim after extraction, while
        // continuation-only rows have already been removed at the row boundary.
        if options.trim_trailing && (options.block || !line.wrapped()) {
            piece.truncate(piece.trim_end_matches(' ').len());
        }
        // A lower-only row disappears from logical copy, including its boundary.
        if !options.block && piece.is_empty() && line.reservation_only() {
            continue;
        }
        if emitted_row && (options.block || !options.join_soft_wraps || !previous_wrapped) {
            result.push('\n');
        }
        result.push_str(&piece);
        // A tall block connects its reserved rows even without a WRAPPED flag.
        previous_wrapped = line.continues();
        emitted_row = true;
    }
    if options.block && emitted_row {
        // Rectangular copy preserves the final physical row terminator too.
        result.push('\n');
    }
    result
}

/// Emits one physical slice, with a caller-owned set across all selected rows.
fn row_text(
    line: super::super::LineView<'_>,
    start: usize,
    end: usize,
    seen: &mut Vec<u32>,
) -> String {
    let mut piece = String::new();
    let mut column = start.min(end);
    while column < end {
        let raw = line.cells()[column];
        if raw.flags.contains(CellFlags::MULTICELL) {
            if let Some(block) = line.multicell(column) {
                // A rectangular selection may first encounter a lower fragment.
                if !seen.contains(&raw.ch) {
                    seen.push(raw.ch);
                    piece.push_str(&block.text);
                }
                column = (block.column + block.columns).min(end);
                // Horizontal continuations are geometry, never additional text.
                continue;
            }
        } else if let Some(cell) = line.cell(column)
            && cell.width != 0
        {
            // Reuse CellView's scalar/cluster decoding for all ordinary neighbours.
            match cell.text {
                CellText::Empty => piece.push(' '),
                CellText::Scalar(ch) => piece.push(ch),
                CellText::Cluster(text) => piece.push_str(text),
            }
        }
        column += 1;
    }
    piece
}
