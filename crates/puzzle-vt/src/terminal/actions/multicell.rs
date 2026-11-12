//! Local clear preserves the connected rectangular prompt band as one unit.
//!
//! A saved Row cannot retain its old slot handles after ED2 frees the store.
//! Copy values first, then reinstall records against the destination identities.
//!
//! Cmd+K is a host action, not ED: it retains the prompt around the cursor while
//! clearing earlier output. A scaled prompt can occupy several physical rows,
//! and another block can connect to its lower fragment. `prompt_band` computes
//! that connected visible band before taking any owned copies.
//!
//! Clearing the live grid retires every old slot. Saved row cells still contain
//! those obsolete integers, so restoring them verbatim would alias newly admitted
//! text. Ordinary cells are copied first with every MULTICELL witness removed;
//! complete records are then admitted against the new row identities.
//!
//! Admission can reject bounded storage. In that case the destination remains
//! erased rather than exposing an orphaned cell key. Sparse hyperlink/underline
//! metadata is restored only after the record installation has finished. No
//! source row is borrowed across the clear or the replacement mutations.
//!
//! The temporary rows and payloads are limited by the visible prompt band.
//! This user action is cold; printing and scrolling do not create such copies.
//! Original fragment offsets survive a root clipped into history, while the
//! new identities prevent an old selection from pointing at the restored prompt.
use crate::{CellFlags, RowFlags, Terminal};

/// Host clear is cold and may own a small temporary copy of the prompt band.
impl Terminal {
    /// Retains all visible fragments connected to the cursor's current row.
    pub(super) fn clear_preserving_multicell(&mut self) -> bool {
        // Dispatch at the cursor row summary, leaving ordinary Cmd+K unchanged.
        if !self
            .screen()
            .row(self.cursor.row)
            .is_some_and(|row| row.flags.contains(RowFlags::MULTICELLS))
        {
            return false;
        }
        let (first, end) = self.prompt_band();
        // Own rows before ED2 clears their cells and drops the mutable records.
        let saved: Vec<_> = (first..end)
            .filter_map(|y| self.screen().row(y).cloned())
            .collect();
        let mut records = Vec::new();
        for row in &saved {
            for reference in &row.blocks {
                if !records.iter().any(|(key, _)| *key == reference.key)
                    && let Some(record) = self.screen().ring.multicell(reference.key)
                {
                    // Several saved rows can refer to this one owned text run.
                    records.push((reference.key, record.clone()));
                }
            }
        }
        let cursor = (self.cursor.row - first, self.cursor.column);
        // Relative cursor position follows the retained band to the top of screen.
        self.erase_display(2, false);
        self.clear_history();
        for (y, old) in saved.iter().enumerate() {
            let (screen, clock) = self.screen_clock_mut();
            if let Some(row) = screen.row_mut(y) {
                row.cells.clone_from(&old.cells);
                // Saved keys are obsolete after ED2; reinstall from owned records.
                for column in 0..row.cells.len() {
                    if row.cells[column].flags.contains(CellFlags::MULTICELL) {
                        row.cells[column] = crate::Cell::default();
                    }
                }
                row.flags = old.flags & !RowFlags::MULTICELLS;
                row.blocks.clear();
                // No stale slot is exposed: reinstall all record witnesses below.
                row.version = clock.written::<true>(row, 0..row.cells.len() + 1);
            }
        }
        self.restore_prompt_blocks(&saved, records);
        // Extras name columns; after replacement those columns belong to new keys.
        for (y, old) in saved.into_iter().enumerate() {
            if let Some(row) = self.screen_mut().row_mut(y) {
                row.extras = old.extras;
            }
        }
        self.cursor.row = cursor.0;
        self.cursor.column = cursor.1;
        self.cursor.pending_wrap = false;
        // A local clear is a fresh view boundary, so no cached row may be reused.
        self.screen_mut().damage.full = true;
        true
    }
    /// Expands only row indexes reached by connected blocks, never all history.
    fn prompt_band(&self) -> (usize, usize) {
        let ring = &self.screen().ring;
        let (mut first, mut end) = (self.cursor.row, self.cursor.row + 1);
        // The interval is bounded by visible rows and only expands. Every
        // continuing iteration adds a row, reducing the number outside the band.
        loop {
            let before = (first, end);
            // Iterate the old bounds while discovering an expanded connected band.
            for y in before.0..before.1 {
                if let Some(row) = ring.visible(y) {
                    for reference in &row.blocks {
                        if let Some(record) = ring.multicell(reference.key) {
                            for fragment in &record.fragments {
                                if let Some(index) = ring.index_of(fragment.line)
                                    && index >= ring.scrollback_len()
                                {
                                    // Historical fragments are not brought back onscreen.
                                    let y = index - ring.scrollback_len();
                                    first = first.min(y);
                                    end = end.max(y + 1);
                                }
                            }
                        }
                    }
                }
            }
            if before == (first, end) {
                // Every connected visible rectangle now fits wholly in the band.
                return before;
            }
        }
    }
    /// Replaces obsolete keys after mapping saved fragments to their new rows.
    fn restore_prompt_blocks(
        &mut self,
        saved: &[crate::Row],
        records: Vec<(u32, crate::multicell::Multicell)>,
    ) {
        for (key, mut record) in records {
            // Recover rendition from a covered cell, not the current application pen.
            let template = saved
                .iter()
                .flat_map(|row| row.cells.iter())
                .find(|cell| cell.flags.contains(CellFlags::MULTICELL) && cell.ch == key)
                .copied();
            record.fragments.retain_mut(|fragment| {
                let Some(y) = saved.iter().position(|row| row.id == fragment.line) else {
                    // A historical fragment is outside the host's retained band.
                    return false;
                };
                let Some(row) = self.screen().row(y) else {
                    // Never publish a record against a missing destination row.
                    return false;
                };
                fragment.line = row.id;
                // Offset remains relative to the original rectangle's top edge.
                true
            });
            if let Some(template) = template
                && !record.fragments.is_empty()
            {
                // insert_multicell installs all cell/index witnesses as one unit.
                let (screen, clock) = self.screen_clock_mut();
                screen.ring.insert_multicell(record, template, clock);
            }
        }
    }
}
