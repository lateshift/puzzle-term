//! Allocate a detached watch table before the caller commits its shared ledger.
//! Entries own trackers and must never be cloned. The proposal reserves only
//! empty slots; commit moves the existing entries without duplicating ownership.
//!
//! Empty reserved storage is not an active watch. The parser's once-per-slice
//! selection tests live entries, preserving its ordinary monomorphised loop.
//! Failed proposals cannot consume a watch serial or capture private markers.
//! Watch and marker proposals are admitted together by Terminal before either
//! owner changes; the returned peak deliberately still includes the old table.
use super::{WriteClock, watches};

/// Watch slots may be reserved without installing a live mutation observer.
impl WriteClock {
    /// Return optional staged storage and the old-plus-proposed peak charge.
    pub(crate) fn prepare_watch_capacity(
        &self,
        additional: usize,
        maximum: usize,
    ) -> Option<(Option<Box<watches::Watches>>, usize)> {
        let current = self.watch_storage_bytes();
        let old = self.watches.as_deref();
        let needed = match old {
            Some(watches) => watches.entries.len().checked_add(additional)?,
            None => additional,
        };
        if additional == 0 || old.is_some_and(|w| w.entries.capacity() >= needed) {
            return Some((None, current));
        }
        // The requested minimum must fit before allocating; the measured
        // capacity is checked again before the proposal is returned.
        let minimum = size_of::<watches::Watches>()
            .checked_add(needed.checked_mul(size_of::<watches::Entry>())?)?;
        if current.checked_add(minimum)? > maximum {
            return None;
        }
        // New slots carry no tracker ownership. Existing entries remain live
        // and queryable until commit, even if a later peer reservation refuses.
        let mut proposed = Box::<watches::Watches>::default();
        proposed.entries.try_reserve_exact(needed).ok()?;
        // Capacity, not entry count, includes every unused reserved slot.
        // The boxed table header is billed even before its first live watch.
        let proposed_bytes = size_of::<watches::Watches>()
            + proposed.entries.capacity() * size_of::<watches::Entry>();
        let peak = current.checked_add(proposed_bytes)?;
        (peak <= maximum).then_some((Some(proposed), peak))
    }

    /// Move entry ownership only after all peer owners have admitted their peaks.
    pub(crate) fn commit_watch_capacity(&mut self, proposed: Option<Box<watches::Watches>>) {
        if let Some(mut proposed) = proposed {
            // append moves existing entries into already reserved space. It
            // cannot duplicate a tracker or grow after the shared admission.
            if let Some(mut old) = self.watches.take() {
                proposed.entries.append(&mut old.entries);
                proposed.counts = old.counts;
                proposed.last_interior_row = old.last_interior_row;
            }
            self.watches = Some(proposed);
        }
    }
}
