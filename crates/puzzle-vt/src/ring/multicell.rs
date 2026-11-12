//! Screen-local rectangular text operations touch only indexed fragments.
//! Every removal clears cells before freeing the slot, so reuse cannot alias ink.
//! The test checker deliberately scans everything; production scrolls never do.
//!
//! A rectangle has one owned payload and one witness per occupied row. Each
//! witness moves with its row handle, so normal history scrolls change no block
//! coordinates. Retirement removes just the departing fragment. The final one
//! releases the payload before its slot can enter the free list.
//! Erasure differs from retirement: it finds and clears every retained fragment,
//! including history, because an edit addresses the whole logical block.
//! Slot zero is valid. Consumers must test MULTICELL before interpreting ch=0 as
//! a never-written blank; the checker therefore validates cells independently of
//! the row index, then validates the reverse relationship from each live record.
//! A missed summary bit, stale generation, duplicate index or partial overwrite
//! must fail this oracle even when looking up the numeric key still succeeds.
//! The production lookup returns None for a missing key without trusting its
//! provenance. Only this debug oracle turns an invalid relationship into a test
//! failure; untrusted terminal bytes never perform unchecked array indexing.
//!
//! History has two kinds of feature ownership. A live record and its String
//! capacity belong to its first retained fragment; each row also owns its sparse
//! witness vector's capacity. Unused store metadata has no owning row, so it is
//! bounded by the active-screen reserve, with any excess charged to history.
//! Incremental deltas keep those bills exact without scanning unrelated blocks.
//! Cold resize may compact the store, rewriting all witnesses before parsing
//! resumes. A normal LF never compacts: it visits only the row being retired.
//! Invariant checks deliberately recompute both bills from independent sources,
//! so a missed transfer cannot make a byte-limit test pass by undercounting.
use super::RowRing;
use crate::{
    Cell, CellFlags, Row, RowFlags,
    multicell::{Multicell, MulticellStore},
};

#[cfg(debug_assertions)]
/// The complete source oracle is isolated from product mutation paths.
mod check;

