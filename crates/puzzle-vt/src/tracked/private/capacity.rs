//! Stage private tracker growth before transferring ownership (M19.3 ledger).
//! Existing handles/serials are copied as inert planning data. Only a successful
//! shared admission replaces the pool; refusal drops the proposal. Both old and
//! proposed allocations count during preparation, including spare release slots.
//!
//! Slots are never compacted: receipts refer to stable slot indices and reuse
//! generations. Dense per-screen lists may move, but their reverse indices must
//! be copied together with the slots. Keeping the previous capacities prevents
//! one growing screen from discarding the other screen's warmed allocation.
//! No positions are published, remapped or observed during this reservation.
//! The caller must hold exclusive ownership until it commits or drops the plan.
use super::{Arena, TrackedPositions};

/// The optional arena is the only owner permitted to adopt a staged pool.
impl TrackedPositions {
    /// Prepare enough slots for `additional` simultaneous private acquisitions.
    /// None is allocation refusal; an empty proposal means warmed capacity fits.
    pub(crate) fn prepare_private_capacity(
        &self,
        additional: usize,
        maximum: usize,
    ) -> Option<(Option<Box<Arena>>, usize)> {
        let old = self.private.as_deref();
        let current = self.private_storage_bytes();
        // Released slots are reusable; Gone-but-owned slots are not free.
        // Generation-exhausted slots are permanently retired, so only the
        // actual free list, never occupied-count subtraction, permits reuse.
        // Such retired capacity remains part of the retained pool charge.
        let slots = match old {
            Some(arena) => arena
                .positions
                .positions
                .len()
                .checked_add(additional.saturating_sub(arena.positions.free.len()))?,
            None => additional,
        };
        // All new captures belong to the currently selected screen. Preserve
        // the inactive list without reserving fictitious captures on both sides.
        let screen = self.screen_index();
        let live = match old {
            Some(arena) => arena.positions.live[screen].len().checked_add(additional)?,
            None => additional,
        };
        if additional == 0
            || old.is_some_and(|a| {
                a.positions.positions.capacity() >= slots
                    && a.positions.free.capacity() >= a.positions.positions.capacity()
                    && a.positions.live[screen].capacity() >= live
            })
        {
            return Some((None, current));
        }
        // Preflight the minimum peak before allocation. Actual capacities are
        // measured again afterward, as Vec may reserve more than requested.
        let minimum = minimum_pool_bytes(old, slots, live, screen)?;
        if current.checked_add(minimum)? > maximum {
            return None;
        }
        let proposed = self.stage_private_pool(old, slots, live)?;
        // The old pool stays alive while the detached copy is validated. Do not
        // subtract it early merely because commit will eventually replace it.
        let peak = current.checked_add(storage(&proposed))?;
        if peak > maximum {
            return None;
        }
        Some((Some(proposed), peak))
    }

    /// Construct an inert proposed engine; validation/commit remain separate.
    fn stage_private_pool(
        &self,
        old: Option<&Arena>,
        slots: usize,
        live: usize,
    ) -> Option<Box<Arena>> {
        let screen = self.screen_index();
        // This detached engine cannot be queried externally. Moving it into
        // the owner only after the full ledger fits preserves every generation.
        let mut proposed = Box::new(Arena {
            positions: TrackedPositions {
                active: self.active,
                ..Default::default()
            },
            occupied: old.map_or(0, |a| a.occupied),
        });
        // Copying only this private engine does not copy public search/selection
        // slots. Its nested private arena stays absent, so ownership cannot recurse.
        let target = &mut proposed.positions;
        let existing = old.map(|a| &a.positions);
        // Slot indices and generations are copied unchanged; neither the free
        // list nor the dense membership list is a new handle-issuance source.
        copy_into(&mut target.positions, existing.map(|p| &p.positions), slots)?;
        // Release cannot allocate later, even if every slot is retired at once.
        let release = target.positions.capacity();
        copy_into(&mut target.free, existing.map(|p| &p.free), release)?;
        // Every live-list position and slot reverse index must agree after the
        // move. Capacity is retained even on an inactive or currently empty side.
        for index in 0..2 {
            let needed = if index == screen { live } else { 0 };
            copy_into(
                &mut target.live[index],
                existing.map(|p| &p.live[index]),
                needed,
            )?;
        }
        Some(proposed)
    }

    /// Commit is infallible and leaves copied handle generations unchanged.
    pub(crate) fn commit_private_capacity(&mut self, proposed: Option<Box<Arena>>) {
        if let Some(proposed) = proposed {
            self.private = Some(proposed);
        }
    }
}

/// Actual capacities, not requested counts, determine the proposal's charge.
fn storage(arena: &Arena) -> usize {
    size_of::<Arena>()
        + arena.positions.positions.capacity() * size_of::<super::super::Slot>()
        + (arena.positions.free.capacity()
            + arena
                .positions
                .live
                .iter()
                .map(Vec::capacity)
                .sum::<usize>())
            * size_of::<usize>()
}

/// Rare reservation preserves old high-water capacities and exact slot contents.
fn copy_into<T: Clone>(target: &mut Vec<T>, old: Option<&Vec<T>>, needed: usize) -> Option<()> {
    // reserve_exact can expose a larger actual capacity; storage() bills that
    // result. The requested size is never used as a retained-byte receipt.
    let capacity = needed.max(old.map_or(0, Vec::capacity));
    target.try_reserve_exact(capacity).ok()?;
    // No user callback sees the proposal, so these copied values are inert
    // until the original owner atomically transfers the prepared engine.
    if let Some(old) = old {
        target.extend_from_slice(old);
    }
    Some(())
}

/// Requested capacities give a no-allocation lower bound on the staged peak.
fn minimum_pool_bytes(
    old: Option<&Arena>,
    slots: usize,
    live: usize,
    screen: usize,
) -> Option<usize> {
    // A proposal preserves old high-water capacity even when few handles are
    // currently occupied. Discounting that slack would underbill the move.
    let positions = old.map(|a| &a.positions);
    let slots = slots.max(positions.map_or(0, |p| p.positions.capacity()));
    // Any slot may be released later. Reserve the complete release-list bound
    // now rather than allocating while a failed metadata candidate is retired.
    let free = slots.max(positions.map_or(0, |p| p.free.capacity()));
    // Present anchors participate in exactly one screen's dense live list.
    // Inactive storage stays intact, but only the active side needs new entries.
    let mut lists = 0usize;
    for index in 0..2 {
        let requested = if index == screen { live } else { 0 };
        lists =
            lists.checked_add(requested.max(positions.map_or(0, |p| p.live[index].capacity())))?;
    }
    // The arena header is a real Box allocation in addition to all three
    // vector families. Every multiplication/addition refuses usize overflow.
    size_of::<Arena>()
        .checked_add(slots.checked_mul(size_of::<super::super::Slot>())?)?
        .checked_add(free.checked_add(lists)?.checked_mul(size_of::<usize>())?)
}
