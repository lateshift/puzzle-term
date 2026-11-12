//! Optional source-order metadata follows owned row handles, never cell copies.
//!
//! A full-width scroll preserves all surviving coordinates. Inserting inside a
//! top-origin band shifts just its visible footer; the history prefix stays put.
//! The two-phase move first fixes boundary coordinates and then checks membership,
//! so a new blank between surviving endpoints cannot look content-preserving.
//!
//! Reflow usually relocates text without a new write. An oversized rectangular
//! block is the exception: atomic placement drops it. Notify its source columns
//! before packing discards the old rows, preserving sticky invalidation through
//! the subsequent coordinate/cutoff rebase. No watch means no added block walk.
use super::RowRing;
use crate::Row;

/// These cold helpers are used only by the singular VT/source owner.
impl RowRing {
    /// Cold optional source metadata updates reuse the ring's indexed row owner.
    pub(crate) fn retained_mut(&mut self, index: usize) -> Option<&mut Row> {
        self.rows.get_mut(index).map(Box::as_mut)
    }

    /// Cold source transforms update row metadata without exposing cell copies.
    pub(crate) fn rows_mut(&mut self) -> impl Iterator<Item = &mut Row> {
        self.rows.iter_mut().map(Box::as_mut)
    }

    /// Insertions shift only the visible footer, never retained history rows.
    pub(super) fn move_watch_orders(&mut self, first: usize, clock: &mut crate::WriteClock) {
        if !clock.has_watches() {
            return;
        }
        let origin = self.rows.front().map(|row| row.watch_order).unwrap_or(0);
        clock.begin_move();
        for (index, row) in self.rows.iter().enumerate().skip(first) {
            clock.moving_boundary(row.id, origin.wrapping_add(index as u32));
        }
        for (index, row) in self.rows.iter_mut().enumerate().skip(first) {
            clock.moved(row, origin.wrapping_add(index as u32));
        }
    }

    /// Observes rectangles that cannot survive atomic reflow at the new geometry.
    /// Only optional watchers need this pre-transform notice; the ordinary path
    /// still visits each block exactly once in the existing reflow builder.
    pub(crate) fn watch_reflow_discards(
        &mut self,
        columns: usize,
        height: usize,
        clock: &mut crate::WriteClock,
    ) {
        if !clock.has_watches() {
            return;
        }
        let Some(store) = self.multicells.as_deref() else {
            return;
        };
        for (_, block) in store.iter() {
            if block.columns <= columns && usize::from(block.sizing.scale) <= height {
                continue;
            }
            for fragment in &block.fragments {
                if let Some(index) = self.index_of(fragment.line) {
                    let row = &mut self.rows[index];
                    // Reflow is atomic. Record actual text loss before source
                    // rows disappear, while precise boundary columns still exist.
                    row.version =
                        clock.written::<true>(row, block.column..block.column + block.columns);
                }
            }
        }
    }
}
