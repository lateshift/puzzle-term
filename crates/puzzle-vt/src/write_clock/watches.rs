//! Opt-in range mutation receipts distinguish identical rewrites from relocation.
//! Two tracked gaps and one sticky changed bit suffice for arbitrarily long text.
//! Interior writes are observed at row granularity; only boundary rows need a
//! column intersection. Observing writes also permits a lossless reflow rebase
//! without scanning source rows again or comparing their text.
//! A repeated interior-row write cannot add information after the first write
//! invalidated every covering interior range. One row classification is cached.
//! Admission, release and movement invalidate that cache; precise boundary rows
//! keep testing columns because an earlier outside write must not mask a later
//! inside write. No per-scalar string, hash or allocation is needed.
use crate::{LineId, PrivateTrackedPos, Row, RowFlags, TrackedPositions, tracked::PositionState};
use std::ops::Range;

/// Opaque, never-reused identity in one Terminal's optional watch owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeWatch(pub(super) u64);

/// Additional mutation work, measured only while a range is watched.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WatchCounts {
    /// One boundary-row summary test per observed mutation.
    pub row_tests: u64,
    /// Repeated writes on a previously classified non-boundary row.
    pub row_cache_hits: u64,
    /// Entries whose row identity was tested; no per-cell source scan occurs.
    pub interval_tests: u64,
    /// Exact column intersections, only on flagged boundary rows.
    pub column_tests: u64,
    /// Handles receiving new optional coordinates, never cell-array moves.
    pub row_moves: u64,
    /// Old/new source-membership checks performed during handle movement.
    pub membership_tests: u64,
    /// Live watches becoming changed for the first time.
    pub invalidations: u64,
}

/// Constant-size source proof returned to a registered consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WatchState {
    /// Current tracked half-open source start.
    pub start: (LineId, usize),
    /// Current tracked half-open source end.
    pub end: (LineId, usize),
    /// Content-preserving relocation rebases this cutoff after observed writes.
    pub clock: u64,
    /// A write intersected the source, including an identical rewrite.
    pub changed: bool,
}

/// A tracked gap uses the preceding cell when the gap follows actual text.
#[derive(Debug)]
struct Gap {
    /// Shared VT registry owns movement, clipping and eviction semantics.
    handle: PrivateTrackedPos,
    /// Offset after that cell; avoids clamping an end-of-row gap on reflow.
    after: bool,
}

/// One source range, independent of its physical row count.
#[derive(Debug)]
pub(crate) struct Entry {
    /// Never aliases another entry after release or reset.
    pub id: RangeWatch,
    /// First included cell, tracked through reflow.
    start: Gap,
    /// Excluded endpoint, tracked with its affinity.
    end: Gap,
    /// Cached positions and sticky mutation result, used by the print loop.
    pub state: WatchState,
    /// Position keys follow row-handle moves; LineIds themselves are not ordered
    /// after IL/DL/region rotations. No per-row membership list is retained.
    pub orders: (u32, u32),
    /// One bounded row move uses old and final boundaries, never partial updates.
    pub prior_orders: (u32, u32),
    /// The two screens have independent positions and overlapping LineIds.
    pub alternate: bool,
}

/// Allocated only when an explicitly registered source consumer installs a watch.
#[derive(Debug, Default)]
pub(crate) struct Watches {
    /// Capacity is bounded by caller admission, not by output volume.
    pub entries: Vec<Entry>,
    /// Count proof of optional overhead; ordinary printing never touches it.
    pub counts: WatchCounts,
    /// All interior watches were invalidated on this row's first write. Further
    /// writes cannot add information until admission/movement changes membership.
    /// The screen tag distinguishes equal LineIds in the two independent buffers.
    pub last_interior_row: Option<(LineId, bool)>,
}

impl Gap {
    /// Captures a real cell with end-gap affinity, without changing the cursor.
    fn capture(point: (LineId, usize), end: bool, tracked: &mut TrackedPositions) -> Self {
        let after = end && point.1 > 0;
        Self {
            handle: tracked.track_private(point.0, point.1 - usize::from(after)),
            after,
        }
    }
    /// Converts the registry's remapped cell back into a half-open gap.
    fn resolve(&self, tracked: &TrackedPositions) -> Option<(LineId, usize)> {
        match tracked.get_private(self.handle) {
            PositionState::Present { line, column } => {
                Some((line, column + usize::from(self.after)))
            }
            PositionState::Gone => None,
        }
    }
}

