//! Erase and scroll operations with background-color erase (§6.2, §6.5).
//!
//! xterm's `bce` capability means erased cells retain the current pen
//! background. Protected cells survive DEC selective erase. History grows
//! only when a full-width region beginning at row zero scrolls upward.
//!
//! There are three different coordinate domains here. ED/EL operate on the
//! visible grid, so their columns ignore DEC margins. ICH/DCH operate from
//! the cursor to the right boundary, which is narrower with DECLRMM. SU/SD
//! operate on a vertical region; the ring retains the removed row only
//! when that region covers the whole width, starts at row zero, and the
//! primary screen is active. These distinctions keep shell scrollback from
//! filling with vim's internal scrolling and leave status lines intact.
//!
//! Cell metadata is sparse and sorted by column. Moving plain cells is a
//! single slice operation, while sparse extras follow their cells in column
//! order. An orphaned wide spacer is never
//! allowed at the edge of the edited span, because it would offset every
//! later glyph in that row. The operations are triggered per control
//! sequence rather than per printed byte; the ASCII store path is separate.
//!
//! The representation distinguishes a row's identity from its screen
//! position. A full-width scroll can move a Row, preserving its LineId and
//! all sparse metadata. A left/right-margin scroll cannot move a Row,
//! because cells outside the rectangle must remain in their positions;
//! it copies only the selected span and leaves row identities fixed.
//! That case marks full damage until M6 can offer a rectangle hint.
//! Neither path creates retained history on the alternate screen. ED 3
//! retires only historical identities; visible rows keep their IDs, so a
//! marker into current text remains valid.
//!
//! A wide glyph consists of a leading cell and a spacer. Erasing only the
//! spacer would leave a leading glyph whose width no longer fits the row.
//! Erase and movement prepare every split edge before changing cells;
//! no shifted-suffix repair scan is needed. The pen's DECSCA bit is copied to both
//! halves so selective erase treats the glyph as one protected unit.

use super::{ActiveScreen, Terminal};
use crate::cell::{Cell, CellFlags};

/// Atomic source/destination cuts are shared by every column-bounded operation.
mod boundaries;
/// Sparse metadata moves with complete cells after boundary preparation.
mod columns;
use columns::shift_extras;

#[cfg(test)]
/// Atomic wide/block edges across every supported column-bounded operation.
mod tests;

impl Terminal {
    /// Erases display cells by ED/DECSED, or only scrollback for ED 3.
    pub(crate) fn erase_display(&mut self, kind: u16, selective: bool) {
        let (columns, rows) = self.size();
        // ED 3 is xterm's history erase. It must not erase visible cells;
        // DECSED has no scrollback variant, so the private form is ignored.
        if kind == 3 {
            if !selective && self.active == ActiveScreen::Primary {
                self.source_cleared();
                self.primary.ring.clear_scrollback(&mut self.tracked);
            }
            return;
        }
        if kind == 2 && !selective {
            self.source_cleared();
            // The wire image model observes this semantic erase at its next boundary.
            let epoch = &mut self.parsing.framing.image_clear
                [usize::from(self.active == ActiveScreen::Alternate)];
            *epoch = epoch.wrapping_add(1);
        }
        let cursor = self.cursor;
        // ED 0/1 select opposite half-planes around the cursor; ED 2
        // touches every row, regardless of the active scroll margins.
        for row in 0..rows {
            let range = match kind {
                0 if row < cursor.row => continue,
                0 if row == cursor.row => cursor.column..columns,
                0 => 0..columns,
                1 if row > cursor.row => continue,
                1 if row == cursor.row => 0..cursor.column + 1,
                1 => 0..columns,
                2 => 0..columns,
                _ => return,
            };
            self.erase_span(row, range.start, range.end, selective);
        }
        // An explicit erase is a control action; delayed autowrap no longer
        // describes the cursor after the screen was modified.
        self.cursor.pending_wrap = false;
    }