/// Keeps cell witnesses, row indexes and record ownership in one mutation seam.
impl RowRing {
    /// Removes rectangles cut by a non-reflowing resize or too tall to retain.
    pub(super) fn clip_multicells(
        &mut self,
        columns: usize,
        height: usize,
        alternate: bool,
        clock: &mut crate::WriteClock,
    ) {
        let Some(store) = self.multicells.as_deref() else {
            return;
        };
        // Resize is a cold whole-screen operation; ordinary LF never uses this scan.
        let remove: Vec<_> = store
            .iter()
            .filter_map(|(key, block)| {
                // A truncated rectangle cannot remain partially addressable on
                // the alternate screen. Primary reflow handles horizontal moves
                // separately, but neither screen can keep a too-tall block.
                let cut = block.column + block.columns > columns
                    || usize::from(block.sizing.scale) > height
                    || (alternate
                        && block.fragments.iter().any(|fragment| {
                            self.index_of(fragment.line).is_some_and(|y| y >= height)
                        }));
                cut.then_some(key)
            })
            .collect();
        for key in remove {
            // Use the atomic edit seam to retire all surviving witnesses, rather
            // than clearing just the cells beyond the new boundary.
            self.erase_multicell(key, Cell::default(), clock);
        }
    }
    /// Restores the owner's sparse link/underline metadata after rectangle replacement.
    pub(crate) fn set_multicell_extra(
        &mut self,
        line: crate::LineId,
        extra: crate::CellExtra,
        clock: &mut crate::WriteClock,
    ) {
        if let Some(index) = self.index_of(line) {
            // The ID survives erasure/reinstallation even when its screen y moved.
            let before = Self::retained_row_bytes(&self.rows[index]);
            self.rows[index].set_extra(extra, clock);
            if index < self.scrollback_len() {
                // Sparse extras can spill from inline to heap storage. History
                // must charge that change even though the payload is unchanged.
                self.history_bytes = self
                    .history_bytes
                    .saturating_sub(before)
                    .saturating_add(Self::retained_row_bytes(&self.rows[index]));
            }
        }
    }
    /// Borrows a block by slot; callers must first inspect the row layout summary.
    #[must_use]
    pub fn multicell(&self, key: u32) -> Option<&Multicell> {
        // Missing stores and retired keys are valid failed lookups, not panics.
        self.multicells.as_deref()?.get(key)
    }
    /// Publishes one fully reserved rectangle into its row-local indexes.
    pub(crate) fn insert_multicell(
        &mut self,
        block: Multicell,
        template: Cell,
        clock: &mut crate::WriteClock,
    ) -> Option<u32> {
        // A slot needs at least one occupied cell. History and screen share this bound.
        let limit = self
            .columns
            .saturating_mul(self.visible_rows + self.scrollback_lines);
        let allowance =
            MulticellStore::active_allowance(self.columns.saturating_mul(self.visible_rows))
                .saturating_add(self.scrollback_bytes);
        let store = self
            .multicells
            .get_or_insert_with(|| Box::new(MulticellStore::default()));
        let reference = store.insert(block, limit, allowance)?;
        // Admission returns the generation witness only after owning the payload.
        // Clone only the seven inline fragment coordinates, never the text payload.
        let block = store.get(reference.key)?;
        let (column, columns, fragments) = (block.column, block.columns, block.fragments.clone());
        let charge = MulticellStore::record_bytes(block);
        for (offset, fragment) in fragments.into_iter().enumerate() {
            // Fragment identities, rather than current y, locate retained history.
            if let Some(index) = self.index_of(fragment.line) {
                let historical = index < self.scrollback_len();
                let row = &mut self.rows[index];
                let before = Self::retained_row_bytes(row);
                if offset == 0 {
                    // One record can cover seven rows, but only its first
                    // retained row owns its payload in row-byte accounting.
                    row.block_bytes += charge;
                }
                // The index travels inside its row handle during a region rotation.
                // It names the record once, regardless of the occupied column count.
                row.blocks.push(reference);
                row.flags.insert(RowFlags::MULTICELLS);
                // Every covered cell carries style/protection but no duplicate text.
                // Ordinary wide/spacer flags must not survive the representation swap.
                let mut cell = template;
                cell.ch = reference.key;
                cell.flags
                    .remove(CellFlags::WIDE | CellFlags::WIDE_SPACER | CellFlags::HAS_EXTRAS);
                cell.flags.insert(CellFlags::MULTICELL);
                // Placement is the only full-rectangle write; scrolling moves handles.
                for x in column..column + columns {
                    row.put(x, cell, clock);
                }
                if historical {
                    // A clipped owner may already be in history during restore;
                    // update the same incremental counter as ordinary row edits.
                    self.history_bytes = self
                        .history_bytes
                        .saturating_sub(before)
                        .saturating_add(Self::retained_row_bytes(row));
                }
            }
        }
        Some(reference.key)
    }
    /// Appends through the ring so owner-row and history charges stay exact.
    pub(crate) fn append_multicell(&mut self, key: u32, next: char, clock: &mut crate::WriteClock) {
        // Grapheme continuations can reallocate text, without changing occupied
        // cells. Charge the growth to the first surviving fragment only.
        let Some(block) = self.multicell(key) else {
            return;
        };
        let Some(owner) = block.fragments.first().map(|fragment| fragment.line) else {
            return;
        };
        let columns = block.column..block.column + block.columns;
        let fragments = block.fragments.clone();
        let before = block.text.capacity();
        if let Some(store) = &mut self.multicells {
            store.append(key, next);
        }
        let growth = self
            .multicell(key)
            .map_or(0, |block| block.text.capacity() - before);
        if let Some(index) = self.index_of(owner) {
            // Fragment IDs survive full-width scrolling, so the owner can be
            // historical even when the joining cursor is on the active screen.
            self.rows[index].block_bytes += growth;
            if index < self.scrollback_len() {
                self.history_bytes += growth;
            }
        }
        // Every retained fragment sees revised ink, including historical rows.
        for fragment in fragments {
            if let Some(index) = self.index_of(fragment.line) {
                let row = &mut self.rows[index];
                row.version = clock.written::<true>(row, columns.clone());
            }
        }
    }
    /// Reports all store metadata, including inactive slots, without a scan.
    #[must_use]
    pub fn multicell_storage_bytes(&self) -> usize {
        // Explicit diagnostics include the lazy Box itself. Before first use
        // there is no store and therefore no feature-owned allocation at all.
        self.multicells.as_deref().map_or(0, |store| {
            store.retained_bytes() + size_of::<MulticellStore>()
        })
    }
    /// Unused metadata is bounded active-screen storage, never unreported history.
    #[must_use]
    pub fn multicell_unused_bytes(&self) -> usize {
        // Spare metadata remains real ownership even after every payload ends.
        self.multicells
            .as_deref()
            .map_or(0, MulticellStore::unused_bytes)
    }
    /// Metadata beyond the structurally bounded active reserve uses history's cap.
    pub(super) fn multicell_history_overhead(&self) -> usize {
        // Free-list capacity cannot be assigned to a row. The active reserve
        // covers it first; excess uses the existing scrollback byte allowance.
        self.multicell_unused_bytes()
            .saturating_sub(MulticellStore::active_allowance(
                self.columns.saturating_mul(self.visible_rows),
            ))
    }
    /// Cold resize releases obsolete slot capacity and updates row-local keys.
    pub(super) fn compact_multicells(&mut self) {
        let Some(store) = self.multicells.as_deref_mut() else {
            return;
        };
        let remap = store.compact();
        // This cold whole-ring rewrite is confined to resize, never a line feed.
        // Row indexes and cell keys must change together while VT owns the ring.
        for row in &mut self.rows {
            for reference in &mut row.blocks {
                if let Some(Some(new)) = remap.get(reference.key as usize) {
                    // Preserve the new slot's debug generation with its index.
                    *reference = *new;
                }
            }
            if row.flags.contains(RowFlags::MULTICELLS) {
                // Scalar-only rows keep their normal cells untouched. Flagged
                // rows may mix ordinary text with sparse block references.
                for column in 0..row.cells.len() {
                    let cell = row.cells[column];
                    if cell.flags.contains(CellFlags::MULTICELL)
                        && let Some(Some(new)) = remap.get(cell.ch as usize)
                    {
                        row.cells[column].ch = new.key;
                    }
                }
            }
        }
    }
    /// Rare block growth enforces the same shared history/spare ceiling as LF.
    pub(crate) fn finish_multicell_mutation(&mut self, tracked: &mut crate::TrackedPositions) {
        // Admission and joining can add bytes without scrolling. Apply the same
        // history eviction and spare-budget rules before returning to the parser.
        self.enforce_byte_limit(tracked);
        self.trim_spares();
    }
    /// Clears every retained fragment before its slot is returned to the free list.
    pub(crate) fn erase_multicell(&mut self, key: u32, blank: Cell, clock: &mut crate::WriteClock) {
        let Some(block) = self.multicell(key) else {
            return;
        };
        let (column, columns, fragments) = (block.column, block.columns, block.fragments.clone());
        let charge = MulticellStore::record_bytes(block);
        for (offset, fragment) in fragments.into_iter().enumerate() {
            if let Some(index) = self.index_of(fragment.line) {
                let historical = index < self.scrollback_len();
                let row = &mut self.rows[index];
                let before = Self::retained_row_bytes(row);
                if offset == 0 {
                    // Removing the first fragment removes its entire ownership
                    // bill here; atomic erase does not transfer it to another row.
                    row.block_bytes -= charge;
                }
                // Clear witnesses first. Returning the slot to the free list earlier
                // could let an old fragment resolve to a newly installed block.
                row.erase_unprotected(column, column + columns, blank, clock);
                row.blocks.retain(|reference| reference.key != key);
                if row.blocks.is_empty() {
                    // The conservative layout summary is exact after erasure;
                    // the ordinary reader can bypass all block lookups again.
                    row.flags.remove(RowFlags::MULTICELLS);
                }
                // Atomic erasure may reach an owner already in history.
                if historical {
                    self.history_bytes = self
                        .history_bytes
                        .saturating_sub(before)
                        .saturating_add(Self::retained_row_bytes(row));
                }
            }
        }
        if let Some(store) = &mut self.multicells {
            // No reachable cell names this record after the clearing loop completes.
            store.remove(key);
        }
    }
    /// Detaches a retiring row's few records; no lookup of surviving rows is needed.
    pub(super) fn retire_multicells(&mut self, row: &mut Row) {
        // The departing row has already been debited. Transfer each payload's
        // charge to its next fragment before releasing the outgoing witness.
        for reference in &row.blocks {
            // Only a departing first fragment transfers ownership. Losing a
            // middle fragment changes geometry but not the record's byte bill.
            let transfer = self.multicell(reference.key).and_then(|block| {
                (block.fragments.first()?.line == row.id)
                    .then(|| {
                        block
                            .fragments
                            .get(1)
                            .map(|next| (next.line, MulticellStore::record_bytes(block)))
                    })
                    .flatten()
            });
            if let Some((id, bytes)) = transfer
                && let Some(index) = self.index_of(id)
            {
                self.rows[index].block_bytes += bytes;
                // Between pop and append there may temporarily be fewer rows
                // than the visible height; none of those rows is historical.
                if index < self.rows.len().saturating_sub(self.visible_rows) {
                    self.history_bytes += bytes;
                }
            }
        }
        if let Some(store) = &mut self.multicells {
            // A partially retained rectangle keeps its text until the final row ends.
            store.retire(row.id, &row.blocks);
        }
        // A spare must not retain an alias to a slot that may immediately be reused.
        row.blocks.clear();
        row.block_bytes = 0;
        row.flags.remove(RowFlags::MULTICELLS);
    }
}
