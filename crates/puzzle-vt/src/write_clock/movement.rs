//! Row-handle moves preserve intervals only when their membership stays intact.
//! A blank inserted between surviving endpoints changes source even though no
//! original endpoint was erased. Update both boundary coordinates first, then
//! compare each moved handle's old/new membership against those complete bounds.
//! Position coordinates may circle zero; stable identities themselves never wrap.
use super::WriteClock;
use crate::{LineId, Row, RowFlags};

impl WriteClock {
    /// Saves the original interval before a bounded band of handles is relocated.
    pub(crate) fn begin_move(&mut self) {
        if let Some(watches) = &mut self.watches {
            // A cached row classification describes the old coordinate space.
            // Discard it before a band can change any source membership.
            watches.last_interior_row = None;
            for entry in &mut watches.entries {
                // One old range per owner suffices; no per-moved-row log grows.
                entry.prior_orders = entry.orders;
            }
        }
    }

    /// Computes final boundaries without overwriting any row's old coordinate.
    pub(crate) fn moving_boundary(&mut self, line: LineId, order: u32) {
        if let Some(watches) = &mut self.watches {
            for entry in &mut watches.entries {
                // Stage one resolves both ends without observing writes or
                // exposing half-updated bounds to the membership comparison.
                if entry.state.start.0 == line {
                    entry.orders.0 = order;
                }
                if entry.state.end.0 == line {
                    entry.orders.1 = order;
                }
            }
        }
    }

    /// Updates a handle's optional coordinate and detects source entering/leaving.
    pub(crate) fn moved(&mut self, row: &mut Row, order: u32) {
        if let Some(watches) = &mut self.watches {
            watches.counts.row_moves = watches.counts.row_moves.saturating_add(1);
            let mut boundary = false;
            for entry in &mut watches.entries {
                boundary |= row.id == entry.state.start.0 || row.id == entry.state.end.0;
                watches.counts.membership_tests = watches.counts.membership_tests.saturating_add(1);
                // A recycled blank has no old membership. Moving it between
                // surviving endpoints must still invalidate exported source.
                let old = row.flags.contains(RowFlags::WATCH_ORDER)
                    && row.flags.contains(RowFlags::WATCH_ALTERNATE) == entry.alternate
                    && inside(row.watch_order, entry.prior_orders);
                let new = inside(order, entry.orders);
                // Preserve whole-range relocation, but never silently grow or
                // shrink its source because another row crossed a boundary.
                if old != new && !entry.state.changed {
                    entry.state.changed = true;
                    watches.counts.invalidations = watches.counts.invalidations.saturating_add(1);
                }
            }
            // Publish the summary only after all final range tests agree.
            row.flags.set(RowFlags::BOUNDARY_WATCH, boundary);
            // Fresh rows acquire the active document tag before future writes.
            // Old inactive-screen metadata cannot match this watch namespace.
            if let Some(entry) = watches.entries.first() {
                row.flags.set(RowFlags::WATCH_ALTERNATE, entry.alternate);
            }
        }
        row.watch_order = order;
        row.flags.insert(RowFlags::WATCH_ORDER);
    }
}

/// Circular coordinates have no identity meaning; distance handles deque wrap.
fn inside(order: u32, bounds: (u32, u32)) -> bool {
    order.wrapping_sub(bounds.0) <= bounds.1.wrapping_sub(bounds.0)
}
