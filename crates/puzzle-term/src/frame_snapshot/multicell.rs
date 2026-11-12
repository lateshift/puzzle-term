//! Sealed visible OSC 66 records, independent of live VT slot reuse (D-50).
//!
//! Sparse row references select this path; ordinary grids allocate no table.
//! A block is copied once even when seven visible rows refer to it. The first
//! visible fragment owns its ink, including when the original root is hidden.
//! Its original offset makes that ink row-relative without reconstructing IDs.
//!
//! Cache identity includes the first-visible-owner role: scrolling a lower
//! fragment into view must rebuild its ink even if its cell version is unchanged.
//! Sparse revisions also invalidate a root when another fragment was edited.
//! The frame builder compares these signatures across independently rotating slots.
//! Each capture compacts visible records into a dense prefix, so a large live
//! free-list index cannot inflate renderer storage. Removed payloads are dropped,
//! while surviving slots reuse their String capacity on the next publication.
//! Cursor and overlay readers consume the same copied fragment coordinates.
use super::{CursorSnapshot, FrameSnapshot};
use puzzle_vt::{multicell::Multicell, terminal::Terminal};
use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

/// One owned record and its first visible drawing row.
pub(crate) struct Block {
    /// Live slot values are used only within this sealed publication.
    pub key: u32,
    /// Complete payload and original fragment geometry, never a Session borrow.
    pub value: Multicell,
    /// Row that emits the complete transformed ink once.
    pub row: usize,
    /// Original vertical offset of that first visible fragment.
    pub offset: u8,
}
/// Lazy bounded visible records and per-row lookup scratch.
#[derive(Default)]
pub(crate) struct Blocks {
    /// Capacity is proportional to visible block owners, never historical keys.
    pub records: Vec<Block>,
    /// Distinct visible keys resolve without repeated linear payload searches.
    lookup: HashMap<u32, usize>,
    /// Each row names only blocks whose ink it owns this capture.
    rows: Vec<Vec<usize>>,
    /// Revisions and owner-role changes invalidate prepared row geometry.
    signatures: Vec<u64>,
}
/// Snapshot readers resolve only immutable, publication-local block copies.
impl FrameSnapshot {
    /// Test-scene contents only, not host API: source text emitted once at its owner.
    /// Raw replay compares this payload as well as ordinary cells; treating every
    /// MULTICELL sentinel as blank would miss a changed OSC 66 heading's text.
    #[cfg(feature = "test-scenes")]
    pub fn diagnostic_multicell_text(&self, row: usize, column: usize) -> Option<&str> {
        // row_blocks already restricts lookup to this publication's first visible
        // fragment owners. Continuation cells must never duplicate the payload.
        self.row_blocks(row)
            .find(|block| block.value.column == column)
            .map(|block| block.value.text.as_str())
    }
    /// Returns records whose first visible fragment belongs to this row.
    pub(crate) fn row_blocks(&self, row: usize) -> impl Iterator<Item = &Block> {
        // The optional owner avoids manufacturing a table for an ordinary publication.
        self.multicells
            .as_deref()
            .and_then(|blocks| blocks.rows.get(row).map(|indices| (blocks, indices)))
            .into_iter()
            .flat_map(|(blocks, indices)| indices.iter().map(|index| &blocks.records[*index]))
    }
    /// Returns the complete visible extent under the physical caret.
    pub(crate) fn cursor_block(&self) -> Option<&Block> {
        // Cursor coordinates remain physical; only its flagged cell can name a block.
        let cursor = self.cursor()?;
        if !cursor
            .cell
            .flags
            .contains(puzzle_vt::cell::CellFlags::MULTICELL)
        {
            return None;
        }
        // Resolve against this slot, since the live VT can already have reused its key.
        let blocks = self.multicells.as_deref()?;
        blocks.records.get(*blocks.lookup.get(&cursor.cell.ch)?)
    }
    /// Copies sparse records before row metadata is sealed.
    pub(super) fn capture_blocks(
        &mut self,
        term: &Terminal,
        count: usize,
        source_row: &impl Fn(usize) -> usize,
    ) {
        // Never-used grids do not construct even an empty hash table or row index.
        if term
            .screen()
            .ring
            .multicells
            .as_ref()
            .is_none_or(|store| store.is_empty())
        {
            self.multicells = None;
            return;
        }
        let blocks = self.multicells.get_or_insert_with(Default::default);
        // Previous lookup entries are invalid even when their numeric keys recur.
        blocks.lookup.clear();
        // Retain per-row vector capacity across the three circulating snapshot slots.
        blocks.rows.resize_with(count, Vec::new);
        blocks.signatures.resize(count, 0);
        // A viewport move changes ownership without necessarily changing any VT row.
        for row in &mut blocks.rows {
            row.clear();
        }
        // Visit sparse references beside the existing row loop, never discover via cells.
        for index in 0..count {
            // The observer distinguishes a sparse row walk from ordinary slot
            // validation, which the unused-feature path must bypass entirely.
            #[cfg(test)]
            crate::frame_builder::testing_work::charge(3, 1);
            let Some(row) = term.screen().ring.retained(source_row(index)) else {
                continue;
            };
            // All geometry-bearing fields contribute to row-cache invalidation.
            let mut signature = DefaultHasher::new();
            for reference in &row.blocks {
                // Count resolution attempts even if an invalid reference is skipped;
                // successful copies alone could conceal an unintended sparse probe.
                #[cfg(test)]
                crate::frame_builder::testing_work::charge(4, 1);
                let Some(source) = term.screen().ring.multicell(reference.key) else {
                    continue;
                };
                // The first encountered fragment owns ink; following fragments own paper.
                let fresh = !blocks.lookup.contains_key(&reference.key);
                // Active prefix indices stay dense even when live VT keys are sparse.
                let slot = blocks
                    .lookup
                    .get(&reference.key)
                    .copied()
                    .unwrap_or(blocks.lookup.len());
                // A single owned payload serves every visible fragment in this slot.
                if fresh {
                    blocks.lookup.insert(reference.key, slot);
                    blocks.copy(slot, reference.key, source, index, row.id);
                    blocks.rows[index].push(slot);
                }
                // A lower row becoming the first visible fragment changes its role
                // even if VT did not edit its cells. Include that role in the key.
                reference.key.hash(&mut signature);
                source.revision.hash(&mut signature);
                source.column.hash(&mut signature);
                source.columns.hash(&mut signature);
                source.fragments.hash(&mut signature);
                fresh.hash(&mut signature);
            }
            // Ordinary rows retain a stable zero without hashing their cell content.
            blocks.signatures[index] = if row.blocks.is_empty() {
                0
            } else {
                // Reserve zero for the ordinary-row dispatch; a real hash may be zero.
                signature.finish() | 1
            };
        }
        // Retired visible payloads release their strings rather than growing forever.
        blocks.records.truncate(blocks.lookup.len());
    }
    /// Row-local revisions add no cell scan to ordinary snapshot capture.
    pub(super) fn block_signature(&self, row: usize) -> u64 {
        // The optional owner avoids manufacturing a table for an ordinary publication.
        self.multicells
            .as_deref()
            .and_then(|blocks| blocks.signatures.get(row))
            .copied()
            .unwrap_or(0)
    }
    /// A block's ink owner isolates its full glyph run for cursor recoloring.
    pub(super) fn block_cursor_column(&self, row: usize, cursor: CursorSnapshot) -> Option<usize> {
        // Resolve against this slot, since the live VT can already have reused its key.
        let blocks = self.multicells.as_deref()?;
        // Only the drawing owner needs isolated cursor ink, including clipped roots.
        let block = blocks.records.get(*blocks.lookup.get(&cursor.cell.ch)?)?;
        (block.row == row).then_some(block.value.column)
    }
}
/// Reused Strings preserve warm publication capacity without cloning a store.
impl Blocks {
    /// Copies one complete value into the active visible prefix.
    fn copy(
        &mut self,
        slot: usize,
        key: u32,
        source: &Multicell,
        row: usize,
        line: puzzle_vt::LineId,
    ) {
        // Original fragment offsets preserve the baseline when the root is offscreen.
        let offset = source
            .fragments
            .iter()
            .find(|f| f.line == line)
            .map_or(0, |f| f.offset);
        // Dense visible order lets warm captures reuse payload storage despite sparse keys.
        if let Some(target) = self.records.get_mut(slot) {
            target.key = key;
            target.row = row;
            target.offset = offset;
            // String::clone_from preserves capacity for unchanged or shorter text.
            target.value.text.clone_from(&source.text);
            // Copy sizing and occupancy together so ink never sees another revision.
            target.value.sizing = source.sizing;
            target.value.column = source.column;
            target.value.columns = source.columns;
            // Fragment lists are bounded by seven rows and keep their inline storage.
            target.value.fragments.clone_from(&source.fragments);
            target.value.revision = source.revision;
        } else {
            // Cold visible growth owns its copy; no borrowed Session data crosses threads.
            self.records.push(Block {
                key,
                value: source.clone(),
                row,
                offset,
            });
        }
    }
}