    /// Erases a line selection using EL/DECSEL coordinates.
    pub(crate) fn erase_line(&mut self, kind: u16, selective: bool) {
        let (columns, _) = self.size();
        // EL 1 includes the cursor's own cell; the half-open Rust end needs
        // one extra column to preserve that xterm control-sequence rule.
        let (start, end) = match kind {
            0 => (self.cursor.column, columns),
            1 => (0, self.cursor.column + 1),
            2 => (0, columns),
            _ => return,
        };
        self.erase_span(self.cursor.row, start, end, selective);
        self.cursor.pending_wrap = false;
    }

    /// Blanks one bounded span, preserving DECSCA-protected cells if asked.
    pub(crate) fn erase_span(
        &mut self,
        row_index: usize,
        start: usize,
        end: usize,
        selective: bool,
    ) {
        // Attribute selected erase work, including the cost of protected-cell inspection.
        if row_index < self.size().1 {
            self.note_operation(super::operation_metrics::Operation::EraseRows, 1);
            self.note_operation(
                super::operation_metrics::Operation::EraseCells,
                end.min(self.size().0).saturating_sub(start) as u64,
            );
        }
        self.prepare_column_edges(row_index, start, end.min(self.size().0), selective);
        self.erase_multicell_span(row_index, start, end, selective);
        let blank = Cell::erased(self.pen.bg);
        // The pen's foreground and attributes do not survive BCE. Only
        // its background colors the newly empty area (§6.2).
        let (screen, clock) = self.screen_clock_mut();
        if let Some(row) = screen.row_mut(row_index) {
            if !selective {
                // Ordinary EL/ECH/ED has no protected cells to inspect. A
                // bulk fill preserves BCE and only checks split wide borders.
                row.erase_unprotected(start, end, blank, clock);
                self.screen_mut().damage.mark(row_index);
                return;
            }
            for column in start..end.min(row.cells.len()) {
                // Protected text remains readable after DECSED/DECSEL;
                // ordinary ED/EL deliberately ignore this bit.
                if selective && row.cells[column].flags.contains(CellFlags::PROTECTED) {
                    continue;
                }
                // Erasing either half invalidates its partner (§6.2).
                let old = row.cells[column];
                if old.flags.contains(CellFlags::WIDE) && column + 1 < row.cells.len() {
                    row.put(column + 1, blank, clock);
                } else if old.flags.contains(CellFlags::WIDE_SPACER) && column > 0 {
                    row.put(column - 1, blank, clock);
                }
                row.put(column, blank, clock);
            }
            // Row::put bumps the version and removes sparse extras, so a
            // cleared hyperlink never remains clickable at that column.
            self.screen_mut().damage.mark(row_index);
        }
    }

    /// Inserts or deletes columns only inside the active horizontal region.
    pub(crate) fn shift_chars(&mut self, count: usize, insert: bool) {
        let row_index = self.cursor.row;
        let start = self.cursor.column;
        // Horizontal edits stop at the right DEC margin only while mode 69
        // is active; otherwise they may reach the physical last column.
        let right = if self.modes.get(true, 69) == Some(true) {
            self.margins.right
        } else {
            self.size().0 - 1
        };
        if start > right {
            return;
        }
        // A malicious large count becomes at most the remaining row width.
        // That also keeps every copy_within range within allocated cells.
        let count = count.min(right - start + 1);
        self.prepare_column_splice(row_index, start, right + 1, count, insert);
        self.shift_multicells(start, right, count, insert);
        let blank = Cell::erased(self.pen.bg);
        let (screen, clock) = self.screen_clock_mut();
        if let Some(row) = screen.row_mut(row_index) {
            if insert {
                // Shift toward the right edge before filling the vacated
                // cells, so source bytes cannot be overwritten early.
                if count < right - start + 1 {
                    row.cells
                        .copy_within(start..right + 1 - count, start + count);
                }
                row.cells.fill_range(start..start + count, blank);
            } else {
                // Deleting collapses following text left and uses BCE for
                // cells opened at the right margin.
                if count < right - start + 1 {
                    row.cells.copy_within(start + count..right + 1, start);
                }
                row.cells.fill_range(right + 1 - count..right + 1, blank);
            }
            // Sparse links and clusters travel with their moved cells.
            // Entries deleted or pushed past the margin are discarded.
            shift_extras(row, start, right, count, insert);
            // Bulk cell movement bypasses Row::put, so change the version
            // explicitly for layout and damage consumers.
            // The stamp orders this bulk replacement with writes on every row.
            // Boundary observers see the whole edited span, including identical
            // cell rewrites; they never infer preservation from copied bytes.
            row.version = clock.written::<true>(row, start..right + 1);
        }
        self.screen_mut().damage.mark(row_index);
        self.cursor.pending_wrap = false;
    }

