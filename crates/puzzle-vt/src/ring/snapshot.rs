//! Sparse block records for detached, read-only visible BufferViews (D-50).
//!
//! A visible window may contain a lower fragment whose root is in history.
//! Copy its payload once, remap all its visible witnesses, and keep only those
//! fragments. Numeric live keys never escape into the detached store. Scratch
//! and text allocations are reused; ordinary grids construct neither table.
use super::RowRing;
use crate::multicell::{BlockRef, MulticellStore};
use std::collections::HashMap;

/// Reusable dense-key mapping for a bounded read-only publication.
#[derive(Default)]
pub struct BlockSnapshotScratch {
    /// At most one entry per visible block, independent of source history size.
    keys: HashMap<u32, BlockRef>,
}
/// Publication copying is separate from live editing and admission semantics.
impl RowRing {
    /// Seals blocks after visible rows have been copied from `source[top..]`.
    /// The receiver must be a detached read-only ring, not an active parser.
    pub fn snapshot_blocks(
        &mut self,
        source: &Self,
        top: usize,
        scratch: &mut BlockSnapshotScratch,
    ) {
        self.snapshot_blocks_rows(source, &|index| top + index, scratch);
    }

    /// Seals the same sparse payloads when a host display omits retained rows.
    /// Source indices only select rows; their identities still own every fragment.
    pub fn snapshot_blocks_rows(
        &mut self,
        source: &Self,
        source_row: &impl Fn(usize) -> usize,
        scratch: &mut BlockSnapshotScratch,
    ) {
        // Dense keys belong to this publication, so the preceding mapping is obsolete.
        scratch.keys.clear();
        // No live parser free-list survives here: this store is an immutable window.
        let store = self.multicells.get_or_insert_with(Default::default);
        for (index, target) in self.rows.iter_mut().enumerate() {
            // The detached ring contains visible rows only; history remains with its owner.
            let Some(row) = source.retained(source_row(index)) else {
                continue;
            };
            // Recopy witnesses even for unchanged rows: dense publication keys
            // may change when another rectangle enters or leaves the viewport.
            target.blocks.clone_from(&row.blocks);
            // Old charges name the preceding publication's dense slots.
            target.block_bytes = 0;
            for reference in &mut target.blocks {
                // Save the Session key before replacing the visible witness.
                let old = reference.key;
                let Some(block) = source.multicell(old) else {
                    continue;
                };
                let next = if let Some(key) = scratch.keys.get(&old) {
                    // Later fragments share the first fragment's copied payload.
                    *key
                } else {
                    let key = store.snapshot_copy(scratch.keys.len(), block);
                    // Remember ownership before the next fragment can encounter this key.
                    scratch.keys.insert(old, key);
                    key
                };
                *reference = next;
                // Touch only indexed rectangles, never scan ordinary row cells.
                for column in block.column..block.column + block.columns {
                    target.cells[column].ch = next.key;
                }
            }
        }
        // Strings outside the active visible prefix are released immediately.
        store.snapshot_finish(scratch.keys.len());
        self.finish_block_snapshot();
    }
    /// Clips fragment metadata and assigns each retained payload exactly once.
    fn finish_block_snapshot(&mut self) {
        let Some(store) = self.multicells.as_deref_mut() else {
            return;
        };
        // Visible row IDs need not be contiguous after partial-region scrolls.
        // Each small fragment list is matched against the bounded window only.
        for key in 0..store.iter().count() as u32 {
            let Some(block) = store.get_mut(key) else {
                continue;
            };
            block
                .fragments
                .retain(|f| self.rows.iter().any(|r| r.id == f.line));
            // Exactly one visible row pays for the payload, even if its root is hidden.
            if let Some(first) = block.fragments.first()
                && let Some(row) = self.rows.iter_mut().find(|r| r.id == first.line)
            {
                row.block_bytes += MulticellStore::record_bytes(block);
            }
        }
    }
}
