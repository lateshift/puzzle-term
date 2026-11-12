//! One full-width row, with rare data kept away from the hot cell array.
//!
//! Sparse extras are ordered by column so a cell overwrite can remove its
//! metadata without scanning every screen row. Ordinary ASCII uses no heap
//! beyond the row's fixed cell allocation (design §6.2, P1).
//! Soft wrap is a row property because reflow joins rows into logical lines.
//! A terminal can change a row after it enters scrollback; the version then
//! invalidates any shaped or painted copy held by a later renderer.
//! Sparse extras are needed for clusters, hyperlinks and underline color,
//! none of which justify enlarging every sixteen-byte cell.
//! Erase operations differ from print stores: most clear a whole suffix.
//! Their ordinary path fills that suffix once, instead of paying sparse lookup
//! and version updates for every column. Selective erasure retains its cell
//! inspection because protection is allowed to vary within the same span.
//! Writes keep row-local cache versions until a source consumer activates the
//! Terminal sequence; both paths invalidate row caches with the same store.
//! Plain stores specialise optional observation away; watched stores report
//! their actual columns, preserving identical-rewrite detection.
use bitflags::bitflags;
use smallvec::SmallVec;

/// Lazy cell storage is shared by every row consumer, including snapshots.
mod cells;
pub use cells::{CellsIter, RowCells};

/// One shared cut rule for column edits, rectangle moves and plain truncation.
pub(crate) mod edges;

use crate::{
    cell::{Cell, CellFlags, PackedColor},
    tracked::LineId,
};

bitflags! {
    /// Flags describing a row's relationship to nearby rows.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct RowFlags: u8 {
        /// This row continues as the next row of the same logical line.
        const WRAPPED = 1 << 0;
        /// A shell-integration prompt marker starts on this row.
        const PROMPT = 1 << 1;
        /// Conservative summary skips placeholder resolution on ordinary rows.
        const IMAGE_PLACEHOLDERS = 1 << 2;
        /// Sparse OSC 66 references are present; ordinary rows bypass that path.
        const MULTICELLS = 1 << 3;
        /// Exact write observers exist on this source boundary, never on plain rows.
        const BOUNDARY_WATCH = 1 << 4;
        /// This retained row has an optional circular source-order coordinate.
        const WATCH_ORDER = 1 << 5;
        /// Optional order metadata belongs to the independent alternate document.
        const WATCH_ALTERNATE = 1 << 6;
    }
}

/// Rare per-column text and decoration data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellExtra {
    /// Column owning this metadata; the row keeps extras sorted by it.
    pub column: usize,
    /// Full cluster text when more than one scalar occupies a cell.
    pub cluster: Option<String>,
    /// Optional underline color independent of foreground.
    pub underline_color: Option<PackedColor>,
    /// Stable hyperlink table handle, interpreted by a later module.
    pub link_id: Option<u32>,
}

/// A stable logical row whose cell allocation rotates through scrollback.
#[derive(Clone, Debug)]
pub struct Row {
    /// Monotonic identity retained while the row scrolls through history.
    pub id: LineId,
    /// One cell per grid column, reused after eviction.
    pub cells: RowCells,
    /// Sorted sparse metadata; two entries fit inline without allocation.
    pub extras: SmallVec<[CellExtra; 2]>,
    /// Row-local sparse block references move intact with this row handle.
    pub blocks: Vec<crate::multicell::BlockRef>,
    /// Record/text bytes charged once on the first retained fragment.
    pub(crate) block_bytes: usize,
    /// Cluster allocations detached by overwrite/recycling, bounded by columns.
    /// Only real multi-scalar clusters create these 32-byte stores (P1).
    cluster_spares: SmallVec<[String; 2]>,
    /// Soft-wrap and prompt annotations.
    pub flags: RowFlags,
    /// Optional source order, maintained only while range watches are live.
    /// WATCH_ORDER excludes scratch/spare rows. This is a circular ring position,
    /// not an identity or write counter; compact storage uses Row's padding.
    pub(crate) watch_order: u32,
    /// Row-local cache version, globally ordered while source observation is active.
    /// Content-preserving reflow inherits the maximum of its source versions.
    pub version: u64,
}

