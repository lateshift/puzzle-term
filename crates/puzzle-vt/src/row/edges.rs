//! A column boundary cuts an atomic glyph only between its occupied cells.
//! Splitting a wide glyph clears both of its halves before a splice.
//! Puzzle extends that same predicate to sparse rectangles. Row can erase a
//! wide pair locally; Terminal must erase every fragment of a split rectangle.
//! There is no scan of the edited suffix and no predicate in ordinary printing.
use crate::{Cell, CellFlags, Row, WriteClock};

/// Ownership to erase when a source or destination edge divides an atomic unit.
pub(crate) enum Boundary {
    /// The wide lead and following spacer share one two-column footprint.
    Wide,
    /// All fragments belong to this checked sparse block key.
    Block(u32),
}

/// Inspect the two cells adjoining a half-open edge, independent of operation.
pub(crate) fn split_at(row: &Row, column: usize) -> Option<Boundary> {
    // Attribute actual edge visits so ordinary-print zeros cannot hide a suffix scan.
    #[cfg(test)]
    crate::print_work::note(crate::print_work::Operation::Edge);
    if column == 0 || column >= row.cells.len() {
        // A physical row edge cannot divide a valid ordinary wide pair.
        return None;
    }
    let left = row.cells[column - 1];
    let right = row.cells[column];
    // Test the lead side; it owns the two-cell footprint.
    if left.flags.contains(CellFlags::WIDE) {
        Some(Boundary::Wide)
    } else if left.flags.contains(CellFlags::MULTICELL)
        && right.flags.contains(CellFlags::MULTICELL)
        && left.ch == right.ch
    {
        // Adjacent blocks may touch; equal live keys distinguish an interior cut.
        Some(Boundary::Block(left.ch))
    } else {
        None
    }
}

/// Local erasure uses the caller's BCE, removing sparse extras and reporting writes.
impl Row {
    /// Clear both halves before a range moves/erases/truncates either one.
    /// Selective erasure preserves the protected pair as an indivisible unit.
    pub(crate) fn erase_split_wide_at(
        &mut self,
        column: usize,
        blank: Cell,
        clock: &mut WriteClock,
        selective: bool,
    ) -> bool {
        // Only a split wide pair is repaired here; sparse blocks belong to Terminal.
        if !matches!(split_at(self, column), Some(Boundary::Wide)) {
            return false;
        }
        // A protected half keeps the whole pair intact under selective erasure.
        // Neither cell may change, or the glyph would be left half erased.
        if selective
            && [column - 1, column]
                .into_iter()
                .any(|x| self.cells[x].flags.contains(CellFlags::PROTECTED))
        {
            return false;
        }
        // Both addresses were bounded by split_at. Whole-cell stores retire
        // their cluster/link/underline extras and observe even the outside write.
        self.put(column - 1, blank, clock);
        self.put(column, blank, clock);
        true
    }
}
