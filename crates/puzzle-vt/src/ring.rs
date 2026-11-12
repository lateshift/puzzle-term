//! Scrollback ring that recycles full-width rows rather than allocating on LF.
//!
//! Visible rows are the tail of one deque. Full-screen upward scrolling
//! appends a new line identity and retains the old top row until either
//! scrollback limit is reached. The oldest allocation then becomes the
//! freshly blank bottom row (design §6.3, P1).
//! The ring never treats visible rows as expendable history. A configuration
//! with zero scrollback still recycles its top visible row into the bottom.
//! Memory accounting concerns row allocations rather than displayed text
//! length, because every retained row owns a complete grid-width cell vector.
//! Sparse cluster strings add to that cost and may evict additional history.
//! Rectangular text follows the same ownership: its payload is charged once to
//! the first retained fragment, while every row owns its sparse index capacity.
//! Retiring that fragment transfers its charge only to the next surviving row.
//! Store holes and free-list capacity are separately bounded and reported; they
//! cannot hide behind rows that have already left history. Scalar-only rings
//! never create the optional store or traverse records during a scroll.
//! Content stamps come from the Terminal clock, shared across both screens.
//! The ring borrows that sequence when it recycles or actually edits a row.
//! New source-order metadata is optional: without watchers, rotations keep the
//! same cell/handle operations as before. Live watchers additionally follow the
//! moved band, while a full-width LF still never traverses retained history.
use std::collections::VecDeque;

/// Scalar diagnostics expose ring/grid growth without clocks or allocation hooks.
mod activity;
#[cfg(debug_assertions)]
/// General cell/extra ownership oracle, absent from optimized product builds.
mod grid_check;
/// Primary height policy preserves the cursor before transferring rows to history.
mod height;
/// Sparse rectangular text belongs to the same screen lifetime as its rows.
mod multicell;
/// Width changes rewrap logical lines while preserving anchors.
mod reflow;
/// Batch region rotation preserves rows while avoiding one rotation per displaced line.
mod region;
/// Bounded detached publications never borrow live block records.
mod snapshot;
/// Retired history storage remains reusable within the existing retention budget.
mod spare;
/// Optional mutation witnesses track source order without a per-row membership map.
mod watches;
pub use activity::StorageGrowth;
pub use snapshot::BlockSnapshotScratch;

use crate::{
    cell::PackedColor,
    row::Row,
    tracked::{LineId, TrackedPositions},
};

/// Moving one owned handle costs at most one native pointer (D-51).
/// Its row record, cells and sparse metadata remain at stable heap addresses.
pub(super) type RowHandle = Box<Row>;

/// Retained primary-screen rows, including the visible tail.
#[derive(Clone, Debug)]
pub struct RowRing {
    /// Absent until OSC 66 first allocates a block; ordinary scrolling does no lookup.
    pub multicells: Option<Box<crate::multicell::MulticellStore>>,
    /// Eight-byte owned handles; moving a band never moves cell or sparse metadata.
    /// Oldest retained line first; the final `visible_rows` form the grid.
    rows: VecDeque<RowHandle>,
    /// The number of rows that must never be evicted as scrollback.
    visible_rows: usize,
    /// Maximum number of rows behind the visible grid.
    scrollback_lines: usize,
    /// User-requested line limit before a width-dependent byte cap applies.
    max_lines: usize,
    /// Heap byte ceiling for retained scrollback rows alone.
    scrollback_bytes: usize,
    /// Width used for every row allocation.
    columns: usize,
    /// Identity assigned to the next appended row.
    next_id: u64,
    /// Exact retained history bytes; visible edits enter this total only on scroll.
    history_bytes: usize,
    /// Number of full-grid upward scrolls, independent of new row IDs.
    scroll_total: u64,
    /// Lifetime growth counters let the desktop attribute a slow locked feed.
    growth: StorageGrowth,
    /// Lifetime history-row recycling is separate from allocation/growth counters.
    recycled_rows: u64,
    /// Cleared history is no longer addressable, but its allocations may be reused.
    spares: Vec<RowHandle>,
    /// Exact owned bytes in the inactive row pool, charged to the history budget.
    spare_bytes: usize,
}

