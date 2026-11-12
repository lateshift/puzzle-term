//! Late presentation selectors resize implicit blocks without changing explicit runs.
//! Reinstalling a rectangle retires all old witnesses before slot reuse; this is
//! a rare grapheme join, never a second pass over ordinary Unicode output.
//!
//! The base scalar already advanced the cursor by a provisional width. D-31 can
//! widen or narrow that width when a selector arrives, so occupancy and cursor
//! are repaired together. An explicit w>0 run never enters this module.
//! A late-wide root at the right edge relocates with DECAWM just like an ordinary
//! grapheme. With DECAWM disabled, neither its old text nor width is changed.
//! Reinstallation preserves inline style and sparse owner metadata; intervening
//! SGR changes affect only the BCE blanks and subsequent ordinary printing.
//! A record can outlive its first row in retained history. The first surviving
//! fragment supplies style, but its offset remains relative to the original root.
//! Relocation is allowed only for a visible root: joining must not resurrect text
//! whose root has already left the screen, nor invent rows beyond the viewport.
use super::*;

/// Repairs only the joined record, preserving ordinary printing's existing work.
impl Terminal {
    /// Applies D-31 width to a bounded joined cluster, including margin relocation.
    pub(super) fn join_implicit_multicell(&mut self, key: u32, next: char) -> bool {
        let Some(mut block) = self.screen().ring.multicell(key).cloned() else {
            return false;
        };
        // The caller proved the last grapheme fits the independent 32-byte cap.
        // This owned candidate lets a rejected widening leave old state intact.
        block.text.push(next);
        let width = super::super::print::grapheme::grapheme_width(&block.text)
            * usize::from(block.sizing.scale);
        let right = if self.lrmm_enabled {
            // Fractional ink sizing does not change occupied margin arithmetic.
            self.margins.right + 1
        } else {
            self.size().0
        };
        // D-31 rejects late widening at a disabled wrap margin. Keep both the
        // old text and rectangle intact, rather than storing an impossible width.
        if block.column + width > right {
            if self.autowrap {
                self.relocate_joined_multicell(key, block);
            }
            return true;
        }
        self.replace_joined_multicell(key, block, width)
    }
    /// Replaces all cell witnesses together after computing the complete new width.
    fn replace_joined_multicell(&mut self, key: u32, mut block: Multicell, width: usize) -> bool {
        // A clipped root still has at least one retained fragment with its style.
        let first = block.fragments[0];
        let Some(row) = self.screen().ring.line(first.line) else {
            return false;
        };
        let cell = row.cells[block.column];
        let extra = row
            .extras
            .iter()
            .find(|extra| extra.column == block.column)
            .cloned();
        // Retire every old cell before the free list can reuse its slot index.
        self.remove_multicell(key);
        // Remove other rectangles only if the revised footprint intersects them.
        for fragment in &block.fragments {
            if let Some(index) = self.screen().ring.index_of(fragment.line) {
                let history = self.screen().ring.scrollback_len();
                if index >= history {
                    // Erasure expands through any newly intersected wide pair/block.
                    self.erase_span(index - history, block.column, block.column + width, false);
                }
            }
        }
        block.columns = width;
        // Reusing an index is not a cache hit: revision and row versions change.
        block.revision = block.revision.saturating_add(1);
        let column = block.column;
        let (screen, clock) = self.screen_clock_mut();
        screen.ring.insert_multicell(block, cell, clock);
        self.finish_sized_storage();
        // Sparse style belongs to the owner, never a continuation cell.
        if let Some(extra) = extra {
            let (screen, clock) = self.screen_clock_mut();
            screen.ring.set_multicell_extra(first.line, extra, clock);
        }
        self.cursor.column = column;
        // Advance from the original left edge rather than adding to the previous
        // width; that handles both narrowing and widening without delayed-wrap drift.
        self.advance_after_print(width, self.size().0);
        true
    }

    /// Moves a late-wide root through the same wrap/scroll rules as new OSC text.
    fn relocate_joined_multicell(&mut self, key: u32, block: Multicell) {
        let Some(root) = block.fragments.iter().find(|fragment| fragment.offset == 0) else {
            // A root already evicted from history cannot be relocated onscreen.
            return;
        };
        let history = self.screen().ring.scrollback_len();
        // This fallback applies only to a visible root. It cannot pull historical
        // text back into the active screen merely because a mark was addressed.
        let Some(index) = self
            .screen()
            .ring
            .index_of(root.line)
            .filter(|index| *index >= history)
        else {
            return;
        };
        let Some(row) = self.screen().ring.line(root.line) else {
            return;
        };
        let cell = row.cells[block.column];
        let extra = row
            .extras
            .iter()
            .find(|extra| extra.column == block.column)
            .cloned();
        let saved = (self.pen, self.current_link, self.underline_color);
        // Temporary style substitution is scoped to reinstalling this one glyph.
        self.remove_multicell(key);
        // Placement copies the old glyph's style, not intervening SGR changes.
        self.pen = cell;
        self.pen
            .flags
            .remove(CellFlags::MULTICELL | CellFlags::HAS_EXTRAS);
        self.current_link = extra.as_ref().and_then(|extra| extra.link_id);
        self.underline_color = extra.as_ref().and_then(|extra| extra.underline_color);
        self.cursor.row = index - history;
        self.cursor.column = block.column;
        self.cursor.pending_wrap = true;
        // Let placement perform the ordinary wrap and any necessary region scroll.
        let base_width = super::super::print::grapheme::grapheme_width(&block.text);
        self.place_multicell(&block.text, block.sizing, base_width);
        (self.pen, self.current_link, self.underline_color) = saved;
        // The application pen remains exactly as it was before the joining mark.
    }
}