impl Entry {
    /// Installs both gap references only after the owner validates the interval.
    pub(super) fn new(
        id: RangeWatch,
        state: WatchState,
        orders: (u32, u32),
        alternate: bool,
        tracked: &mut TrackedPositions,
    ) -> Self {
        Self {
            id,
            start: Gap::capture(state.start, true, tracked),
            end: Gap::capture(state.end, true, tracked),
            state,
            orders,
            prior_orders: orders,
            alternate,
        }
    }
    /// An evicted tracked cell permanently makes this watch unavailable.
    pub(super) fn state(&self, tracked: &TrackedPositions) -> Option<WatchState> {
        self.start.resolve(tracked)?;
        self.end.resolve(tracked)?;
        Some(self.state)
    }

    /// Releases both live registry references when the optional owner retires it.
    pub(super) fn release(self, tracked: &mut TrackedPositions) {
        tracked.release_private(self.start.handle);
        tracked.release_private(self.end.handle);
    }
    /// Remaps content without clearing a prior write or checking text equality.
    pub(super) fn relocate(&mut self, tracked: &TrackedPositions, clock: u64) {
        if let (Some(start), Some(end)) = (self.start.resolve(tracked), self.end.resolve(tracked)) {
            self.state.start = start;
            self.state.end = end;
            // Every intervening write was already observed. Max-source stamps
            // from a merged boundary row therefore cannot manufacture a rewrite.
            self.state.clock = clock;
        } else {
            self.state.changed = true;
        }
    }
}

impl Watches {
    /// Observes a write; interior membership is classified once per written row.
    pub(super) fn written(&mut self, row: &Row, columns: Range<usize>) {
        self.counts.row_tests = self.counts.row_tests.saturating_add(1);
        let boundary = row.flags.contains(RowFlags::BOUNDARY_WATCH);
        let key = (row.id, row.flags.contains(RowFlags::WATCH_ALTERNATE));
        if self.last_interior_row == Some(key) {
            self.counts.row_cache_hits = self.counts.row_cache_hits.saturating_add(1);
            return;
        }
        let mut precise = false;
        for entry in &mut self.entries {
            self.counts.interval_tests = self.counts.interval_tests.saturating_add(1);
            precise |= entry.observe(row, boundary, &columns, &mut self.counts);
        }
        // Boundary writes can miss now and intersect later, so only an entirely
        // row-granular classification may skip subsequent column decisions.
        // Admission and all coordinate transforms explicitly clear this cache.
        // A missing/changed owner therefore cannot suppress a new owner's writes.
        self.last_interior_row = (!precise).then_some(key);
    }
}

/// Intersection and sticky invalidation are shared by all mutation families.
impl Entry {
    /// Returns whether later writes on this row still need exact column checks.
    fn observe(
        &mut self,
        row: &Row,
        boundary: bool,
        columns: &Range<usize>,
        counts: &mut WatchCounts,
    ) -> bool {
        let state = &mut self.state;
        // Coordinates are circular deque positions, not identities/counters.
        // Screen tags exclude stale metadata left on the inactive document.
        if !row.flags.contains(RowFlags::WATCH_ORDER)
            || state.changed
            || row.flags.contains(RowFlags::WATCH_ALTERNATE) != self.alternate
            || row.watch_order.wrapping_sub(self.orders.0)
                > self.orders.1.wrapping_sub(self.orders.0)
        {
            return false;
        }
        let (first, last) = (state.start.0, state.end.0);
        let precise = boundary && (row.id == first || row.id == last);
        let touches = if precise {
            counts.column_tests = counts.column_tests.saturating_add(1);
            let start = if row.id == first { state.start.1 } else { 0 };
            // One additional column denotes a line-break metadata mutation,
            // included only if the selected source continues beyond this row.
            let end = if row.id == last {
                state.end.1
            } else {
                row.cells.len() + 1
            };
            columns.start < end && columns.end > start
        } else {
            row.id != first && row.id != last
        };
        if touches {
            state.changed = true;
            counts.invalidations = counts.invalidations.saturating_add(1);
        }
        precise
    }
}
