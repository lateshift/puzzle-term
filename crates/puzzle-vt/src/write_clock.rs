//! Terminal-wide content ordering, independent of time and physical row IDs.
//!
//! Unobserved mutations update only their row-local versions. Activation rebases
//! above both documents before the first source witness; releasing the last
//! observer/watch restores that unused path (user decision 2026-10-16).
//! A source interval can remember one stamp instead of every interior row's
//! version: a later write necessarily has a greater stamp. Reflow inherits
//! source stamps because moving content is not a write. This owner contains no
//! timer, synchronization or protocol state, and is shared by borrowing only.
//!
//! The clock and watch identities have different lifetimes. Releasing a watch
//! never rewinds either sequence; otherwise a stale caller could resolve a new
//! interval under a recycled identity. Reset clears source references while
//! leaving the Terminal's mutation sequence intact for cache comparisons.
//!
//! Watch allocation is optional and lazy. The selected plain print loop does
//! not inspect the boxed table, while cold edits use the same write observer.
//! A watch owner supplies its admission cap; this generic layer does not know
//! which optional protocol or application requested the interval.
//!
//! Relocation preserves a sticky write result before rebasing the cutoff.
//! Destination rows may inherit a boundary row's later outside write through
//! maximum-source reflow. That inherited stamp is not a new inside write, so
//! precise observation, rather than comparing final contents, preserves intent.
//! Tracked gaps provide eviction and reflow affinity; order coordinates provide
//! membership after region scrolling has reordered otherwise stable LineIds.
//!
//! Storage reporting distinguishes one fixed entry from actual table capacity.
//! A consumer charges the shared allocation once, including spare Vec slots,
//! and separately charges its own witness values and retained semantic data.

/// Optional precise mutation tracking shares this sequence owner.
mod watches;
pub use watches::{RangeWatch, WatchCounts, WatchState};

/// One terminal's never-reused content mutation sequence, including both screens.
/// Standalone row/ring callers own one clock for their complete document too.
#[derive(Debug, Default)]
pub struct WriteClock {
    /// The sole content clock for both screens.
    value: u64,
    /// Global stamping is needed only while a source consumer or watch exists.
    active: bool,
    /// Explicit source owners (including selections) share activation.
    observers: usize,
    /// A released receipt is never reused by this Terminal.
    next_watch: u64,
    /// No watch storage exists on an unregistered/no-watch path.
    watches: Option<Box<watches::Watches>>,
}

impl WriteClock {
    /// Last globally assigned stamp; zero can also mean observation never activated.
    #[must_use]
    pub fn latest(&self) -> u64 {
        self.value
    }

    /// Records one mutation, compiling optional watch work out of the plain loop.
    #[inline]
    pub(crate) fn written<const WATCH: bool>(
        &mut self,
        row: &crate::Row,
        columns: std::ops::Range<usize>,
    ) -> u64 {
        #[cfg(test)]
        crate::print_work::note(crate::print_work::Operation::Version);
        if !self.active {
            // Ordinary rendering only needs a changing row-local cache key.
            // INVARIANT: a row cannot receive 2^64 writes in a practical lifetime.
            assert!(row.version < u64::MAX, "row version exhausted");
            return row.version + 1;
        }
        if WATCH && let Some(watches) = &mut self.watches {
            watches.written(row, columns);
        }
        self.next()
    }

    /// Assigns one unique version to an actual cell or row-metadata mutation.
    ///
    /// # Panics
    /// Only on internal sequence exhaustion. Even one billion writes each second
    /// would require over 584 years without destroying this Terminal. Hostile
    /// input cannot set the counter; RIS intentionally preserves its lifetime.
    #[inline]
    pub(crate) fn next(&mut self) -> u64 {
        // INVARIANT: no practical Terminal can execute 2^64 mutations; refuse
        // the theoretical exhaustion explicitly rather than alias an old stamp.
        assert!(self.value < u64::MAX, "terminal write clock exhausted");
        #[cfg(test)]
        crate::print_work::note(crate::print_work::Operation::ClockStore);
        self.value += 1;
        self.value
    }
}

/// Watch admission and lifetime are cold, caller-bounded operations.
impl WriteClock {
    /// Refused watches must not activate stamping or consume an identity.
    pub(crate) fn can_watch(&self, limit: usize) -> bool {
        self.next_watch != u64::MAX && self.watches.as_ref().map_or(0, |w| w.entries.len()) < limit
    }

    /// Installs a validated interval without growing past the supplied live cap.
    pub(crate) fn watch(
        &mut self,
        start: (crate::LineId, usize),
        end: (crate::LineId, usize),
        limit: usize,
        orders: (u32, u32),
        alternate: bool,
        tracked: &mut crate::TrackedPositions,
    ) -> Option<RangeWatch> {
        if !self.can_watch(limit) {
            return None;
        }
        // A refused admission consumes no identity or tracker. Once accepted,
        // the monotonically issued receipt cannot alias a released predecessor.
        self.next_watch += 1;
        let id = RangeWatch(self.next_watch);
        let state = WatchState {
            start,
            end,
            clock: self.value,
            changed: false,
        };
        let watches = self.watches.get_or_insert_with(Default::default);
        watches.last_interior_row = None;
        watches
            .entries
            .push(watches::Entry::new(id, state, orders, alternate, tracked));
        Some(id)
    }