    /// Scrolls the current DEC region upward, keeping history only at top.
    pub(crate) fn scroll_region_up(&mut self, count: usize) {
        let (width, height) = self.size();
        // Clamp margin fields even if a resize later shortened the grid;
        // a malformed control sequence must never make row indexing panic.
        let top = self.margins.top.min(height - 1);
        let bottom = self.margins.bottom.min(height - 1);
        if top > bottom {
            return;
        }
        let count = count.min(bottom - top + 1);
        self.note_operation(super::operation_metrics::Operation::ScrollUp, count as u64);
        // A horizontal DEC margin changes this from row rotation to a
        // rectangle copy. Outer columns cannot follow moved row IDs.
        if self.margins.left != 0 || self.margins.right != width - 1 {
            // DECLRMM scrolls only its rectangle, not entire row objects.
            self.scroll_rect(top, bottom, count, true);
            return;
        }
        self.prepare_multicell_scroll(top, bottom, count, true);
        if self.active == ActiveScreen::Primary && top == 0 {
            // Primary top-origin output still admits each outgoing line to history.
            for _ in 0..count {
                self.scroll_up_once(top, bottom, height);
            }
        } else {
            // Alternate and lower-origin bands discard rows. Rotate once by the
            // whole count rather than repeatedly moving the same survivors.
            let background = self.pen.bg;
            let Self {
                primary,
                alternate,
                active,
                tracked,
                write_clock,
                ..
            } = self;
            let ring = match active {
                ActiveScreen::Primary => &mut primary.ring,
                ActiveScreen::Alternate => &mut alternate.ring,
            };
            ring.scroll_region(top, bottom, count, true, tracked, background, write_clock);
        }
        // Rows outside DECSTBM, especially tmux's footer, retain their
        // line identities and cached instances across this region scroll.
        self.mark_scrolled_region(top, bottom);
    }

    /// Moves one full-width row, admitting primary history only at top.
    fn scroll_up_once(&mut self, top: usize, bottom: usize, height: usize) {
        let background = self.pen.bg;
        match (self.active, top, bottom == height - 1) {
            // A full primary screen can use the ring's allocation-recycling
            // fast path without moving any unaffected footer rows.
            (ActiveScreen::Primary, 0, true) => {
                self.primary
                    .scroll_up(&mut self.tracked, background, &mut self.write_clock)
            }
            // A shorter top-origin region inserts a blank at its bottom;
            // the outgoing row keeps its LineId in primary history.
            (ActiveScreen::Primary, 0, false) => self.primary.ring.scroll_top_region_up(
                bottom,
                &mut self.tracked,
                background,
                &mut self.write_clock,
            ),
            // A lower-origin or alternate region discards the outgoing row.
            (ActiveScreen::Primary, _, _) => self.primary.ring.scroll_region_up(
                top,
                bottom,
                &mut self.tracked,
                background,
                &mut self.write_clock,
            ),
            (ActiveScreen::Alternate, _, _) => self.alternate.ring.scroll_region_up(
                top,
                bottom,
                &mut self.tracked,
                background,
                &mut self.write_clock,
            ),
        }
    }

