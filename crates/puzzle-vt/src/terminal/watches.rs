//! Registered source consumers watch writes, never producer-supplied coordinates.
//! Admission validates real retained gaps; reflow uses the shared tracked registry.
//! The normal parser selects its no-watch instantiation once per feed slice.
//!
//! A version alone cannot distinguish two identical rewrites at different
//! columns. A live range therefore observes actual writes at its boundaries,
//! and observes interior writes at row granularity. The sticky result survives
//! reflow; copied text is never used to guess whether a rewrite occurred.
//!
//! LineIds describe identity, not order: inserting a row can place a newer ID
//! before older surviving IDs. Optional circular order coordinates solve that
//! distinction without retaining a per-row membership list for every range.
//! Full-width LF keeps all survivors' coordinates; a region updates its moved
//! handles only. The compact coordinate occupies existing row-header padding.
//!
//! Source admission may cover tens of thousands of history rows. Initialization
//! is consequently resumable under the same caller-owned physical-visit credits
//! as inspection. No callback hides an unbounded setup loop. Direct trusted VT
//! callers have an explicit convenience entry point; OSC handlers use completed,
//! owner-scoped scans and the constant-time prepared entry point instead.
use super::Terminal;
use crate::{LineId, RangeWatch, RowFlags, WatchCounts, WatchState};

impl Terminal {
    /// Observes a validated half-open interval; `limit` bounds simultaneous owners.
    /// Empty/reversed/out-of-document coordinates are refused before allocating.
    pub fn watch_source(
        &mut self,
        start: (LineId, usize),
        end: (LineId, usize),
        limit: usize,
    ) -> Option<RangeWatch> {
        let ring = &self.screen().ring;
        // The convenience path accepts trusted coordinates, but still proves
        // both identities exist before any tracker or serial is consumed.
        let first = ring.index_of(start.0)?;
        let last = ring.index_of(end.0)?;
        // Trusted direct callers may prepare in one operation; protocol handlers
        // use the bounded preparation API and validated indices below instead.
        let mut next = 0;
        let mut visits = usize::MAX;
        self.prepare_source_watch_rows(&mut next, &mut visits);
        self.watch_source_prepared(start, end, (first, last), limit)
    }

    /// Assigns optional row-order coordinates under a caller's physical-visit cap.
    /// Parsing must remain paused; transforms cancel the caller's continuation.
    pub fn prepare_source_watch_rows(&mut self, next: &mut usize, visits: &mut usize) -> bool {
        // Once installed, row movement maintains the coordinates. Later owners
        // reuse that preparation instead of touching the entire history again.
        if self.write_clock.has_watches() {
            return true;
        }
        let alternate = self.active == super::ActiveScreen::Alternate;
        let ring = &mut self.screen_mut().ring;
        while *visits > 0 {
            // One indexed metadata visit consumes one credit, even though it
            // neither reads cell text nor changes the row's content version.
            let Some(row) = ring.retained_mut(*next) else {
                return true;
            };
            let Ok(order) = u32::try_from(*next) else {
                return false;
            };
            row.watch_order = order;
            row.flags.insert(RowFlags::WATCH_ORDER);
            row.flags.set(RowFlags::WATCH_ALTERNATE, alternate);
            *next += 1;
            *visits -= 1;
        }
        *next == ring.scrollback_len() + ring.visible_rows()
    }

    /// Installs a watch using indices already verified by a bounded source scan.
    /// This trusted seam validates IDs in constant time; PTY bytes never call it.
    pub fn watch_source_prepared(
        &mut self,
        start: (LineId, usize),
        end: (LineId, usize),
        indices: (usize, usize),
        limit: usize,
    ) -> Option<RangeWatch> {
        let (first, last) = indices;
        let ring = &self.screen().ring;
        // The bounded walker supplied these slots. Recheck their identities in
        // O(1), rather than restarting index_of's fallback scan during admission.
        let a = ring.retained(first)?;
        let b = ring.retained(last)?;
        if a.id != start.0
            || b.id != end.0
            || first > last
            || start.1 > ring.columns()
            || end.1 > ring.columns()
            || first == last && start.1 >= end.1
            || !a.flags.contains(RowFlags::WATCH_ORDER)
            || !b.flags.contains(RowFlags::WATCH_ORDER)
        {
            return None;
        }
        // A gap may equal width at pending wrap. The private tracker captures
        // its preceding cell/after affinity so a resize cannot clamp it away.
        let orders = (a.watch_order, b.watch_order);
        let alternate = self.active == super::ActiveScreen::Alternate;
        // All coordinate checks precede activation; the first witness must be
        // newer than every row-local version accumulated without observers.
        if !self.write_clock.can_watch(limit) {
            return None;
        }
        self.activate_write_clock();
        let watch =
            self.write_clock
                .watch(start, end, limit, orders, alternate, &mut self.tracked)?;
        let ring = &mut self.screen_mut().ring;
        for index in [first, last] {
            if let Some(row) = ring.retained_mut(index) {
                row.flags.insert(RowFlags::BOUNDARY_WATCH);
            }
        }
        Some(watch)
    }