impl RowRing {
    /// Builds visible rows and reserves the bounded deque slots up front.
    #[must_use]
    pub fn new(columns: usize, visible_rows: usize, max_lines: usize, max_bytes: usize) -> Self {
        // The minimum width has room for both halves of a wide glyph.
        let columns = columns.max(2);
        // A one-line grid gives LF a real row to recycle.
        let visible_rows = visible_rows.max(1);
        // Full-width rows dominate history memory even when visually blank.
        let row_bytes = Self::row_storage_bytes(columns);
        // The byte cap applies to history, while visible rows always exist.
        let scrollback_lines = max_lines.min(max_bytes / row_bytes);
        // Reserving deque slots avoids a hidden allocation at the first wrap.
        let capacity = visible_rows.saturating_add(scrollback_lines);
        let mut rows = VecDeque::with_capacity(capacity);
        // IDs start at zero and advance with appended terminal lines.
        for index in 0..visible_rows {
            rows.push_back(Box::new(Row::new(LineId(index as u64), columns)));
        }
        Self {
            multicells: None,
            rows,
            visible_rows,
            scrollback_lines,
            max_lines,
            scrollback_bytes: max_bytes,
            columns,
            next_id: visible_rows as u64,
            history_bytes: 0,
            scroll_total: 0,
            recycled_rows: 0,
            spares: Vec::with_capacity(scrollback_lines),
            spare_bytes: 0,
            growth: StorageGrowth {
                rows: visible_rows as u64,
                cell_bytes: visible_rows
                    .saturating_mul(columns)
                    .saturating_mul(size_of::<crate::Cell>()) as u64,
                deque_growths: 0,
            },
        }
    }

    /// Full-width cell capacity plus the separately allocated row record.
    fn row_storage_bytes(columns: usize) -> usize {
        columns
            .saturating_mul(size_of::<crate::Cell>())
            .saturating_add(size_of::<Row>())
    }

    /// Includes the boxed header as well as cells and sparse owned allocations.
    fn retained_row_bytes(row: &Row) -> usize {
        row.retained_bytes().saturating_add(size_of::<Row>())
    }

    /// Returns the grid width in columns.
    #[must_use]
    pub const fn columns(&self) -> usize {
        self.columns
    }

    /// Returns the visible grid height.
    #[must_use]
    pub const fn visible_rows(&self) -> usize {
        self.visible_rows
    }

    /// Returns the number of retained historical rows.
    #[must_use]
    pub fn scrollback_len(&self) -> usize {
        // The visible tail is present even when history is disabled.
        self.rows.len() - self.visible_rows
    }

    /// Applies a live history limit without discarding visible rows.
    pub fn set_limits(&mut self, lines: usize, bytes: usize, tracked: &mut TrackedPositions) {
        self.max_lines = lines;
        self.scrollback_bytes = bytes;
        let row_bytes = Self::row_storage_bytes(self.columns);
        self.scrollback_lines = lines.min(bytes / row_bytes);
        // Evict the oldest history first so line IDs held by selections
        // cannot point to a row beyond the newly chosen retention limit.
        while self.scrollback_len() > self.scrollback_lines {
            if let Some(mut row) = self.rows.pop_front() {
                self.history_bytes = self
                    .history_bytes
                    .saturating_sub(Self::retained_row_bytes(&row));
                self.retire_multicells(&mut row);
                tracked.evict(row.id);
            }
        }
        self.enforce_byte_limit(tracked);
        self.trim_spares();
    }

    /// Cumulative viewport scroll count for external change observers.
    #[must_use]
    pub const fn scroll_total(&self) -> u64 {
        self.scroll_total
    }

    /// Returns a visible row at a top-origin index.
    #[must_use]
    pub fn visible(&self, row: usize) -> Option<&Row> {
        // A history row is never addressable as a visible grid coordinate.
        (row < self.visible_rows)
            .then(|| self.rows.get(self.scrollback_len() + row).map(Box::as_ref))
            .flatten()
    }

    /// Returns a mutable visible row at a top-origin index.
    pub fn visible_mut(&mut self, row: usize) -> Option<&mut Row> {
        // Bounds precede the history offset to avoid aliasing scrollback.
        if row >= self.visible_rows {
            return None;
        }
        let index = self.scrollback_len() + row;
        self.rows.get_mut(index).map(Box::as_mut)
    }

    /// Borrows a document-order row in constant time, including history.
    #[must_use]
    pub fn retained(&self, index: usize) -> Option<&Row> {
        // Viewport frames must not scan all scrollback once per visible row.
        self.rows.get(index).map(Box::as_ref)
    }

    /// Locates a stable identity in document order, including region-edit gaps.
    #[must_use]
    pub fn index_of(&self, id: LineId) -> Option<usize> {
        #[cfg(test)]
        index_operations::record();
        // Normal full-width scrolling has contiguous IDs. Verify the guessed
        // slot before using it; region edits can reorder or retire identities.
        let guessed = self
            .oldest_id()
            .and_then(|first| id.0.checked_sub(first.0))
            .and_then(|distance| usize::try_from(distance).ok());
        if let Some(index) = guessed
            && self.rows.get(index).is_some_and(|row| row.id == id)
        {
            return Some(index);
        }
        // Sparse edited documents retain correctness without a per-row map.
        self.rows.iter().position(|row| row.id == id)
    }

