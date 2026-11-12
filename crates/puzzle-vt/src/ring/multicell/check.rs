//! Debug/test source oracle for sparse rectangular cells (M18.7/M19.2b).
//! Reverse validation builds the row-ID index once per check. A linear search
//! per fragment would multiply retained history by block count under ASan.
//! All cell, generation, overlap and byte-accounting checks remain unchanged;
//! product lookups and mutation paths never allocate this temporary map.
//!
//! Validation deliberately ignores the product's line-lookup optimization.
//! Region edits can make stable identities sparse, defeating its guessed slot;
//! a valid fallback would then search all retained rows for each fragment.
//! This oracle already traverses all rows to inspect cells, so indexing those
//! rows once bounds reverse validation by the source it actually checks.
//! Fragment offset comparisons remain bounded by OSC 66's scale range, not by
//! scrollback length. Byte charges are recomputed independently, so replacing
//! lookup does not hide a stale reference, lost fragment or accounting defect.
use super::{MulticellStore, RowRing};
use crate::{CellFlags, RowFlags};

/// Both directions of source ownership must agree after each tested operation.
impl RowRing {
    /// Expensive invariant oracle, compiled out of optimized product builds.
    #[cfg(debug_assertions)]
    pub fn check_multicells(&self) -> Result<(), String> {
        // Check the cell-to-record direction independently of the sparse index.
        // A missing index must not hide an orphaned flagged cell from this oracle.
        for row in self.iter() {
            for (column, cell) in row.cells.iter().enumerate() {
                if !cell.flags.contains(CellFlags::MULTICELL) {
                    // Scalar cells have no relationship to the optional block table.
                    continue;
                }
                let block = self
                    .multicell(cell.ch)
                    .ok_or("cell references freed slot")?;
                // Both the stable row and horizontal span must describe this cell;
                // resolving a live key alone would miss a stale moved witness.
                if !block.fragments.iter().any(|f| f.line == row.id)
                    || !(block.column..block.column + block.columns).contains(&column)
                {
                    return Err("cell is outside its recorded fragment".into());
                }
            }
            // The row witness detects reuse while the cell preserves its 16-byte ABI.
            for reference in &row.blocks {
                let store = self
                    .multicells
                    .as_deref()
                    .ok_or("row references absent store")?;
                // A recycled slot can have valid text yet the wrong generation.
                if !store.valid_reference(*reference) {
                    return Err("stale generation".into());
                }
            }
            // Ordinary dispatch trusts this summary; a false negative loses ink.
            if row.flags.contains(RowFlags::MULTICELLS) != !row.blocks.is_empty() {
                return Err("row layout summary disagrees with sparse index".into());
            }
        }
        // One index serves every reverse lookup, including sparse/reordered
        // identities where the product's verified guessed-slot path falls back.
        let rows: std::collections::HashMap<_, _> = self
            .iter()
            .enumerate()
            .map(|(index, row)| (row.id, index))
            .collect();
        if rows.len() != self.rows.len() {
            return Err("duplicate retained row identity".into());
        }
        self.check_block_fragments(&rows)
    }
    // Reverse validation below complements the per-cell direction above: a
    // lost cell cannot be hidden by a still-live record or a correct byte sum.
    /// Recomputes row and history ownership without trusting incremental charges.
    #[cfg(debug_assertions)]
    fn check_multicell_charges(&self, store: &MulticellStore) -> Result<(), String> {
        // Payload/record bytes belong exactly once to the first retained row.
        // Unused slot and free-list capacity is separately reported store memory.
        for row in self.iter() {
            // Independently derive the row's charge from records, not from
            // the increment/decrement bookkeeping used by production edits.
            let expected: usize = row
                .blocks
                .iter()
                .filter_map(|reference| store.get(reference.key))
                .filter(|block| {
                    block
                        .fragments
                        .first()
                        .is_some_and(|first| first.line == row.id)
                })
                .map(MulticellStore::record_bytes)
                .sum();
            if expected != row.block_bytes {
                return Err("first-fragment storage charge differs".into());
            }
        }
        let history: usize = self
            .rows
            .iter()
            .take(self.scrollback_len())
            .map(|row| Self::retained_row_bytes(row))
            .sum();
        if history != self.history_bytes {
            // A correct total store size can still hide a misplaced ownership
            // charge, which would make later history eviction use the wrong cap.
            return Err(format!(
                "history storage charge differs: expected {history}, recorded {}",
                self.history_bytes
            ));
        }
        Ok(())
    }
    /// Checks the reverse relationship too: a live record must own all its cells.
    #[cfg(debug_assertions)]
    fn check_block_fragments(
        &self,
        rows: &std::collections::HashMap<crate::tracked::LineId, usize>,
    ) -> Result<(), String> {
        let Some(store) = self.multicells.as_deref() else {
            return Ok(());
        };
        store.check_storage()?;
        // Verify allocator metadata before trusting any row-owned accounting.
        self.check_multicell_charges(store)?;
        // Offsets are relative to the original rectangle, including clipped roots.
        // Neither duplicate offsets nor duplicate row IDs describe legal fragments.
        // Reverse validation detects overlaps or edits that left only some of
        // a rectangle intact, even if every remaining flagged cell resolves.
        for (key, block) in store.iter() {
            for (index, fragment) in block.fragments.iter().enumerate() {
                if usize::from(fragment.offset) >= usize::from(block.sizing.scale)
                    || block.fragments[..index]
                        .iter()
                        .any(|old| old.line == fragment.line || old.offset == fragment.offset)
                {
                    return Err("invalid or duplicate rectangle fragment".into());
                }
            }
            if block.fragments.is_empty() {
                // Retirement must free a record immediately after its final witness.
                return Err("unreferenced live slot".into());
            }
            for fragment in &block.fragments {
                // Hash lookup followed by document-index lookup is expected O(1).
                // Never call line/index_of once per fragment in this full oracle.
                let index = *rows.get(&fragment.line).ok_or("missing fragment row")?;
                let row = self.retained(index).ok_or("missing indexed fragment row")?;
                if row.blocks.iter().filter(|r| r.key == key).count() != 1 {
                    // Exactly one index entry is necessary for atomic row-local edits.
                    return Err("missing or duplicate row index".into());
                }
                // Every recorded column must belong exclusively to this slot.
                for x in block.column..block.column + block.columns {
                    let cell = row.cells.get(x).ok_or("fragment extends past row")?;
                    if !cell.flags.contains(CellFlags::MULTICELL) || cell.ch != key {
                        // An ordinary write or another key inside the span is an overlap.
                        return Err("fragment overlaps another block or ordinary cell".into());
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
/// Structural lookup counts guard against reintroducing a history scan per block.
mod tests;
