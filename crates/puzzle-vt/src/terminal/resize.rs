//! Grid resizing with primary reflow and alternate truncation (§6.6).
//!
//! The primary ring owns scrollback and joins rows at WRAPPED boundaries.
//! The alternate ring belongs to a full-screen program and never reflows.
//! Both dimensions are clamped before touching storage so a zero-sized
//! surface cannot produce an invalid cursor or margin. Platform PTY resize
//! throttling is deliberately outside this pure VT crate (§4.1).
//!
//! Column changes run primary reflow before row-height adjustment so a
//! cursor's old LineId still identifies its logical offset. Reflow assigns
//! fresh physical row IDs, then remaps tracked handles. The alternate grid
//! keeps columns as stored and clips or pads them; full-screen applications
//! repaint after the host reports a PTY resize.
//! A new size invalidates all cached render rows, resets DEC margins and
//! pending wrap, and extends tab stops at eight-column intervals. The
//! cursor is clamped only after its logical position has been remapped.
//! Mode 2048's in-band report is emitted here because it describes the new
//! grid and pixel geometry; the platform separately resizes the PTY.

use super::{ActiveScreen, Terminal, TerminalEvent};

impl Terminal {
    /// Resizes both grids and emits the optional in-band mode-2048 report.
    pub fn resize(&mut self, columns: usize, rows: usize) {
        let (columns, rows) = (columns.max(2), rows.max(1));
        let (old_columns, old_rows) = self.size();
        if (columns, rows) == (old_columns, old_rows) {
            return;
        }
        // Resize each independent line-ID domain before restoring the active one.
        let mapped = self.resize_primary(columns, rows, old_columns);
        // Alternate contents keep their cell positions; applications redraw
        // after the host reports a new PTY size.
        self.tracked.set_screen(ActiveScreen::Alternate);
        self.alternate
            .ring
            .resize_plain(columns, rows, &mut self.tracked, &mut self.write_clock);
        self.tracked.set_screen(self.active);
        self.relocate_source_watches();
        // Shell markers share the same registry remapping as viewport anchors.
        self.zones.sync(&self.tracked);
        if self.active == ActiveScreen::Primary
            && let Some((row, column)) = mapped
        {
            self.cursor.row = row;
            self.cursor.column = column;
        }
        self.cursor.row = self.cursor.row.min(rows - 1);
        self.cursor.column = self.cursor.column.min(columns - 1);
        self.cursor.pending_wrap = false;
        // Every tab vector extension starts with xterm's eight-column
        // defaults while retained stops at old columns remain unchanged.
        self.tabs.resize_with(columns, || false);
        for column in old_columns..columns {
            self.tabs[column] = column != 0 && column % 8 == 0;
        }
        // Resizing resets margins to the full grid but must preserve the
        // remapped cursor, unlike DECSTBM's home-cursor operation.
        self.margins.top = 0;
        self.margins.bottom = rows - 1;
        self.margins.left = 0;
        self.margins.right = columns - 1;
        self.primary.damage = crate::damage::Damage::new(rows);
        self.alternate.damage = crate::damage::Damage::new(rows);
        self.events
            .push(TerminalEvent::GridResized { columns, rows });
        crate::protocols::rockorager::resized(self, columns, rows);
    }
    /// Primary reflow alone remaps logical lines; alternate anchors remain untouched.
    fn resize_primary(
        &mut self,
        columns: usize,
        rows: usize,
        old_columns: usize,
    ) -> Option<(usize, usize)> {
        // The old LineId is needed before reflow assigns fresh identities
        // to all physical rows of each logical line.
        let primary_cursor = if self.active == ActiveScreen::Primary {
            self.primary
                .row(self.cursor.row)
                .map(|row| (row.id, self.cursor.column))
        } else {
            // A full-screen application must not discard the hidden shell's prompt.
            let cursor = self.saved[0].map_or(super::Cursor::default(), |saved| saved.cursor);
            self.primary
                .row(cursor.row)
                .map(|row| (row.id, cursor.column))
        };
        // Primary reflow must never rewrite an alternate-screen image anchor.
        self.tracked.set_screen(ActiveScreen::Primary);
        let mapped = if columns != old_columns && self.reflow_enabled {
            // Fresh physical row IDs after rewrap should not be mistaken
            // for a content rewrite by tracked selections (§6.9).
            self.primary
                .ring
                .watch_reflow_discards(columns, rows, &mut self.write_clock);
            self.note_reflow();
            self.reflow_generation = self.reflow_generation.wrapping_add(1);
            self.primary
                .ring
                .resize_reflow(columns, rows, &mut self.tracked, primary_cursor)
        } else {
            self.primary.ring.resize_primary_plain(
                columns,
                rows,
                &mut self.tracked,
                primary_cursor,
                &mut self.write_clock,
            )
        };
        if self.active == ActiveScreen::Alternate
            && let (Some(saved), Some((row, column))) = (&mut self.saved[0], mapped)
        {
            // DECRC after leaving the alternate screen restores the resized shell position.
            saved.cursor.row = row;
            saved.cursor.column = column;
        }
        mapped
    }
}

#[cfg(test)]
mod tests;