    /// Scrolls the current DEC region downward without creating history.
    pub(crate) fn scroll_region_down(&mut self, count: usize) {
        let top = self.margins.top;
        let bottom = self.margins.bottom;
        let width = self.size().0;
        // Reverse index and SD never prepend to scrollback, even when the
        // region starts at the top of the primary screen.
        let count = count.min(bottom - top + 1);
        self.note_operation(
            super::operation_metrics::Operation::ScrollDown,
            count as u64,
        );
        // Reverse index and SD never create backward scrollback, so even
        // a top-origin rectangle is confined to visible cells.
        if self.margins.left != 0 || self.margins.right != width - 1 {
            self.scroll_rect(top, bottom, count, false);
            return;
        }
        self.prepare_multicell_scroll(top, bottom, count, false);
        let background = self.pen.bg;
        // Neither screen admits history on downward movement; both can rotate
        // the band once and recycle all vacated rows in their original order.
        let Self {
            primary,
            alternate,
            active,
            tracked,
            write_clock,
            ..
        } = self;
        let ring = match active {
            ActiveScreen::Primary => &mut primary.ring,
            ActiveScreen::Alternate => &mut alternate.ring,
        };
        ring.scroll_region(top, bottom, count, false, tracked, background, write_clock);
        self.mark_scrolled_region(top, bottom);
    }

    /// Shifts a DECLRMM rectangle without changing rows outside its columns.
    fn scroll_rect(&mut self, top: usize, bottom: usize, count: usize, upward: bool) {
        let left = self.margins.left;
        let right = self.margins.right;
        // Prepare the entire source/destination band before its first copy.
        // Clearing afterward could pair an orphan with newly copied unrelated ink.
        for y in top..=bottom {
            self.prepare_column_edges(y, left, right + 1, false);
            // Existing OSC 66 policy removes blocks touched by partial-row moves;
            // blocks wholly outside the rectangle retain every fragment.
            self.erase_multicell_span(y, left, right + 1, false);
        }
        for _ in 0..count {
            if upward {
                // Forward order reads the lower source row before that
                // row is itself replaced on the next loop iteration.
                for row in top..bottom {
                    self.copy_rect_row(row + 1, row, left, right);
                }
                self.erase_span(bottom, left, right + 1, false);
            } else {
                // Reverse order gives the same source-preserving behaviour
                // when rows move down and a blank enters at the top.
                for row in (top + 1..=bottom).rev() {
                    self.copy_rect_row(row - 1, row, left, right);
                }
                self.erase_span(top, left, right + 1, false);
            }
        }
        self.mark_scrolled_region(top, bottom);
    }

    /// Marks the rows whose screen position or cells changed during a scroll.
    fn mark_scrolled_region(&mut self, top: usize, bottom: usize) {
        // A moved row changes its output position even if its own version
        // did not change, while rows beyond the margins remain reusable.
        for row in top..=bottom {
            self.screen_mut().damage.mark(row);
        }
    }

    /// Copies sparse metadata with a cell when a DEC rectangle scrolls.
    fn copy_rect_row(&mut self, source: usize, target: usize, left: usize, right: usize) {
        for column in left..=right {
            // End the immutable source borrow before taking the mutable
            // destination borrow; a row may also have sparse cluster text.
            let Some(row) = self.screen().row(source) else {
                return;
            };
            let cell = row.cells[column];
            let extra = row
                .extras
                .iter()
                .find(|item| item.column == column)
                .cloned();
            let (screen, clock) = self.screen_clock_mut();
            if let Some(dest) = screen.row_mut(target) {
                // `put` removes the destination's prior sparse metadata;
                // install only the matching source metadata afterward.
                dest.put(column, cell, clock);
                if let Some(extra) = extra {
                    dest.set_extra(extra, clock);
                }
            }
        }
    }
}