impl Row {
    /// Allocates a full-width blank row before it enters the ring.
    #[must_use]
    pub fn new(id: LineId, columns: usize) -> Self {
        Self {
            id,
            // Full-width storage makes any column writable without growth.
            cells: RowCells::new(columns),
            extras: SmallVec::new(),
            blocks: Vec::new(),
            block_bytes: 0,
            cluster_spares: SmallVec::new(),
            flags: RowFlags::empty(),
            version: 0,
            watch_order: 0,
        }
    }

    /// Clears a recycled row without discarding its cell allocation.
    pub fn recycle(&mut self, id: LineId, background: PackedColor, clock: &mut crate::WriteClock) {
        let version = clock.written::<true>(self, 0..self.cells.len() + 1);
        self.clear_for_reflow(id, background);
        self.version = version;
    }

    /// Resets allocation-only reflow scratch; the builder supplies source maxima.
    pub(crate) fn clear_for_reflow(&mut self, id: LineId, background: PackedColor) {
        // A recycled row becomes a different logical line at the ring tail.
        self.id = id;
        // BCE uses the current pen background, not a theme default.
        self.cells.fill(Cell::erased(background));
        // SmallVec keeps inline storage; spilled storage remains reusable.
        // Detach old cluster text into reusable storage before clearing metadata.
        while let Some(extra) = self.extras.pop() {
            self.recycle_cluster(extra.cluster);
        }
        self.blocks.clear();
        self.block_bytes = 0;
        self.flags = RowFlags::empty();
        // A fresh destination's version is inherited from the source it copies.
        self.version = 0;
        self.watch_order = 0;
    }

    /// Replaces a cell and drops its old sparse metadata.
    pub fn put(&mut self, column: usize, cell: Cell, clock: &mut crate::WriteClock) {
        self.put_observed::<true>(column, cell, clock);
    }

    /// The same store is specialised once for watched versus ordinary print slices.
    #[inline]
    pub(crate) fn put_observed<const WATCH: bool>(
        &mut self,
        column: usize,
        cell: Cell,
        clock: &mut crate::WriteClock,
    ) {
        if column < self.cells.len() {
            // A complete replacement needs no writable borrow: get_mut would
            // first fill this very slot with BCE and immediately overwrite it.
            // Materialize only a preceding gap, as the ordinary ASCII path does.
            self.cells.store(column, cell);
            // Sticky until recycling/whole erasure; overwrites may leave a harmless
            // false positive, never a missed placeholder after cluster joining.
            if cell.ch == Cell::IMAGE_PLACEHOLDER {
                self.flags.insert(RowFlags::IMAGE_PLACEHOLDERS);
            }
            // An overwritten hyperlink or cluster cannot survive as stale
            // metadata on the new scalar at this column.
            if let Ok(index) = self
                .extras
                .binary_search_by_key(&column, |extra| extra.column)
            {
                let extra = self.extras.remove(index);
                self.recycle_cluster(extra.cluster);
            }
            self.version = clock.written::<WATCH>(self, column..column + 1);
        }
    }

    /// Adds or replaces metadata while preserving column order.
    pub fn set_extra(&mut self, extra: CellExtra, clock: &mut crate::WriteClock) {
        self.set_extra_observed::<true>(extra, clock);
    }

    /// Sparse print metadata uses the same optional mutation instantiation.
    pub(crate) fn set_extra_observed<const WATCH: bool>(
        &mut self,
        extra: CellExtra,
        clock: &mut crate::WriteClock,
    ) {
        if extra.column < self.cells.len() {
            let column = extra.column;
            self.copy_extra(extra);
            self.version = clock.written::<WATCH>(self, column..column + 1);
        }
    }