    /// Returns a retained line by its absolute identity.
    #[must_use]
    pub fn line(&self, id: LineId) -> Option<&Row> {
        // ID lookup is for sparse markers; the hot print path uses row index.
        self.index_of(id).and_then(|index| self.retained(index))
    }

    /// Returns the oldest retained line identity.
    #[must_use]
    pub fn oldest_id(&self) -> Option<LineId> {
        self.rows.front().map(|row| row.id)
    }

    /// Returns the newest visible line identity.
    #[must_use]
    pub fn newest_id(&self) -> Option<LineId> {
        self.rows.back().map(|row| row.id)
    }

    /// Iterates retained history and visible rows in document order.
    pub fn iter(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().map(Box::as_ref)
    }

    /// The unit-test reference uses eager cells without a production policy switch.
    #[cfg(test)]
    pub(crate) fn make_rows_eager(&mut self) {
        for row in &mut self.rows {
            row.cells.make_eager();
        }
    }

    /// Appends a blank row and recycles the oldest allocation at capacity.
    ///
    /// xterm's full-width top-margin scroll places the former top row in
    /// history; the same row allocation returns when the cap evicts it.
    pub fn scroll_up(
        &mut self,
        tracked: &mut TrackedPositions,
        background: PackedColor,
        clock: &mut crate::WriteClock,
    ) {
        // A line ID also changes during region editing and reflow; this
        // counter measures only actual full-grid movement.
        self.scroll_total = self.scroll_total.saturating_add(1);
        // Only the row crossing the visible boundary adds historical bytes.
        // Historical allocations are immutable until eviction or reflow, so
        // sparse Unicode text needs no whole-history scan on each LF.
        let outgoing_id = self
            .multicells
            .as_ref()
            .and_then(|_| self.visible(0).map(|row| row.id));
        let outgoing_bytes = self.visible(0).map_or(0, Self::retained_row_bytes);
        let old_history = self.scrollback_len();
        let fresh = self.recycle_for_scroll(outgoing_bytes, tracked);
        // Evicting a root may transfer its payload charge to the outgoing row.
        let outgoing_bytes = outgoing_id.map_or(outgoing_bytes, |id| {
            self.line(id).map_or(0, Self::retained_row_bytes)
        });
        // A zero-history ring recycles its visible top; that row never becomes
        // retained history. Otherwise exactly one row crosses into history.
        if old_history > 0 || fresh.is_none() {
            self.history_bytes = self.history_bytes.saturating_add(outgoing_bytes);
        }
        // A recycled allocation gets a new identity; the old one stays gone.
        let id = LineId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        // Rare initial growth allocates once per retained historical line.
        let mut fresh = fresh.unwrap_or_else(|| self.new_row(id, clock));
        // Both fresh and inactive allocations receive the current BCE background.
        fresh.recycle(id, background, clock);
        if clock.has_watches() {
            // Surviving history keeps its key: no walk depends on history length.
            let order = self
                .rows
                .back()
                .map(|row| row.watch_order)
                .map_or(0, |last| last.wrapping_add(1));
            clock.begin_move();
            clock.moved(&mut fresh, order);
        }
        self.observe_deque_growth();
        #[cfg(test)]
        crate::scroll_operations::moved(std::iter::once(fresh.as_ref()));
        self.rows.push_back(fresh);
        self.enforce_byte_limit(tracked);
        self.trim_spares();
    }