    /// Retires source references; empty storage is freed rather than entering the loop.
    pub(crate) fn release(&mut self, id: RangeWatch, tracked: &mut crate::TrackedPositions) {
        if let Some(watches) = &mut self.watches
            && let Some(index) = watches.entries.iter().position(|entry| entry.id == id)
        {
            // Entries own both tracker references; swap-removal moves ownership
            // without changing a survivor's opaque receipt identity.
            watches.entries.swap_remove(index).release(tracked);
            watches.last_interior_row = None;
            if watches.entries.is_empty() {
                self.watches = None;
            }
            self.refresh_activity();
        }
    }

    /// Clears every reference on a destructive source or screen transition.
    pub(crate) fn clear_watches(&mut self, tracked: &mut crate::TrackedPositions) {
        if let Some(watches) = self.watches.take() {
            for entry in watches.entries {
                entry.release(tracked);
            }
        }
        self.refresh_activity();
    }
}

/// Constant-time absence checks and cold lookup/remapping are separate from stores.
impl WriteClock {
    /// The parser makes this choice once outside its monomorphised byte loop.
    pub(crate) fn has_watches(&self) -> bool {
        self.watches.as_ref().is_some_and(|w| !w.entries.is_empty())
    }
    /// Resolves only this Terminal's live, never-reused receipt identity.
    pub(crate) fn state(
        &self,
        id: RangeWatch,
        tracked: &crate::TrackedPositions,
    ) -> Option<WatchState> {
        self.watches
            .as_ref()?
            .entries
            .iter()
            .find(|entry| entry.id == id)?
            .state(tracked)
    }
    /// Optional counter storage does not exist before watch registration.
    pub(crate) fn counts(&self) -> WatchCounts {
        self.watches
            .as_ref()
            .map_or(WatchCounts::default(), |w| w.counts)
    }
    /// Every prior write was tracked; remap and rebase without a physical verification scan.
    pub(crate) fn relocate(
        &mut self,
        tracked: &crate::TrackedPositions,
        ring: &crate::ring::RowRing,
    ) {
        if let Some(watches) = &mut self.watches {
            watches.last_interior_row = None;
            for entry in &mut watches.entries {
                entry.relocate(tracked, self.value);
                if let (Some(start), Some(end)) = (
                    ring.index_of(entry.state.start.0),
                    ring.index_of(entry.state.end.0),
                ) {
                    // A reflow supplies a new physical order for the same text.
                    // Both endpoints are resolved before publishing that order.
                    entry.orders = (start as u32, end as u32);
                    entry.prior_orders = entry.orders;
                } else {
                    entry.state.changed = true;
                }
            }
        }
    }
    /// After reflow, order keys are verified document indices for two boundary rows.
    pub(crate) fn boundary_indices(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.watches
            .iter()
            .flat_map(|w| w.entries.iter())
            .map(|entry| (entry.orders.0 as usize, entry.orders.1 as usize))
    }
}

/// Source membership is reconciled atomically across a moved row band.
mod movement;

/// Storage receipts let optional consumers charge actual capacities globally.
impl WriteClock {
    /// Actual heap owned by the optional watch table; row data is existing VT storage.
    pub(crate) fn watch_storage_bytes(&self) -> usize {
        self.watches.as_ref().map_or(0, |w| {
            size_of::<watches::Watches>() + w.entries.capacity() * size_of::<watches::Entry>()
        })
    }
    /// Fixed tracked record size, independent of row count and line length.
    pub(crate) fn watch_entry_bytes() -> usize {
        size_of::<watches::Entry>()
    }
}

/// Optional allocations are staged under a shared consumer ledger.
mod capacity;

/// Source interval comparisons share the maintained row-handle order.
mod relations;
pub use relations::RangeRelation;

/// Cold activation never changes existing row-cache keys or allocates storage.
impl WriteClock {
    /// Rebase before taking a witness, including versions accumulated while paused.
    pub(crate) fn activate(&mut self, maximum: u64) {
        if !self.active {
            // INVARIANT: exhaustion is unreachable at practical mutation rates;
            // wrapping would let a later rewrite precede an existing witness.
            assert!(
                maximum.max(self.value) < u64::MAX,
                "terminal write clock exhausted"
            );
            self.value = maximum.max(self.value) + 1;
            self.active = true;
        }
    }

    /// A registered source owner keeps stamping even between its live watches.
    pub(crate) fn observe(&mut self) {
        // INVARIANT: simultaneous owners require memory, so usize cannot exhaust.
        assert!(
            self.observers < usize::MAX,
            "source observer count exhausted"
        );
        self.observers += 1;
    }

    /// Removing the last owner restores row-local stamps unless a watch remains.
    pub(crate) fn unobserve(&mut self) {
        if self.observers != 0 {
            self.observers -= 1;
        }
        self.refresh_activity();
    }

    /// Cold lifetime transitions alone change whether mutations store the clock.
    fn refresh_activity(&mut self) {
        self.active = self.observers != 0 || self.has_watches();
    }

    /// Avoid scanning either document while an existing consumer keeps it ordered.
    pub(crate) fn active(&self) -> bool {
        self.active
    }
}
