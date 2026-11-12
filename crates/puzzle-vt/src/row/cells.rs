//! Full-width capacity with a lazily erased suffix (D-51).
//!
//! Only the written prefix contains authoritative stored cells. The remaining
//! columns read as one row-local BCE cell. Recycling changes two small values
//! instead of touching a cold history allocation. No raw slice is exposed:
//! every reader, including snapshots and reflow, sees the logical cells.
//! Writing a distant column materializes only its preceding gap, preserving
//! colors that were erased before the write. Storage remains full-width and
//! retained-byte accounting charges its capacity, not the sparse text length.
use crate::Cell;
use std::{
    fmt,
    ops::{Index, IndexMut, Range},
};

/// Logical iteration applies the same implicit-fill rule as indexed reads.
mod iter;
pub use iter::CellsIter;

/// One row's reusable physical allocation and its logical erased suffix.
pub struct RowCells {
    /// Allocated slots; positions after extent may contain retired stale ink.
    storage: Vec<Cell>,
    /// Number of leading slots whose contents are authoritative.
    extent: usize,
    /// BCE value returned for every implicit trailing column.
    fill: Cell,
    /// The unit-test oracle retains the former eagerly cleared representation.
    #[cfg(test)]
    eager: bool,
}

/// All cell access passes through the written-extent boundary.
impl RowCells {
    /// Allocates fixed-width capacity once, with a completely implicit blank row.
    pub(super) fn new(columns: usize) -> Self {
        // Initialize the backing allocation once. Later resets may leave its
        // retired bytes untouched because extent determines which bytes are live.
        #[cfg(test)]
        crate::scroll_operations::written(columns);
        Self {
            storage: vec![Cell::default(); columns],
            extent: 0,
            fill: Cell::default(),
            #[cfg(test)]
            eager: false,
        }
    }
    /// Logical width is independent of the amount of text written.
    pub fn len(&self) -> usize {
        // Erasing text changes extent, never the terminal's logical column count.
        self.storage.len()
    }
    /// Empty storage is permitted for internal copies, although terminal grids are nonempty.
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }
    /// Capacity continues to count against the retained-memory budget.
    pub fn capacity(&self) -> usize {
        self.storage.capacity()
    }
    /// Describes stored work without scanning cells or materializing blanks.
    pub fn written_extent(&self) -> usize {
        // This is stored work, not visible ink: printed spaces count as writes.
        self.extent
    }
    /// Allocation identity is diagnostic only; stale suffix cells must never be read through it.
    pub fn as_ptr(&self) -> *const Cell {
        self.storage.as_ptr()
    }
    /// Returns an authoritative stored cell or the row's implicit BCE value.
    pub fn get(&self, column: usize) -> Option<&Cell> {
        // The bounds check preserves Vec::get's absent-column behavior, whereas
        // a valid erased column always returns an actual row-local blank value.
        if column >= self.len() {
            return None;
        }
        Some(if column < self.extent {
            &self.storage[column]
        } else {
            &self.fill
        })
    }
    /// Returns the final logical cell, including an implicit trailing blank.
    pub fn last(&self) -> Option<&Cell> {
        // Empty internal rows cannot underflow when locating their final column.
        self.len()
            .checked_sub(1)
            .and_then(|column| self.get(column))
    }
    /// Makes a writable column real, initializing only its preceding implicit gap.
    pub fn get_mut(&mut self, column: usize) -> Option<&mut Cell> {
        // A mutable reference cannot point at the shared fill template: changing
        // it would alter every implicit column instead of this one grid cell.
        if column >= self.len() {
            return None;
        }
        self.materialize(column + 1);
        #[cfg(test)]
        crate::scroll_operations::written(1);
        Some(&mut self.storage[column])
    }
    /// Replaces a complete cell without first writing a blank to that same slot.
    pub(crate) fn store(&mut self, column: usize, cell: Cell) {
        // INVARIANT: the parser has already validated this column's logical cell.
        assert!(column < self.len());
        // A distant write preserves the intervening BCE gap. At the next
        // sequential column the gap is empty, so printing performs one store.
        self.materialize(column);
        #[cfg(test)]
        crate::scroll_operations::written(1);
        self.storage[column] = cell;
        self.extent = self.extent.max(column + 1);
    }
    /// Reads logical width without exposing the retired contents of the allocation.
    pub fn iter(&self) -> CellsIter<'_> {
        CellsIter::new(self)
    }
    /// Replaces a whole row in constant time, retaining all physical capacity.
    pub fn fill(&mut self, cell: Cell) {
        // Replacing both authority markers invalidates the entire old prefix.
        // Sparse extras and row versions are maintained separately by Row.
        self.extent = 0;
        self.fill = cell;
        #[cfg(test)]
        if self.eager {
            // Reference rows perform the former full-width erase on every recycle.
            crate::scroll_operations::cleared(self.len());
            self.storage.fill(cell);
            self.extent = self.len();
        }
    }
    /// Replaces an interval, representing a trailing erase without writing its suffix.
    pub fn fill_range(&mut self, range: Range<usize>, cell: Cell) {
        // VT erase spans are clamped here too; an empty interval cannot change
        // the background of the rest of the row as a whole-row reset would.
        let end = range.end.min(self.len());
        let start = range.start.min(end);
        if start == end {
            return;
        }
        #[cfg(test)]
        if self.eager {
            // The reference deliberately writes every erased column, even a suffix.
            crate::scroll_operations::cleared(end - start);
            self.storage[start..end].fill(cell);
            return;
        }
        if end == self.len() {
            // Only a changed BCE color needs the earlier implicit gap preserved.
            // Erasing default paper far to the right must not make that gap real.
            if cell != self.fill {
                self.materialize(start);
                self.extent = start;
            } else {
                self.extent = self.extent.min(start);
            }
            self.fill = cell;
        } else if cell == self.fill {
            // Erasing an already implicit interval has no physical work to do.
            let stop = end.min(self.extent);
            if start < stop {
                #[cfg(test)]
                crate::scroll_operations::cleared(stop - start);
                self.storage[start..stop].fill(cell);
            }
        } else {
            // An interior color change is real content until a later suffix erase.
            self.materialize(end);
            #[cfg(test)]
            crate::scroll_operations::cleared(end - start);
            self.storage[start..end].fill(cell);
        }
    }
    /// Moves columns for ICH/DCH without copying an implicit source tail.
    pub fn copy_within(&mut self, source: Range<usize>, destination: usize) {
        // INVARIANT: callers clamp edits to the active horizontal margins.
        assert!(source.start <= source.end && source.end <= self.len());
        let count = source.end - source.start;
        // Validate before subtracting the destination from width; both spans
        // must be inside the same row even when no stored source cells exist.
        assert!(destination <= self.len() && count <= self.len() - destination);
        let copied = self.extent.min(source.end).saturating_sub(source.start);
        // Source columns after extent all have the same fill. Only the live
        // prefix requires memmove; its overlap semantics matter for ICH/DCH.
        if copied > 0 {
            // Initialize destination gaps before memmove; existing source slots stay intact.
            self.materialize(destination + copied);
            // Count each copied destination once, even when source and destination overlap.
            // Gap fills are charged separately by materialize, so neither kind of write is hidden.
            #[cfg(test)]
            crate::scroll_operations::written(copied);
            self.storage
                .copy_within(source.start..source.start + copied, destination);
        }
        let start = destination + copied;
        // Retire or overwrite destination ink covered by the implicit source.
        // A bounded horizontal edit must preserve stored cells beyond its end.
        let end = (destination + count).min(self.extent);
        if start < end {
            #[cfg(test)]
            crate::scroll_operations::cleared(end - start);
            self.storage[start..end].fill(self.fill);
        }
        if destination + count >= self.extent {
            // The moved implicit suffix can also retire old stored trailing blanks.
            self.extent = self.extent.min(start);
        }
    }
    /// Plain resizing preserves the old BCE tail, but newly added cells use the supplied blank.
    pub fn resize(&mut self, columns: usize, blank: Cell) {
        let old = self.len();
        if columns > old && blank != self.fill {
            // Two different suffix colors require storing the old part before extension.
            self.materialize(old);
            self.fill = blank;
        }
        #[cfg(test)]
        crate::scroll_operations::written(columns.saturating_sub(old));
        self.storage.resize(columns, blank);
        // Shrinking clips authority as well as storage; a later expansion must
        // not resurrect the cells that used to lie beyond the right margin.
        self.extent = self.extent.min(columns);
    }
    /// Initializes a newly written prefix only as far as its last requested column.
    fn materialize(&mut self, end: usize) {
        // Every caller has checked end against width. Materialization leaves
        // the old stored prefix intact and never allocates another cell vector.
        if end > self.extent {
            #[cfg(test)]
            crate::scroll_operations::cleared(end - self.extent);
            self.storage[self.extent..end].fill(self.fill);
            self.extent = end;
        }
    }
    /// Selects the original full-cell-storage policy only in the eager test oracle.
    #[cfg(test)]
    pub(crate) fn make_eager(&mut self) {
        self.materialize(self.len());
        self.eager = true;
    }
}