    /// Returns sticky mutation state; an evicted endpoint cannot authorize use.
    pub fn source_watch(&self, watch: RangeWatch) -> Option<WatchState> {
        self.write_clock.state(watch, &self.tracked)
    }

    /// Current retained endpoint rows, using maintained watch order only.
    /// This cold query adds no bookkeeping to ordinary printing or row movement.
    pub fn source_watch_order(&self, watch: RangeWatch) -> Option<(usize, usize)> {
        let oldest = self.screen().ring.retained(0)?;
        if !oldest.flags.contains(RowFlags::WATCH_ORDER) {
            return None;
        }
        self.write_clock
            .order(watch, &self.tracked, oldest.watch_order)
    }
    /// Compare unchanged live ranges without scanning rows or assuming ordered IDs.
    pub fn source_range_relation(
        &self,
        first: RangeWatch,
        second: RangeWatch,
    ) -> Option<crate::RangeRelation> {
        // The oldest retained handle is the origin of the current deque.
        // Initialization visits every row before admitting the first watch.
        let oldest = self.screen().ring.retained(0)?;
        if !oldest.flags.contains(RowFlags::WATCH_ORDER) {
            return None;
        }
        self.write_clock
            .relation(first, second, &self.tracked, oldest.watch_order)
    }

    /// Releases one owner's tracked gaps; stale copies cannot resolve a later watch.
    pub fn release_source_watch(&mut self, watch: RangeWatch) {
        self.write_clock.release(watch, &mut self.tracked);
        // Conservative flags may remain until recycling; they never create a
        // column test for a row not named by another live watch's endpoints.
    }

    /// Actual optional table allocation, including capacity slack, charged globally.
    pub fn source_watch_storage_bytes(&self) -> usize {
        self.write_clock.watch_storage_bytes()
    }

    /// Constant retained entry size; shared capacity is reported separately above.
    pub fn source_watch_entry_bytes() -> usize {
        crate::WriteClock::watch_entry_bytes()
    }

    /// Reports optional mutation work for count proofs, never per-frame telemetry.
    pub fn source_watch_counts(&self) -> WatchCounts {
        self.write_clock.counts()
    }

    /// Content-preserving transforms remap boundaries without rescanning their text.
    pub(super) fn relocate_source_watches(&mut self) {
        if !self.write_clock.has_watches() {
            return;
        }
        let ring = match self.active {
            super::ActiveScreen::Primary => &self.primary.ring,
            super::ActiveScreen::Alternate => &self.alternate.ring,
        };
        self.write_clock.relocate(&self.tracked, ring);
        // New physical identities replace old row flags. Only the two ends of
        // each interval need exact columns; interior changes remain row-granular.
        let (screen, clock) = self.screen_clock_mut();
        for (index, row) in screen.ring.rows_mut().enumerate() {
            row.watch_order = index as u32;
            row.flags.insert(RowFlags::WATCH_ORDER);
            row.flags.remove(RowFlags::BOUNDARY_WATCH);
        }
        // Mark exactly two endpoints per watcher, avoiding rows × watchers work
        // on a large reflow. Interior rows never need a column-range predicate.
        for (first, last) in clock.boundary_indices() {
            for index in [first, last] {
                if let Some(row) = screen.ring.retained_mut(index) {
                    row.flags.insert(RowFlags::BOUNDARY_WATCH);
                }
            }
        }
    }

    /// Clears optional references before destructive clear/reset/screen transitions.
    pub(crate) fn clear_source_watches(&mut self) {
        self.write_clock.clear_watches(&mut self.tracked);
    }
}

#[cfg(test)]
mod tests;
