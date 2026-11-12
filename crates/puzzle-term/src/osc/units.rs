//! Source interval boundaries cannot cut an indivisible grid text unit.
//! The source walker supplies its own captured gaps; consumers never choose
//! coordinates or expand a range to include unselected text. Wide characters
//! and rectangular blocks must fit completely, including vertical fragments.
//! No default parser/frame invokes these checks: they run only during admitted,
//! bounded source inspection for an explicitly registered consumer.
//!
//! SourceScan verifies the issuing registration and current document before
//! constructing this row view. Geometry alone is not an attachment certificate:
//! the consumer must still inspect hidden/blank text, spend its physical visits,
//! admit retained capacity and install a current source witness. Keeping those
//! decisions separate lets other protocols reuse the geometry rule without
//! inheriting this optional protocol's semantic or authorization policy.
use super::{OscContext, SourceRow};
use puzzle_vt::{CellFlags, multicell::Multicell};

/// Read source units only through the exclusive current-document context.
impl OscContext<'_> {
    /// Borrow the active document's source unit; no layout/font work is required.
    /// This read is for trusted source inspection, not a producer coordinate API.
    pub fn source_block(&self, key: u32) -> Option<&Multicell> {
        self.terminal.screen().ring.multicell(key)
    }
}

/// Captured row views enforce unit geometry without acquiring source ownership.
impl SourceRow<'_> {
    /// True only when every included wide/rectangular unit is included whole.
    /// False is refusal, never permission to widen the caller's selected text.
    pub fn contains_complete_units(&self, source: &OscContext<'_>) -> bool {
        if self.columns.is_empty() {
            return true;
        }
        // The excluded end cannot land between a wide lead and its spacer.
        // The first included cell cannot be the orphaned second half either.
        if self.row.cells[self.columns.start]
            .flags
            .contains(CellFlags::WIDE_SPACER)
            || self.row.cells[self.columns.end - 1]
                .flags
                .contains(CellFlags::WIDE)
        {
            return false;
        }
        // Each row-side reference names one block, avoiding a store lookup for
        // every covered cell of the same rectangle. At most seven fragments
        // are metadata in that fixed block, not another physical-row traversal.
        for reference in &self.row.blocks {
            let Some(block) = source.source_block(reference.key) else {
                return false;
            };
            let right = block.column + block.columns;
            if self.columns.start >= right || self.columns.end <= block.column {
                continue;
            }
            if self.columns.start > block.column
                || self.columns.end < right
                || !self.contains_rectangle(block)
            {
                return false;
            }
        }
        true
    }

    /// Inspect only the block's bounded fragment list and the two captured gaps.
    fn contains_rectangle(&self, block: &Multicell) -> bool {
        // A partly evicted rectangle cannot be reattached as a complete unit.
        // Its ordinary terminal copy/render behavior remains independently owned.
        if block.fragments.len() != usize::from(block.sizing.scale)
            || block
                .fragments
                .iter()
                .enumerate()
                .any(|(index, f)| usize::from(f.offset) != index)
        {
            return false;
        }
        let [start, end] = self.bounds;
        let right = block.column + block.columns;
        for (index, fragment) in block.fragments.iter().enumerate() {
            // A source beginning on a lower fragment omits the original first
            // row even if the left edge itself is included. The end is symmetric.
            if fragment.line == start.line && (index != 0 || start.column > block.column) {
                return false;
            }
            if fragment.line == end.line
                && (index + 1 != block.fragments.len() || end.column < right)
            {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
/// Source admission must preserve all dimensions of an atomic text unit.
mod tests;
