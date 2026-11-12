//! Double-ended logical cell iteration, including lazily erased columns.
//! An index range provides an exact remaining length without materializing
//! the row. Reflow's reverse occupancy search and bulk exports therefore
//! preserve their existing ordering and allocation behavior.
use super::{Cell, RowCells};
use std::ops::Range;

/// Borrowed full-width iterator whose suffix reads one immutable fill cell.
pub struct CellsIter<'a> {
    /// The owner supplies either stored cells or its row-local BCE fill.
    cells: &'a RowCells,
    /// Unvisited logical column interval, shared by both directions.
    remaining: Range<usize>,
}
/// Construction captures logical width rather than the written extent.
impl<'a> CellsIter<'a> {
    /// Starts at the first column even for a completely erased row.
    pub(super) fn new(cells: &'a RowCells) -> Self {
        Self {
            cells,
            remaining: 0..cells.len(),
        }
    }
}
/// Forward traversal uses exactly the same logical read as direct indexing.
impl<'a> Iterator for CellsIter<'a> {
    /// Both prefix and suffix values remain borrowed from this one row.
    type Item = &'a Cell;
    /// One index advance does not allocate or alter the row's stored prefix.
    fn next(&mut self) -> Option<Self::Item> {
        self.remaining
            .next()
            .and_then(|column| self.cells.get(column))
    }
    /// The underlying range already knows the exact unvisited count.
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.remaining.size_hint()
    }
}
/// Reverse iteration lets occupancy trim default blanks without copying a row.
impl DoubleEndedIterator for CellsIter<'_> {
    /// Front and back share the index range, so they cannot yield a cell twice.
    fn next_back(&mut self) -> Option<Self::Item> {
        self.remaining
            .next_back()
            .and_then(|column| self.cells.get(column))
    }
}
/// Width and iterator endpoints are bounded by the physical storage length.
impl ExactSizeIterator for CellsIter<'_> {}
