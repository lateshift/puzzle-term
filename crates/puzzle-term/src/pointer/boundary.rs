//! Text-view hit testing for M12.selection, independent of mouse protocol cells.
//!
//! A simple drag names gaps between whole graphemes, including the gap after
//! the final column. Rounded boundaries never cut a wide glyph or OSC 66 block.
//! Only default-attribute trailing blanks snap; wrapped rows remain full width.
//!
//! Hosts supply grid-relative device pixels. Dividing by the current device
//! cell width makes the threshold independent of display scale and font size.
//! A half-cell decision is deliberately not reused for xterm mouse reporting:
//! that protocol reports the cell under the pointer, not a text insertion gap.
//!
//! The same function is used for presses, motion and stationary edge dragging.
//! Consequently reversing direction does not move the fixed anchor, and a
//! release without crossing a midpoint cannot manufacture a selected character.
use super::{PointerInput, SelectPoint, Terminal};
use crate::vt::{Cell, CellFlags};

/// Resolves a nearest text edge, optionally collapsing a hard row's blank suffix.
pub(super) fn hit(
    term: &Terminal,
    point: SelectPoint,
    input: PointerInput,
    snap: bool,
) -> SelectPoint {
    let Some(line) = term.buffer().line(point.line) else {
        return point;
    };
    // Clipping admits the gap after the last column, unlike cell hit testing.
    let width = term.size().0;
    let x = (input.position[0] / f64::from(input.cell[0].max(1))).clamp(0.0, width as f64);
    // A filled rectangle is one text item even when its lower fragment was hit.
    if let Some((first, last)) = term.buffer().multicell_extent(point.line, point.column) {
        return if x < (first.1 + last.1 + 1) as f64 / 2.0 {
            SelectPoint {
                line: first.0,
                column: first.1,
            }
        } else {
            SelectPoint {
                line: last.0,
                column: last.1 + 1,
            }
        };
    }
    if snap && !line.wrapped() {
        // Inspect only this gesture's row, never the document or frame hot path.
        // Styled blanks, links and protected cells remain deliberate grid content.
        let end = line
            .cells()
            .iter()
            .rposition(|cell| !padding(*cell))
            .map_or(0, |column| column + 1);
        if x >= end as f64 {
            return SelectPoint {
                column: end,
                ..point
            };
        }
    }
    // The floored hit identifies the complete unit that contains this pointer.
    let cell = line.cells()[point.column];
    // Both physical halves choose the same lead before testing the midpoint;
    // otherwise a narrow threshold in the spacer would select half a character.
    let first = if cell.flags.contains(CellFlags::WIDE_SPACER) {
        point.column.saturating_sub(1)
    } else {
        point.column
    };
    // Use the whole glyph's midpoint, rather than each half-cell's midpoint.
    let span = if line.cells()[first].flags.contains(CellFlags::WIDE) {
        2
    } else {
        1
    };
    // Equality chooses the right edge consistently at both gesture endpoints.
    // Comparing absolute positions also keeps leftward drags symmetric.
    let column = if x < first as f64 + span as f64 / 2.0 {
        first
    } else {
        first + span
    };
    SelectPoint { column, ..point }
}

/// Printed plain spaces and untouched default blanks are the same line-end padding.
fn padding(mut cell: Cell) -> bool {
    // Preserve every style/flag bit during comparison; a coloured erasure or
    // an invisible attributed space can still be intentionally selected content.
    if cell.ch == u32::from(' ') {
        cell.ch = 0;
    }
    cell == Cell::default()
}
