//! Primary height changes keep the cursor's row, not the unused bottom of the grid.
//!
//! Terminals remove rows below the cursor before transferring top rows to
//! history; growth pulls at most the added height from history (design §6.6).
//! Alternate resize stays on resize_plain and never uses this policy.
//!
//! The policy follows xterm's screen resize (screen.c, ThomasDickey/xterm-snapshots
//! revision bccd466414dc4ecb264a3824a6f1e41bebe65214).
use super::RowRing;
use crate::{
    row::Row,
    tracked::{LineId, PositionState, TrackedPositions},
};

impl RowRing {
    /// Applies primary height policy before a non-reflowing width adjustment.
    pub(crate) fn resize_primary_plain(
        &mut self,
        columns: usize,
        rows: usize,
        tracked: &mut TrackedPositions,
        cursor: Option<(LineId, usize)>,
        clock: &mut crate::WriteClock,
    ) -> Option<(usize, usize)> {
        // Reject whole rectangles before truncation can strand their lower cells.
        self.clip_multicells(columns.max(2), rows.max(1), false, clock);
        // Resolve the old identity before geometry can alter the visible tail.
        let mapped = self.resize_primary_height(rows, tracked, cursor);
        // Height is already final; this updates columns and their byte ceiling.
        self.resize_plain(columns, rows, tracked, clock);
        mapped
    }

    /// Counts rewrapped visible rows before applying cursor-aware height policy.
    pub(super) fn apply_reflowed_height(
        &mut self,
        rows: usize,
        tracked: &mut TrackedPositions,
        cursor: Option<PositionState>,
        origin: Option<PositionState>,
    ) -> Option<(usize, usize)> {
        // Width changes can produce extra physical rows. Counting only the old
        // height would put a fitting cursor in history before trimming bottom blanks.
        if let Some(PositionState::Present { line, .. }) = origin
            && let Some(index) = self.index_of(line)
        {
            self.visible_rows = self.rows.len() - index;
        }
        let cursor = cursor.and_then(|state| match state {
            PositionState::Present { line, column } => Some((line, column)),
            PositionState::Gone => None,
        });
        // Both cursor and sparse anchors already name the builder's new identities.
        self.resize_primary_height(rows, tracked, cursor)
    }

    /// Retires bottom rows first, then makes only the required top rows historical.
    ///
    /// Trailing blank rows are trimmed without trimming the cursor pin. Top
    /// movement is max(cursor_y + 1 - new_height, 0), as in xterm screen.c
    /// (bccd466 L508–513, L2338–2369). When nonblank rows exist below the cursor
    /// they may be truncated rather than moving a cursor that already fits.
    /// Growth follows xterm L2363–2369, pulling history even above the bottom,
    /// as design §6.6 explicitly requires.
    pub(super) fn resize_primary_height(
        &mut self,
        rows: usize,
        tracked: &mut TrackedPositions,
        cursor: Option<(LineId, usize)>,
    ) -> Option<(usize, usize)> {
        // A grid always keeps at least one visible row.
        let rows = rows.max(1);
        let cursor_index = cursor.and_then(|(id, _)| self.index_of(id));
        // A cursor-free ring caller retains bottom alignment, as before.
        let below = cursor_index.map_or(0, |index| self.rows.len() - index - 1);
        // Only rows below the cursor may be dropped; the cursor row stays visible.
        let discard = self.visible_rows.saturating_sub(rows).min(below);
        for _ in 0..discard {
            self.retire_bottom(tracked);
        }
        // Everything above the surviving rows is already history or becomes it.
        self.visible_rows = rows;
        while self.rows.len() < rows {
            // Pull retained history first. Only the remaining deficit is new blank space.
            self.rows
                .push_back(Box::new(Row::new(LineId(self.next_id), self.columns)));
            self.next_id = self.next_id.saturating_add(1);
        }
        // Zero history is still a primary screen: excess TOP rows are evicted here.
        self.trim_history(tracked);
        self.compact_multicells();
        self.trim_spares();
        let (id, column) = cursor?;
        let index = self.index_of(id)?;
        Some((
            index.saturating_sub(self.scrollback_len()).min(rows - 1),
            column,
        ))
    }

    /// A removed bottom row retires its identity; a later blank never resurrects it.
    fn retire_bottom(&mut self, tracked: &mut TrackedPositions) {
        if let Some(mut row) = self.rows.pop_back() {
            // Selections/images anchored to truncated rows become Gone, not aliases.
            self.retire_multicells(&mut row);
            tracked.evict(row.id);
        }
    }
}
