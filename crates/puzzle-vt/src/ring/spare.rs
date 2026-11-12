//! Cleared history retires text, not reusable grid allocations (M16.8).
//!
//! RIS and ED3 make every old history identity inaccessible immediately. Dropping
//! full-width rows and allocating them again on subsequent output needlessly
//! touches cold memory. This pool holds only allocations already paid for, and
//! shares both the existing history byte ceiling and effective line ceiling.
//! No spare is visible through document iteration, search or snapshot capture.
use super::{RowHandle, RowRing};

impl RowRing {
    /// Cold activation covers retained rows and inactive allocations on both screens.
    pub(crate) fn maximum_write_version(&self) -> u64 {
        // Spares can return after registration; include their last cache keys too.
        self.rows
            .iter()
            .chain(&self.spares)
            .map(|row| row.version)
            .max()
            .unwrap_or(0)
    }

    /// Retains an evicted allocation only when live history plus spares still fit.
    pub(super) fn retain_spare(&mut self, row: RowHandle) {
        let bytes = Self::retained_row_bytes(&row);
        // Sparse metadata belongs to the allocation too; never discount cluster
        // capacity just because the row no longer has a document identity.
        if self.spares.len() + self.scrollback_len() < self.scrollback_lines
            && self
                .history_bytes
                .saturating_add(self.multicell_history_overhead())
                .saturating_add(self.spare_bytes)
                .saturating_add(bytes)
                <= self.scrollback_bytes
        {
            self.spare_bytes = self.spare_bytes.saturating_add(bytes);
            // The vector reserved these slots at construction. Clearing history
            // therefore does not introduce an allocation at the reset boundary.
            self.spares.push(row);
        }
    }

    /// Releases inactive storage before a live-limit change can exceed its bound.
    pub(super) fn trim_spares(&mut self) {
        // Ordinary scalar output does not visit this loop. Only newly retained
        // sparse data or smaller limits can exceed the combined storage ceiling.
        while self.spares.len() + self.scrollback_len() > self.scrollback_lines
            || self
                .history_bytes
                .saturating_add(self.multicell_history_overhead())
                .saturating_add(self.spare_bytes)
                > self.scrollback_bytes
        {
            let Some(row) = self.spares.pop() else {
                break;
            };
            self.spare_bytes = self
                .spare_bytes
                .saturating_sub(Self::retained_row_bytes(&row));
        }
    }

    /// Reports live and reusable history storage together for memory diagnostics.
    #[must_use]
    pub fn allocated_history_bytes(&self) -> usize {
        // Visible rows are guaranteed independently, as in the existing cap.
        self.history_bytes
            .saturating_add(self.multicell_history_overhead())
            .saturating_add(self.spare_bytes)
    }
}