    /// Copies source metadata during content-preserving reflow without a new write.
    pub(crate) fn copy_extra(&mut self, extra: CellExtra) {
        // Reflow can copy a CLUSTER sentinel directly, bypassing scalar put.
        if extra
            .cluster
            .as_ref()
            .is_some_and(|text| text.starts_with('\u{10eeee}'))
        {
            self.flags.insert(RowFlags::IMAGE_PLACEHOLDERS);
        }
        let column = extra.column;
        // External sequences must never create metadata outside the grid.
        if column >= self.cells.len() {
            return;
        }
        match self
            .extras
            .binary_search_by_key(&column, |item| item.column)
        {
            // Insertion is rare; binary lookup keeps all consumers ordered.
            Ok(index) => {
                let old = std::mem::replace(&mut self.extras[index], extra);
                self.recycle_cluster(old.cluster);
            }
            Err(index) => self.extras.insert(index, extra),
        }
        // Renderers need not search sparse extras for ordinary cells.
        self.cells[column].flags.insert(CellFlags::HAS_EXTRAS);
    }

    /// Takes existing cluster storage or a warm spare only after a real join.
    pub(crate) fn cluster_storage(&mut self, column: usize) -> String {
        // Taking the string preserves hyperlink/underline metadata for the
        // later replacement; the text is restored before this mutation escapes.
        let stored = self
            .extras
            .iter_mut()
            .find(|extra| extra.column == column)
            .and_then(|extra| extra.cluster.take());
        let mut text = stored
            .or_else(|| self.cluster_spares.pop())
            .unwrap_or_else(|| String::with_capacity(32));
        text.clear();
        text
    }

    /// Retains at most one spare per column and accounts it as row memory.
    fn recycle_cluster(&mut self, text: Option<String>) {
        if let Some(mut text) = text
            && self.cluster_spares.len() < self.cells.len()
        {
            // Clearing changes length only; its bounded byte allocation stays warm.
            text.clear();
            self.cluster_spares.push(text);
        }
    }

    /// Estimates retained heap bytes for the scrollback byte limit.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        // Row cells are always full width; cluster strings add rare storage.
        let clusters = self
            .extras
            .iter()
            .filter_map(|extra| extra.cluster.as_ref());
        let extra_bytes = if self.extras.spilled() {
            // Inline SmallVec capacity is part of Row itself, not heap.
            self.extras.capacity() * size_of::<CellExtra>()
        } else {
            0
        };
        // Spare Strings own the same bounded bytes as attached clusters.
        // Count their SmallVec heap only when it has spilled from inline storage.
        self.block_bytes
            + self.blocks.capacity() * size_of::<crate::multicell::BlockRef>()
            + self.cells.capacity() * size_of::<Cell>()
            + clusters.map(String::capacity).sum::<usize>()
            + extra_bytes
            + self
                .cluster_spares
                .iter()
                .map(String::capacity)
                .sum::<usize>()
            + if self.cluster_spares.spilled() {
                self.cluster_spares.capacity() * size_of::<String>()
            } else {
                0
            }
    }
}

/// Bulk erasure owns only one row and never affects a neighboring row allocation.
impl Row {
    /// Blanks an ordinary BCE span, retiring sparse metadata and split wide partners.
    pub(crate) fn erase_unprotected(
        &mut self,
        start: usize,
        end: usize,
        blank: Cell,
        clock: &mut crate::WriteClock,
    ) {
        let end = end.min(self.cells.len());
        if start >= end {
            return;
        }
        // The same atomic cut rule runs before movement and before erasure;
        // neither path may leave an outside partner referring to replaced cells.
        self.erase_split_wide_at(start, blank, clock, false);
        self.erase_split_wide_at(end, blank, clock, false);
        self.cells.fill_range(start..end, blank);
        // Complete erasure proves the row contains no placeholder; partial
        // erasure keeps the conservative summary without rescanning surviving cells.
        if start == 0 && end == self.cells.len() {
            self.flags.remove(RowFlags::IMAGE_PLACEHOLDERS);
        }
        // Sparse entries are sorted. Remove the erased interval backwards so
        // retained entries are never skipped; cluster allocations remain warm.
        let first = self.extras.partition_point(|extra| extra.column < start);
        let last = self.extras.partition_point(|extra| extra.column < end);
        for index in (first..last).rev() {
            let extra = self.extras.remove(index);
            self.recycle_cluster(extra.cluster);
        }
        // Damage consumers compare versions, not the number of erased cells.
        self.version = clock.written::<true>(self, start..end);
    }
}

#[cfg(test)]
#[path = "row/tests.rs"]
mod tests;
