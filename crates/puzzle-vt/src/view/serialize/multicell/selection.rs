//! Rich copy shares physical selection boundaries and logical block ownership.
//! Rectangles can start on a continuation, so CellView's whole-buffer owner is
//! replaced only for the first selected intersection of a given sparse record.
//!
//! Persistence restores rectangle geometry; a selection snippet instead exports
//! only the logical text selected by a gesture. Both HTML and VT must include
//! that text once even if the gesture starts below its original root. The source
//! style comes from the first selected fragment; the block keeps its sizing keys.
//! This explicit copy operation may allocate its output and seen-record list.
//! No such list exists in the parser or in ordinary frame assembly.
//! HTML uses the existing run merger, so adjacent equal styles stay one span.
//! VT retains OSC 66 metadata instead of translating it to a font-size setting.
//! A purely reserved lower row is not a hard newline in normal copy. A block
//! selection, however, retains its rectangular row boundaries deliberately.
use super::*;

/// Exports styled payloads once without exposing continuation keys or false lines.
pub(in super::super) fn rich(view: BufferView<'_>, range: CellRange, format: Format) -> String {
    // Only HTML needs an envelope. VT finishes with SGR reset so copying a
    // styled snippet does not leave the receiver's pen in its final rendition.
    let mut output = if format == Format::Html {
        "<pre>".into()
    } else {
        String::new()
    };
    let mut runs = html::Runs::default();
    let mut seen = Vec::new();
    // The whole-buffer owner may lie outside the selection. Track selected
    // intersections rather than trusting CellView's normal owner-only text.
    let mut previous_wrapped = true;
    for line in view.lines(range.start.0..=range.end.0) {
        // Pure previously copied reservations do not add a phantom hard boundary.
        if !range.block
            && line.reservation_only()
            && line.multicells().all(|(key, _)| seen.contains(&key))
        {
            continue;
        }
        if !previous_wrapped {
            // Flush a style span before a true logical boundary; otherwise the
            // HTML merger could accidentally swallow or relocate the newline.
            runs.finish(&mut output);
            output.push('\n');
        }
        let (begin, end) = columns(line, range);
        let mut column = begin;
        while column < end {
            append_cell(
                view,
                line,
                &mut column,
                &mut seen,
                &mut output,
                &mut runs,
                format,
            );
        }
        previous_wrapped = !range.block && line.continues();
    }
    finish(&mut output, &mut runs, range, format);
    output
}

/// Closes the format envelope after flushing the final shared HTML style run.
fn finish(output: &mut String, runs: &mut html::Runs, range: CellRange, format: Format) {
    // A final row terminator is selection policy, not a rectangle reservation.
    runs.finish(output);
    if range.block || range.final_newline {
        output.push('\n');
    }
    output.push_str(if format == Format::Html {
        "</pre>"
    } else {
        "\x1b[0m"
    });
}

/// Trims only real blank cells, never a selected lower rectangular fragment.
fn columns(line: LineView<'_>, range: CellRange) -> (usize, usize) {
    // Normal ranges use full intermediate rows; rectangular ranges repeat the
    // same horizontal limits on every row, including a selected lower fragment.
    let first = if range.block || line.id() == range.start.0 {
        range.start.1
    } else {
        0
    };
    let mut end = if range.block || line.id() == range.end.0 {
        range.end.1
    } else {
        line.cells().len()
    }
    .min(line.cells().len());
    if range.trim_trailing && (range.block || !line.wrapped()) {
        // Text lookup reports continuations as empty. Inspect MULTICELL first
        // so trimming cannot remove the gesture's only intersection of a block.
        while end > first
            && !line.cells()[end - 1].flags.contains(CellFlags::MULTICELL)
            && line
                .cell(end - 1)
                .is_some_and(|cell| matches!(cell.text, CellText::Empty | CellText::Scalar(' ')))
        {
            end -= 1;
        }
    }
    (first.min(end), end)
}

/// Kitty metadata is retained on rich-copy runs rather than flattened to font size.
fn write_block(output: &mut String, cell: CellView<'_>, block: &Multicell) {
    // All six keys are per-operation state. Later normal text must not inherit
    // this scale, and the receiver need not know any of our storage identities.
    output.push_str(&sgr(cell.raw, cell.underline_color));
    let s = block.sizing;
    output.push_str(&format!(
        "\x1b]66;s={}:w={}:n={}:d={}:v={}:h={};{}\x1b\\",
        s.scale, s.width, s.numerator, s.denominator, s.vertical, s.horizontal, block.text
    ));
}

/// Appends one logical item, advancing across all its physical continuation cells.
fn append_cell(
    view: BufferView<'_>,
    line: LineView<'_>,
    column: &mut usize,
    seen: &mut Vec<u32>,
    output: &mut String,
    runs: &mut html::Runs,
    format: Format,
) {
    // One iteration must either advance the horizontal frontier or terminate
    // the row. This includes missing records and malformed caller coordinates.
    let Some(mut cell) = line.cell(*column) else {
        // End this row on an invalid public coordinate, never retry it forever.
        *column = line.cells().len();
        return;
    };
    if let Some(block) = line.multicell(*column) {
        // Jump across the covered columns at once. A rectangle's lower
        // cells have no separate scalar or printable spacer content.
        *column = block.column + block.columns;
        if seen.contains(&cell.raw.ch) {
            // The first selected fragment already emitted this text. Geometry
            // still advances, but no duplicate style span or OSC is appended.
            return;
        }
        seen.push(cell.raw.ch);
        // Override only this borrowed view; the VT's immutable cells
        // keep their private key and ordinary physical coordinates.
        cell.text = CellText::Cluster(&block.text);
        cell.width = block.columns as u8;
        if format == Format::Vt {
            // Emit one protocol operation with inherited SGR, retaining
            // fractional alignment as well as the occupied dimensions.
            write_block(output, cell, block);
            return;
        }
    } else {
        // Ordinary cells advance by one; a wide spacer is skipped on the next
        // iteration, retaining the caller's exact physical selection boundary.
        *column += 1;
    }
    if cell.width == 0 {
        // Ordinary wide spacers remain suppressed exactly as before.
        return;
    }
    if format == Format::Html {
        // Reuse escaping and style coalescing from ordinary rich copy.
        runs.cell(output, cell, view.term);
    } else {
        output.push_str(&sgr(cell.raw, cell.underline_color));
        push_text(output, cell.text);
        // The shared emitter keeps empty ordinary cells as literal spaces.
    }
}