    /// Retains a top-origin region's outgoing line without moving rows below it.
    pub fn scroll_top_region_up(
        &mut self,
        bottom: usize,
        tracked: &mut TrackedPositions,
        background: PackedColor,
        clock: &mut crate::WriteClock,
    ) {
        // The caller has already checked top=0 and full width. Clamp the
        // bottom again because parser input must never index beyond storage.
        if bottom >= self.visible_rows {
            return;
        }
        // Only the outgoing top row can add sparse bytes to history.
        // Inspecting all 10,000 retained lines on each SU made tmux's
        // status-line scroll cost depend on scrollback length.
        let outgoing_id = self
            .multicells
            .as_ref()
            .and_then(|_| self.visible(0).map(|row| row.id));
        let outgoing_bytes = self.visible(0).map_or(0, Self::retained_row_bytes);
        // Inserting after the region turns its former top row into history
        // while rows below `bottom` keep their visible coordinates.
        let old_history = self.scrollback_len();
        let fresh = self.recycle_for_scroll(outgoing_bytes, tracked);
        // Evicting a root may transfer its payload charge to the outgoing row.
        let outgoing_bytes = outgoing_id.map_or(outgoing_bytes, |id| {
            self.line(id).map_or(0, Self::retained_row_bytes)
        });
        let popped_visible_top = old_history == 0 && fresh.is_some();
        // With no history capacity, the visible top row itself is recycled;
        // otherwise it stays at the tail of retained history.
        let insert_at = if popped_visible_top {
            bottom
        } else {
            self.scrollback_len() + bottom + 1
        };
        let id = LineId(self.next_id);
        // A recycled cell vector never reuses its old external LineId.
        self.next_id = self.next_id.saturating_add(1);
        let mut blank = fresh.unwrap_or_else(|| self.new_row(id, clock));
        blank.recycle(id, background, clock);
        // Inserting immediately after the scrolling band shifts its old
        // first row into history and leaves the footer at the same index.
        self.observe_deque_growth();
        #[cfg(test)]
        crate::scroll_operations::inserting(&self.rows, insert_at, &blank);
        self.rows.insert(insert_at, blank);
        self.move_watch_orders(insert_at, clock);
        // A top-region scroll accounts the same outgoing allocation while
        // leaving every footer row outside history and at its old coordinate.
        if !popped_visible_top {
            self.history_bytes = self.history_bytes.saturating_add(outgoing_bytes);
        }
        self.enforce_byte_limit(tracked);
        self.trim_spares();
    }

    /// Drops only retained history, leaving visible content and IDs intact.
    pub fn clear_scrollback(&mut self, tracked: &mut TrackedPositions) {
        // Observe document retirement separately from allocation retention.
        let retired = self.scrollback_len();
        // ED 3 removes only rows older than the visible tail. Retiring the
        // IDs first makes dangling overlay and selection handles become Gone.
        while self.scrollback_len() > 0 {
            if let Some(mut row) = self.rows.pop_front() {
                self.history_bytes = self
                    .history_bytes
                    .saturating_sub(Self::retained_row_bytes(&row));
                self.retire_multicells(&mut row);
                tracked.evict(row.id);
                self.retain_spare(row);
            }
        }
        self.history_bytes = 0;
        tracing::trace!(
            event = "vt.history_cleared",
            retired,
            reusable = self.spares.len(),
            reusable_bytes = self.spare_bytes,
            "history identities retired; bounded allocations remain reusable"
        );
    }

    /// Reuses an outgoing allocation at either history ceiling before creating a row.
    fn recycle_for_scroll(
        &mut self,
        outgoing: usize,
        tracked: &mut TrackedPositions,
    ) -> Option<RowHandle> {
        let history = self.scrollback_len();
        let limit = self.visible_rows + self.scrollback_lines;
        if self.rows.len() < limit
            && self
                .history_bytes
                .saturating_add(self.multicell_history_overhead())
                .saturating_add(outgoing)
                <= self.scrollback_bytes
        {
            return None;
        }
        let mut row = self.rows.pop_front()?;
        self.recycled_rows = self.recycled_rows.saturating_add(1);
        // At zero history the removed row is visible, and contributes no history
        // bytes. Every other removal debits exactly the allocation being retired.
        if history > 0 {
            self.history_bytes = self
                .history_bytes
                .saturating_sub(Self::retained_row_bytes(&row));
        }
        self.retire_multicells(&mut row);
        tracked.evict(row.id);
        Some(row)
    }

    /// Evicts excess history if rare extras pushed memory over its cap.
    fn enforce_byte_limit(&mut self, tracked: &mut TrackedPositions) {
        // Exact boundary accounting includes sparse text and reusable stores.
        // Evict only the excess oldest rows; never rescan surviving history.
        while self.scrollback_len() > 0
            && self
                .history_bytes
                .saturating_add(self.multicell_history_overhead())
                > self.scrollback_bytes
        {
            if let Some(mut row) = self.rows.pop_front() {
                self.history_bytes = self
                    .history_bytes
                    .saturating_sub(Self::retained_row_bytes(&row));
                self.retire_multicells(&mut row);
                tracked.evict(row.id);
            }
        }
    }

    /// Measures retained history including sparse cluster allocations.
    #[must_use]
    pub fn history_bytes(&self) -> usize {
        // Historical rows cannot be edited through visible_mut; only scroll,
        // clear, limit changes and reflow change their accounted allocations.
        self.history_bytes
    }
}

#[cfg(test)]
#[path = "ring/tests.rs"]
mod tests;

#[cfg(test)]
/// Test-only observations distinguish oracle map lookups from product row scans.
pub(crate) mod index_operations;
