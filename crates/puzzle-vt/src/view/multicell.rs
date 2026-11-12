//! Sparse borrowed text geometry shared by copy, search and render publications.
//!
//! Row indexes select the rare path before looking up a cell key. The first
//! retained fragment is the logical owner even after the original root evicts.
//!
//! A row's block index moves with its allocation during scrolling. These readers
//! never infer a root by subtracting numeric LineIds: partial-region rotations
//! can change retained order, and eviction can remove the original top fragment.
//! Original vertical offsets remain on the record for later rendering.
//!
//! `continues` describes logical-band continuity. A tall block connects physical
//! rows without requiring the application to print a newline or a soft wrap.
//! `reservation_only` answers a different question: whether this row contains
//! any independently emitted text. Copy and search use both, so a reserved blank
//! row does not become a spurious empty line in the user's logical contents.
//!
//! Slot values are meaningful only after the inline MULTICELL flag was checked.
//! Zero is a valid key and must never be confused with an untouched blank here.
//! Every reference stays borrowed from the sole VT owner; frame snapshots copy
//! records explicitly instead of retaining these borrows across publications.
use super::{BufferView, CellFlags, LineId, LineView, RowFlags};
use crate::multicell::Multicell;

/// Rectangular text remains borrowed from the active screen's sole VT owner.
impl<'a> LineView<'a> {
    /// Selects sparse layout without scanning cells or allocating a table.
    #[must_use]
    pub fn has_multicells(self) -> bool {
        self.row.flags.contains(RowFlags::MULTICELLS)
    }
    /// Soft wraps and vertical reservations connect the same logical band.
    #[must_use]
    pub fn continues(self) -> bool {
        // Sparse traversal occurs only on flagged rows; ordinary rows test one bit.
        self.wrapped()
            || (self.has_multicells()
                && self.multicells().any(|(_, block)| {
                    block
                        .fragments
                        .last()
                        .is_some_and(|last| last.line != self.id())
                }))
    }
    /// Resolves a covered physical cell, including a lower continuation.
    #[must_use]
    pub fn multicell(self, column: usize) -> Option<&'a Multicell> {
        if !self.has_multicells() {
            return None;
        }
        let cell = self.row.cells.get(column)?;
        // Slot zero is a valid key, never an unwritten scalar on this path.
        if !cell.flags.contains(CellFlags::MULTICELL) {
            return None;
        }
        self.term.screen().ring.multicell(cell.ch)
    }
    /// Visits this row's distinct records once, without walking their cells.
    pub fn multicells(self) -> impl Iterator<Item = (u32, &'a Multicell)> {
        // One row entry per record avoids visiting every occupied physical cell.
        self.row.blocks.iter().filter_map(move |reference| {
            self.term
                .screen()
                .ring
                .multicell(reference.key)
                .map(|block| (reference.key, block))
        })
    }
    /// A lower reservation alone contributes neither spaces nor a hard newline.
    #[must_use]
    pub fn reservation_only(self) -> bool {
        self.has_multicells()
            && self.row.cells.iter().all(|cell| {
                if cell.flags.contains(CellFlags::MULTICELL) {
                    // The first retained row emits the payload, even with offset > 0.
                    self.term
                        .screen()
                        .ring
                        .multicell(cell.ch)
                        .is_some_and(|block| {
                            block
                                .fragments
                                .first()
                                .is_some_and(|first| first.line != self.id())
                        })
                } else {
                    // Printed spaces and coloured blanks remain independent content.
                    cell.ch == 0 && cell.bg == super::PackedColor::DEFAULT
                }
            })
    }
}

/// Cold geometry queries use the same physical rectangle as the renderer.
impl BufferView<'_> {
    /// Returns retained inclusive extent; physical mouse reports stay unchanged.
    #[must_use]
    pub fn multicell_extent(
        self,
        line: LineId,
        column: usize,
    ) -> Option<((LineId, usize), (LineId, usize))> {
        let block = self.line(line)?.multicell(column)?;
        // Fragment order follows the rectangle's retained document order.
        Some((
            (block.fragments.first()?.line, block.column),
            (
                block.fragments.last()?.line,
                // Inclusive extents match selection/search's physical convention.
                block.column + block.columns - 1,
            ),
        ))
    }
}