/// Cold clones allocate a row once; warmed snapshot copies only its stored prefix.
impl Clone for RowCells {
    /// A distinct owner retains full-width capacity and the same logical fill.
    fn clone(&self) -> Self {
        #[cfg(test)]
        crate::scroll_operations::written(self.len());
        Self {
            storage: self.storage.clone(),
            extent: self.extent,
            fill: self.fill,
            #[cfg(test)]
            eager: self.eager,
        }
    }
    /// Snapshot circulation keeps capacity and never copies retired suffix ink.
    fn clone_from(&mut self, source: &Self) {
        // A width change may grow this owner once. At a warmed fixed geometry
        // resize is inert, and retired source bytes are neither read nor copied.
        #[cfg(test)]
        crate::scroll_operations::written(source.len().saturating_sub(self.len()) + source.extent);
        self.storage.resize(source.len(), Cell::default());
        self.storage[..source.extent].copy_from_slice(&source.storage[..source.extent]);
        self.extent = source.extent;
        self.fill = source.fill;
        #[cfg(test)]
        {
            self.eager = source.eager;
        }
    }
}
/// Equality compares terminal contents rather than retired physical memory.
impl PartialEq for RowCells {
    /// Independent sparse representations of the same logical row compare equal.
    fn eq(&self, other: &Self) -> bool {
        // Cache diagnostics compare logical cells, including different BCE
        // suffix boundaries that nevertheless represent exactly the same row.
        self.iter().eq(other.iter())
    }
}
/// Logical equality is reflexive for every compact cell representation.
impl Eq for RowCells {}
/// Grid/debug snapshots retain their former full-width list representation.
impl fmt::Debug for RowCells {
    /// Formatting cannot leak an evicted line through the implicit suffix.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}
/// Existing column readers share the same guarded logical lookup.
impl Index<usize> for RowCells {
    /// Logical indexing borrows one compact terminal cell.
    type Output = Cell;
    /// Bounds remain the caller's grid invariant, as for the former Vec.
    fn index(&self, column: usize) -> &Cell {
        // INVARIANT: indexed parser/model paths already clamp the column to grid width.
        assert!(column < self.len());
        if column < self.extent {
            &self.storage[column]
        } else {
            &self.fill
        }
    }
}
/// A column mutation always initializes its preceding logical gap.
impl IndexMut<usize> for RowCells {
    /// Bounds checks precede materialization, preventing an out-of-range slice.
    fn index_mut(&mut self, column: usize) -> &mut Cell {
        // INVARIANT: only valid terminal coordinates use this indexing convenience.
        assert!(column < self.len());
        self.materialize(column + 1);
        #[cfg(test)]
        crate::scroll_operations::written(1);
        &mut self.storage[column]
    }
}
/// Borrowed loops cannot bypass the implicit suffix by accessing physical storage.
impl<'a> IntoIterator for &'a RowCells {
    /// Values borrow either authoritative storage or the row's fill cell.
    type Item = &'a Cell;
    /// Full-width double-ended traversal never materializes a suffix.
    type IntoIter = CellsIter<'a>;
    /// Iteration remains allocation-free and covers every logical column.
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Structural, BCE and randomized old-representation equivalence witnesses.
#[cfg(test)]
mod tests;
