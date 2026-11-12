//! Source and destination column edges share the same atomic cut preparation.
//! Movement must prepare every affected row before copying any cell. Otherwise
//! a copied spacer can look paired with an unrelated destination lead. Sparse
//! rectangles require Session-owned whole-block retirement, not row-local fills.
use super::*;
use crate::row::edges::{Boundary, split_at};

/// Boundary inspection is cold control work; full-width row-handle scrolling bypasses it.
impl Terminal {
    /// Prepare both half-open edges before a bounded erase, shift or rectangle move.
    pub(super) fn prepare_column_edges(
        &mut self,
        y: usize,
        start: usize,
        end: usize,
        selective: bool,
    ) {
        // Empty selections cannot cut a glyph. In particular, do not erase a
        // pair merely because two equal coordinates happen to bisect it.
        if start >= end {
            return;
        }
        for edge in [start, end] {
            // Resolve each edge afresh: clearing a sparse rectangle at the
            // first edge may also have removed the cell at the second edge.
            self.split_column_boundary(y, edge, selective);
        }
    }

    /// One cut rule handles wide pairs and all fragments of a sparse text block.
    fn split_column_boundary(&mut self, y: usize, edge: usize, selective: bool) {
        // Invalid addressed rows are a harmless no-op, as for the enclosing edit.
        let Some(row) = self.screen().row(y) else {
            return;
        };
        let Some(boundary) = split_at(row, edge) else {
            // Complete glyphs wholly on either side are not part of this erase.
            return;
        };
        if selective && row.cells[edge - 1].flags.contains(CellFlags::PROTECTED) {
            // DECSCA applies to the whole admitted block/pair, not one fragment.
            return;
        }
        match boundary {
            // Row-local fills cannot retire a block's other rows or its payload;
            // the Terminal's existing sparse eraser owns that complete lifetime.
            Boundary::Block(key) => self.remove_multicell(key),
            Boundary::Wide => {
                // The current pen background supplies BCE to both halves,
                // including the half outside the caller's moving rectangle.
                let blank = Cell::erased(self.pen.bg);
                let (screen, clock) = self.screen_clock_mut();
                if let Some(row) = screen.row_mut(y)
                    && row.erase_split_wide_at(edge, blank, clock, selective)
                {
                    // A source boundary erase can be outside the nominal edit;
                    // it needs damage just as much as the copied destination.
                    screen.damage.mark(y);
                }
            }
        }
    }

    /// An ICH/DCH splice has one interior source cut in addition to its outer edges.
    pub(super) fn prepare_column_splice(
        &mut self,
        y: usize,
        start: usize,
        end: usize,
        count: usize,
        insert: bool,
    ) {
        self.prepare_column_edges(y, start, end, false);
        // ICH discards a source suffix; DCH discards a source prefix. Clearing
        // their inner cut first ensures copy_within moves only complete glyphs.
        let inner = if insert { end - count } else { start + count };
        self.split_column_boundary(y, inner, false);
    }
}