/// Physical visible rectangles are shared by drawing, picking and IME placement.
impl FrameSnapshot {
    /// Returns visible row count for an owned rectangle, without assuming dense IDs.
    pub(crate) fn block_rows(&self, block: &Block) -> usize {
        // Stop at the first unrelated row; IDs can be reordered by region scrolling.
        self.metadata
            .iter()
            .skip(block.row)
            .take_while(|row| {
                // Membership uses stable identities, not arithmetic on monotonic IDs.
                row.id
                    .is_some_and(|id| block.value.fragments.iter().any(|f| f.line == id))
            })
            .count()
    }
    /// Reports physical column/row/width/height and the cached ink owner row.
    pub(crate) fn cursor_extent(&self) -> Option<[usize; 4]> {
        // Cursor coordinates remain physical; only its flagged cell can name a block.
        let cursor = self.cursor()?;
        // Return the visible rectangle rather than the full offscreen source height.
        if let Some(block) = self.cursor_block() {
            Some([
                block.value.column,
                block.row,
                block.value.columns,
                self.block_rows(block),
            ])
        } else {
            // Unscaled cursor geometry is unchanged, including ordinary wide glyphs.
            Some([
                cursor.view.column,
                cursor.row_index,
                if cursor.wide { 2 } else { 1 },
                1,
            ])
        }
    }
    /// Iterates visible rectangles only; an ordinary grid returns an empty iterator.
    pub(crate) fn blocks(&self) -> impl Iterator<Item = &Block> {
        // The optional owner avoids manufacturing a table for an ordinary publication.
        self.multicells
            .as_deref()
            .into_iter()
            .flat_map(|blocks| blocks.records.iter())
            // A marker row has no source ink/extent, even if its hidden source
            // identity was the first fragment of an entirely folded rectangle.
            .filter(|block| !self.metadata.get(block.row).is_some_and(|row| row.marker))
    }
}
