//! Full-width region scrolling rotates each row once per CSI operation.
//! Repeating a single-line rotation N times costs region-height × N row moves.
//! A cycle rotation costs only region-height swaps, then recycles the discarded
//! N rows. This works across the deque wrap without making history contiguous.
//! Every survivor keeps its LineId, extras and allocation. Only discarded rows
//! get new identities; footer rows and retained history are never rotated.
//! Columns never move independently here: this path is selected only when
//! left/right margins cover the entire width. Rectangle scrolling retains its
//! existing cell-copy path because row identities cannot follow partial columns.
//! Tracked positions resolve survivors by identity, not their former screen row.
//! Recycled cells use the current background, preserving the terminfo BCE contract.
//! The operation's count is clamped to its height before any index arithmetic.
//! A zero count leaves both versions and identities untouched.
//! Raw deque slots may be split into two physical slices after prior scrolling.
//! All swaps use logical indices, so no contiguous-history copy is required.
//! Optional source watches use a separate order coordinate because newer IDs
//! can precede older surviving rows after this rotation. All final endpoint
//! coordinates are computed before comparing moved handles against the range.
//! This catches a new blank entering between live endpoints, even if the
//! discarded row was outside that range. Whole-source movement preserves it.
//! These metadata passes run only with live watches and only over this band;
//! their work never depends on column count or retained history length.
//! The ordinary no-watch path performs no coordinate or membership visits.
use super::*;

/// Row-wide scrolling shares one identity-preserving implementation.
impl RowRing {
    /// Single-line compatibility entry point shares the bounded batch operation.
    pub fn scroll_region_up(
        &mut self,
        top: usize,
        bottom: usize,
        tracked: &mut TrackedPositions,
        background: PackedColor,
        clock: &mut crate::WriteClock,
    ) {
        self.scroll_region(top, bottom, 1, true, tracked, background, clock);
    }

    /// Reverse index and one inserted line use the same downward batch operation.
    pub fn scroll_region_down(
        &mut self,
        top: usize,
        bottom: usize,
        tracked: &mut TrackedPositions,
        background: PackedColor,
        clock: &mut crate::WriteClock,
    ) {
        self.scroll_region(top, bottom, 1, false, tracked, background, clock);
    }

    /// Rotates a region by a bounded count without adding discarded rows to history.
    /// Empty or invalid regions are ignored, as for the single-line entry points.
    #[allow(
        clippy::too_many_arguments,
        reason = "region coordinates, direction and identity owner are independent"
    )]
    pub(crate) fn scroll_region(
        &mut self,
        top: usize,
        bottom: usize,
        count: usize,
        upward: bool,
        tracked: &mut TrackedPositions,
        background: PackedColor,
        clock: &mut crate::WriteClock,
    ) {
        if top > bottom || bottom >= self.visible_rows {
            return;
        }
        // Inclusive terminal margins become one positive half-open rotation span.
        let length = bottom - top + 1;
        let count = count.min(length);
        if count == 0 {
            return;
        }
        // History remains ahead of the visible grid and is never a discarded band.
        let base = self.scrollback_len() + top;
        // Downward movement is the complementary left rotation. A full-region
        // count has no surviving row and therefore requires no swaps.
        let shift = if upward { count } else { length - count } % length;
        let order_origin = self.rows.front().map(|row| row.watch_order).unwrap_or(0);
        self.rotate_rows(base, length, shift);
        // Rotation puts the discarded rows at the vacated side of the band.
        let first = if upward { length - count } else { 0 };
        for step in 0..count {
            // Downward blank IDs match repeated RI: the last displaced row is
            // recycled first, so the newest ID appears at the region's top.
            let index = if upward {
                first + step
            } else {
                first + count - 1 - step
            };
            if let Some(row) = self.rows.get_mut(base + index) {
                // The old identity becomes Gone before the recycled allocation
                // is assigned the next monotonic identity and BCE cells.
                if let Some(store) = &mut self.multicells {
                    store.retire(row.id, &row.blocks);
                }
                tracked.evict(row.id);
                row.recycle(LineId(self.next_id), background, clock);
                self.next_id = self.next_id.saturating_add(1);
            }
        }
        self.move_region_watches(base, length, order_origin, clock);
    }

    /// Updates only moved handles, with no history or cell traversal.
    fn move_region_watches(
        &mut self,
        base: usize,
        length: usize,
        origin: u32,
        clock: &mut crate::WriteClock,
    ) {
        if clock.has_watches() {
            clock.begin_move();
            for index in base..base + length {
                clock.moving_boundary(self.rows[index].id, origin.wrapping_add(index as u32));
            }
            for index in base..base + length {
                clock.moved(&mut self.rows[index], origin.wrapping_add(index as u32));
            }
        }
    }

    /// Swaps disjoint rotation cycles, preserving deque fragments and cell vectors.
    fn rotate_rows(&mut self, base: usize, length: usize, shift: usize) {
        if shift == 0 {
            return;
        }
        // A whole alternate grid has no fixed footer or history. The deque's
        // native rotation advances its head (constant time when full), avoiding
        // swaps of every row's inline rare metadata on full-screen SU/SD.
        if base == 0 && length == self.rows.len() {
            #[cfg(test)]
            crate::scroll_operations::rotating(&self.rows, shift);
            self.rows.rotate_left(shift);
            return;
        }
        // A band entirely within either physical deque fragment can use the
        // standard slice rotation, which moves row records in larger blocks.
        // Wrapped bands retain the index-cycle path below; neither copies history.
        let (left, right) = self.rows.as_mut_slices();
        if base + length <= left.len() {
            #[cfg(test)]
            crate::scroll_operations::moved(left[base..base + length].iter().map(Box::as_ref));
            left[base..base + length].rotate_left(shift);
            return;
        }
        if base >= left.len() {
            let start = base - left.len();
            #[cfg(test)]
            crate::scroll_operations::moved(right[start..start + length].iter().map(Box::as_ref));
            right[start..start + length].rotate_left(shift);
            return;
        }
        let (mut a, mut b) = (length, shift);
        // The greatest common divisor is exactly the number of disjoint cycles.
        while b != 0 {
            (a, b) = (b, a % b);
        }
        // Cycles visit disjoint rows. One pivot owner travels through its cycle;
        // the remaining owners each move into the preceding vacated position.
        // Counting every exchange below retains the pivot's repeated work, while
        // distinct identities show how many row allocations participate overall.
        for start in 0..a {
            let mut current = start;
            loop {
                // Modular logical indices also handle a physically wrapped deque.
                let next = (current + shift) % length;
                if next == start {
                    break;
                }
                // Row ownership moves intact: no cluster string or cell vector is cloned.
                #[cfg(test)]
                crate::scroll_operations::exchanged(
                    &self.rows[base + current],
                    &self.rows[base + next],
                );
                self.rows.swap(base + current, base + next);
                current = next;
            }
        }
    }
}
