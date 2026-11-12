//! Ring/grid growth is observable without an allocator, clock or platform API.
//! The desktop samples these lifetime counters before and after Session::feed.
//! Its diagnostic can connect a maximum lock hold to a new cell allocation or
//! deque relocation, rather than guessing from the elapsed time. They count
//! only ring/grid storage, not sparse cluster strings or other subsystems.
use super::*;

/// Lifetime allocation/relocation events for one screen's row ring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StorageGrowth {
    /// New full-width row allocations, including the original visible grid.
    pub rows: u64,
    /// Logical cell bytes requested for those allocations (not process RSS).
    pub cell_bytes: u64,
    /// Deque appends/inserts with no spare slot, requiring metadata growth.
    pub deque_growths: u64,
}

impl RowRing {
    /// Copies counters in constant time; no historical-row traversal occurs.
    pub fn storage_growth(&self) -> StorageGrowth {
        self.growth
    }

    /// Reports actual history row allocations reused at either capacity ceiling.
    pub fn recycled_rows(&self) -> u64 {
        self.recycled_rows
    }

    /// Attributes cold row allocation before its cells are initialized.
    pub(super) fn new_row(&mut self, id: LineId, clock: &mut crate::WriteClock) -> Box<Row> {
        if let Some(mut row) = self.spares.pop() {
            // A cleared identity never returns, only its privately owned allocation.
            self.spare_bytes = self
                .spare_bytes
                .saturating_sub(Self::retained_row_bytes(&row));
            row.recycle(id, PackedColor::DEFAULT, clock);
            return row;
        }
        // Recycled rows bypass this site; only actual initial growth counts.
        self.growth.rows = self.growth.rows.saturating_add(1);
        self.growth.cell_bytes = self
            .growth
            .cell_bytes
            .saturating_add(self.columns.saturating_mul(size_of::<crate::Cell>()) as u64);
        Box::new(Row::new(id, self.columns))
    }

    /// Observes a relocation event immediately before an append or insert.
    pub(super) fn observe_deque_growth(&mut self) {
        // Construction reserves the bounded slots. A later limit increase or
        // reflow can change that high-water mark, so this observation remains.
        if self.rows.len() == self.rows.capacity() {
            self.growth.deque_growths = self.growth.deque_growths.saturating_add(1);
        }
    }
}
