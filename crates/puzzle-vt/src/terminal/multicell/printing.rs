//! Ordinary printing respects lower reservations and joins block text in place.
//! The existing fast ASCII mask diverts only flagged cells here; ordinary rows
//! retain their single-store path without a separate per-cell feature probe.
//!
//! The physical cursor can address any fragment through CUP. A top fragment is
//! overwriteable, while a lower fragment reserves columns for ink above it.
//! A reservation therefore redirects ordinary printing without modifying its
//! owner. The target is reclassified after each skip, covering adjacent blocks.
//! Combining marks follow predecessor ownership instead: they extend retained
//! text and invalidate every visible fragment, never create a second owner.
//! Explicit widths are immutable even when a mark changes emoji presentation.
//! Implicit widths use the separate D-31 repair path to update the rectangle.
use super::*;
use unicode_width::UnicodeWidthChar;

/// Ordinary printing enters this seam only for a special-layout predecessor/row.
impl Terminal {
    /// Joins only a real grapheme continuation; explicit widths never change.
    pub(crate) fn join_multicell(&mut self, next: char) -> bool {
        // This explicit entry is used by OSC legacy text. Normal Unicode shares
        // the predecessor already read by the existing grapheme/mark path.
        let column = if self.cursor.pending_wrap {
            self.cursor.column
        } else {
            let Some(column) = self.cursor.column.checked_sub(1) else {
                return false;
            };
            column
        };
        let Some(cell) = self
            .screen()
            .row(self.cursor.row)
            .and_then(|r| r.cells.get(column))
        else {
            // An empty predecessor at column zero cannot receive a combining mark.
            return false;
        };
        // Ordinary text continues through its existing mark handler instead.
        if !cell.flags.contains(CellFlags::MULTICELL) {
            return false;
        }
        self.join_multicell_at(cell.ch, next)
    }
    /// Extends a block resolved by the ordinary path's existing predecessor lookup.
    // Only a flagged rectangular predecessor reaches this allocation/edit path.
    // Keep it out of the ordinary scalar loop even under whole-program LTO.
    #[cold]
    #[inline(never)]
    pub(crate) fn join_multicell_at(&mut self, key: u32, next: char) -> bool {
        // Resolving a missing key is a harmless refusal in release builds; the
        // debug checker reports the underlying invariant breach at its boundary.
        let Some(block) = self.screen().ring.multicell(key) else {
            return false;
        };
        let joins = if self.modes.get(true, 2027) == Some(true) {
            // Reuse the same incremental UAX state as ordinary grapheme printing.
            super::super::print::text_joins(
                block.text.graphemes(true).next_back().unwrap_or(""),
                next,
            )
        } else {
            // Legacy clients measure each scalar; only zero-width marks join.
            UnicodeWidthChar::width(next) == Some(0)
        };
        if !joins {
            // A new grapheme returns to the regular placement decision.
            return false;
        }
        // The last grapheme remains subject to the same 32-byte anti-Zalgo cap,
        // while an explicit run as a whole has the protocol's 4096-byte ceiling.
        if block.text.graphemes(true).next_back().map_or(0, str::len) + next.len_utf8() > 32
            || block.text.len() + next.len_utf8() > 4096
        {
            // Excess marks are consumed without moving the physical cursor.
            return true;
        }
        // Explicit runs keep their declared width; implicit clusters obey D-31
        // even when a selector arrives in a later parser chunk.
        if block.sizing.width == 0 {
            return self.join_implicit_multicell(key, next);
        }
        // Only the inline fragment coordinates are copied; appending owns the
        // String in place and accounts a rare capacity change in the store.
        let fragments = block.fragments.clone();
        let (screen, clock) = self.screen_clock_mut();
        screen.ring.append_multicell(key, next, clock);
        self.finish_sized_storage();
        // Every visible fragment must invalidate its row cache for changed ink.
        // Historical fragments retain the revised payload through the same key.
        let history = self.screen().ring.scrollback_len();
        for fragment in fragments {
            // Region rotations may reorder IDs, so resolve each surviving witness.
            if let Some(index) = self.screen().ring.index_of(fragment.line)
                && index >= history
            {
                self.screen_mut().damage.mark(index - history);
            }
        }
        true
    }
    /// Skips lower fragments or clears a top-row rectangle before ordinary printing.
    // The target-cell mask has already proved this is a rectangular collision;
    // ordinary printing must not inline its multi-row ownership repair.
    #[cold]
    #[inline(never)]
    pub(crate) fn prepare_multicell_print(&mut self, width: usize) -> bool {
        #[cfg(test)]
        crate::print_work::note(crate::print_work::Operation::MulticellRetry);
        let (columns, _) = self.size();
        // Progress is the remaining forward grid suffix: a skip increases x,
        // advances y, or scrolls into an erased bottom row. A failed transition
        // instead removes the blocking rectangle and exits; it never resets x
        // and retries the same row. Existing reservations cannot be created here.
        loop {
            let Some(row) = self.screen().row(self.cursor.row) else {
                return false;
            };
            if !row.flags.contains(RowFlags::MULTICELLS) {
                // The row summary chooses the ordinary path before any slot lookup.
                return true;
            }
            let Some(cell) = row.cells.get(self.cursor.column) else {
                return false;
            };
            if !cell.flags.contains(CellFlags::MULTICELL) {
                // A normal target can still overlap a block when this scalar is wide.
                break;
            }
            let Some(block) = self.screen().ring.multicell(cell.ch) else {
                return false;
            };
            // Original offsets survive root eviction, so a clipped continuation
            // remains a lower fragment even if it is now the first retained one.
            let lower = block
                .fragments
                .iter()
                .any(|f| f.line == row.id && f.offset != 0);
            let end = block.column + block.columns;
            let key = cell.ch;
            if !lower {
                // Any top-row overwrite clears the complete rectangle first.
                self.remove_multicell(key);
                break;
            }
            if !self.skip_lower_multicell(end, width, columns) {
                // Kitty's overwrite rules erase an entire rectangle. When no
                // forward position exists, the reviewer-defined fallback applies
                // that rule here; keep x so this character can actually be written.
                // https://sw.kovidgoyal.net/kitty/text-sizing-protocol/#wrapping-and-overwriting-behavior
                self.remove_multicell(key);
                break;
            }
        }
        // A wide scalar can start outside a block and still overlap its first
        // column; test the complete destination interval before writing either half.
        self.erase_multicell_span(
            self.cursor.row,
            self.cursor.column,
            self.cursor.column + width,
            false,
        );
        true
    }
    /// Skips strictly rightward, or reports an impossible wrap without moving x.
    fn skip_lower_multicell(&mut self, end: usize, width: usize, columns: usize) -> bool {
        // The addressed cell belongs to this rectangle, so end > current x.
        // A full glyph must fit after the skip, including both cells of a wide
        // pair. Do not first move to an unusable final column and let ordinary
        // wrapping silently restart the same row.
        if end + width > columns {
            let left = if self.lrmm_enabled {
                self.margins.left
            } else {
                0
            };
            // Lower-fragment skipping is independent of DECAWM (kitty rule 4).
            // A successful scroll produces a blank destination; a no-op LF is
            // refused by wrap_sized and the caller overwrites the obstruction.
            self.wrap_sized(left)
        } else {
            self.cursor.column = end;
            true
        }
    }
}
