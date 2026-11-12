//! Rewraps retained logical lines after a grid-width change (§6.6).
//!
//! A row marked WRAPPED continues the same logical line. Reflow walks the
//! old rows once, copies occupied cells into new-width rows and keeps a
//! compact offset map for the cursor and tracked anchors. Blank cells with
//! a nondefault background count as content; unprinted default blanks at
//! each old row's tail do not. A wide lead and spacer are moved together.
//! The renderer will rebuild from full damage after this operation.
//!
//! A logical line is a run of physical rows whose preceding row has the
//! WRAPPED bit. A hard line ending is represented by a row without that
//! bit. Reflow is therefore a streaming operation: read one old row, copy
//! its occupied cells into a destination row, and only end the destination
//! logical line when the old row has a hard ending. Empty hard rows are
//! retained because they represent real vertical space in terminal output.
//!
//! Occupancy differs from visible ink. A printed space has `ch = ' '`
//! and is content. An erased cell with a nondefault background is also
//! content: dropping it would shorten a colored banner after resize.
//! A never-written default blank has `ch = 0` and can be trimmed from an
//! old row's tail. Wide leads and spacers form an indivisible two-column
//! unit; if the pair does not fit, the destination row ends first.
//!
//! Physical rows receive fresh monotonic LineIds because one old row may
//! split into several, or several old rows may merge. Existing anchors
//! point at old IDs, so the builder records each old row's logical-line
//! number and starting offset. It also records the new row at each output
//! offset. A tracked cell maps through those two tables into a new ID and
//! column. The cursor uses the same mapping. Eviction after reflow turns
//! mapped handles into Gone; it must not make them point to recycled ink.
//!
//! History is still bounded after a resize. Its byte budget depends on
//! the new width, so the effective line cap is recomputed from the user's
//! configured maximum and the full-width cell cost. Sparse cluster extras
//! are counted by the existing exact-byte pass only when present. The
//! visible tail is always retained, even if it exceeds the history budget.
//!
//! The plain resize path handles the alternate screen and primary resize
//! when reflow is disabled. It changes each row's cell vector width but
//! does not join WRAPPED rows. A clipped wide lead at the new right edge
//! is erased so rendering never sees a half-character without its spacer.

use std::collections::VecDeque;

use crate::{
    cell::{Cell, CellFlags, PackedColor},
    row::{Row, RowFlags},
    tracked::{LineId, PositionState, TrackedPositions},
};

use super::RowRing;

/// Atomic rectangular packing is separate from the ordinary reflow hot path.
mod multicell;

/// Old row identity and its first cell's offset in one logical line.
struct OldLocation {
    /// Original stable identity, used by existing tracked handles.
    id: LineId,
    /// Sequence number of the joined logical line.
    logical: usize,
    /// Column offset within the joined line before this old row.
    start: usize,
    /// Number of occupied old columns retained from this row.
    length: usize,
}

/// New row identity and the first logical offset it contains.
struct NewLocation {
    /// Joined line number shared with the old location map.
    logical: usize,
    /// Offset before the first content cell on this new row.
    start: usize,
    /// Fresh monotonic identity allocated to the rewrapped row.
    id: LineId,
}

/// Temporary builder that keeps one writable destination row at a time.
struct Builder {
    /// Finished rows in document order, oldest first.
    rows: VecDeque<Box<Row>>,
    /// Destination row currently receiving cells.
    current: Box<Row>,
    /// Next free destination column.
    column: usize,
    /// Logical-line index, incremented at each hard break.
    logical: usize,
    /// Number of old content columns visited in this logical line.
    offset: usize,
    /// Width of each new row.
    width: usize,
    /// Next identity, never reused from the old ring.
    next_id: u64,
    /// Old-to-logical mapping for sparse anchors.
    old: Vec<OldLocation>,
    /// Logical-to-new mapping for sparse anchors.
    new: Vec<NewLocation>,
}

impl Builder {
    /// Allocates a first destination row and reserves metadata per old row.
    fn new(width: usize, next_id: u64, old_count: usize) -> Self {
        // The builder starts with a real row even if all old text is blank.
        let first = Box::new(Row::new(LineId(next_id), width));
        Self {
            rows: VecDeque::with_capacity(old_count),
            current: first,
            column: 0,
            logical: 0,
            offset: 0,
            width,
            next_id: next_id.saturating_add(1),
            old: Vec::with_capacity(old_count),
            new: vec![NewLocation {
                logical: 0,
                start: 0,
                id: LineId(next_id),
            }],
        }
    }

    /// Copies occupied cells from one old row, continuing soft lines.
    fn consume(&mut self, source: &Row) {
        // Record the old origin before appending this row's content; that
        // offset later maps a cursor or zone anchored inside the source.
        let length = occupied_length(source);
        self.old.push(OldLocation {
            id: source.id,
            logical: self.logical,
            start: self.offset,
            length,
        });
        // An empty hard row still contributes its line break, not any cells.
        if length == 0 {
            self.current.version = self.current.version.max(source.version);
        }
        let mut column = 0;
        while column < length {
            // Cell flags, not Unicode scalar values, define stored width.
            let cell = source.cells[column];
            // A valid wide lead owns two adjacent columns; move them as a
            // unit so no width change can strand a spacer at the new edge.
            let wide = cell.flags.contains(CellFlags::WIDE)
                && source
                    .cells
                    .get(column + 1)
                    .is_some_and(|next| next.flags.contains(CellFlags::WIDE_SPACER));
            let span = if wide { 2 } else { 1 };
            if self.column + span > self.width {
                // A wide pair must never straddle a destination boundary.
                self.wrap();
            }
            self.copy_cell(source, column, span);
            column += span;
            self.offset += span;
        }
        // Only a hard line ending finalizes the current destination row.
        if !source.flags.contains(RowFlags::WRAPPED) {
            self.end_line();
        }
    }

    /// Copies one scalar or intact wide pair with its sparse metadata.
    fn copy_cell(&mut self, source: &Row, column: usize, span: usize) {
        // Cells are POD-sized value records, while rare cluster/link fields
        // are cloned only for the columns that actually own an extra.
        self.current.cells[self.column] = source.cells[column];
        // A source row can split; every destination fragment inherits its stamp.
        self.current.version = self.current.version.max(source.version);
        // Summary follows every copied fragment, including a bare placeholder.
        self.current.flags |= source.flags & RowFlags::IMAGE_PLACEHOLDERS;
        if span == 2 {
            self.current.cells[self.column + 1] = source.cells[column + 1];
        }
        if let Ok(index) = source
            .extras
            .binary_search_by_key(&column, |extra| extra.column)
        {
            // Sparse entries remain sorted because source columns are read
            // in ascending order and destination columns only advance.
            let mut extra = source.extras[index].clone();
            extra.column = self.column;
            self.current.copy_extra(extra);
        }
        self.column += span;
    }

    /// Starts another physical row of the same logical line.
    fn wrap(&mut self) {
        // The row being finished continues onto the next new row.
        self.current.flags.insert(RowFlags::WRAPPED);
        self.rows.push_back(std::mem::replace(
            &mut self.current,
            Box::new(Row::new(LineId(self.next_id), self.width)),
        ));
        self.new.push(NewLocation {
            // The following row begins at the next unread logical offset.
            logical: self.logical,
            start: self.offset,
            id: LineId(self.next_id),
        });
        self.next_id = self.next_id.saturating_add(1);
        self.column = 0;
    }

    /// Ends one hard line and prepares the next old row's logical group.
    fn end_line(&mut self) {
        // Push even an empty row: hard blank lines remain visible after
        // narrowing or widening a terminal, as they do in xterm.
        self.rows.push_back(std::mem::replace(
            &mut self.current,
            Box::new(Row::new(LineId(self.next_id), self.width)),
        ));
        self.logical += 1;
        // A hard break starts a new offset coordinate system.
        self.offset = 0;
        self.column = 0;
        self.new.push(NewLocation {
            logical: self.logical,
            start: 0,
            id: LineId(self.next_id),
        });
        self.next_id = self.next_id.saturating_add(1);
    }

    /// Maps a retained old cell coordinate onto its new physical row.
    fn locate(&self, line: LineId, column: usize) -> PositionState {
        // Sparse anchors can use a linear old-row lookup; the bulk cell
        // walk never calls this function for each printed cell.
        let Some(old) = self.old.iter().find(|item| item.id == line) else {
            return PositionState::Gone;
        };
        let offset = old.start + column.min(old.length);
        // Several new rows may cover one logical line. Choose the last
        // start offset not beyond the old cell's logical offset.
        let Some(target) = self
            .new
            .iter()
            .take_while(|item| item.logical <= old.logical)
            .filter(|item| item.logical == old.logical && item.start <= offset)
            .last()
        else {
            return PositionState::Gone;
        };
        PositionState::Present {
            line: target.id,
            column: offset.saturating_sub(target.start).min(self.width - 1),
        }
    }
}

/// Finds the last cell with explicit ink or a nondefault erased background.
fn occupied_length(row: &Row) -> usize {
    // Only trailing never-written default blanks are disposable; a colored
    // erased cell or explicit space is part of the logical line.
    row.cells
        .iter()
        .rposition(|cell| {
            cell.ch != 0
                || cell.bg != PackedColor::DEFAULT
                || cell
                    .flags
                    .intersects(CellFlags::WIDE | CellFlags::WIDE_SPACER | CellFlags::MULTICELL)
        })
        .map_or(0, |last| last + 1)
}

impl RowRing {
    /// Resizes rows in place when reflow is disabled or on the alternate grid.
    pub fn resize_plain(
        &mut self,
        columns: usize,
        visible_rows: usize,
        tracked: &mut TrackedPositions,
        clock: &mut crate::WriteClock,
    ) {
        let columns = columns.max(2);
        // A resize invalidates inactive-width capacity; the new geometry grows on demand.
        self.spares.clear();
        self.spare_bytes = 0;
        // One row and two columns are the minimum cursor-bearing geometry.
        let visible_rows = visible_rows.max(1);
        self.clip_multicells(columns, visible_rows, self.scrollback_lines == 0, clock);
        for row in &mut self.rows {
            // Plain truncation has one destination boundary. Use the same
            // atomic cut predicate as edits before discarding its outside half;
            // row-local extras must retire along with the clipped wide lead.
            let clipped_lead = row.erase_split_wide_at(columns, Cell::default(), clock, false);
            row.cells.resize(columns, Cell::default());
            row.extras.retain(|extra| extra.column < columns);
            // Height-only changes preserve source content and its stamp.
            if columns != self.columns {
                row.version = clock.written::<true>(
                    row,
                    columns
                        .min(self.columns)
                        .saturating_sub(usize::from(clipped_lead))
                        ..columns.max(self.columns),
                );
            }
        }
        self.columns = columns;
        self.visible_rows = visible_rows;
        while self.rows.len() < visible_rows {
            // Growing the primary view first exposes retained history;
            // only a view taller than all retained rows needs new blanks.
            self.rows
                .push_back(Box::new(Row::new(LineId(self.next_id), columns)));
            self.next_id = self.next_id.saturating_add(1);
        }
        // The alternate ring has no history; a shorter grid drops bottom
        // rows, while primary history remains subject to its normal caps.
        if self.scrollback_lines == 0 {
            while self.rows.len() > visible_rows {
                if let Some(mut row) = self.rows.pop_back() {
                    self.retire_multicells(&mut row);
                    tracked.evict(row.id);
                }
            }
        } else {
            self.trim_history(tracked);
        }
        self.compact_multicells();
        self.enforce_byte_limit(tracked);
    }

    /// Rewraps every retained logical line and remaps sparse anchors.
    pub fn resize_reflow(
        &mut self,
        columns: usize,
        visible_rows: usize,
        tracked: &mut TrackedPositions,
        cursor: Option<(LineId, usize)>,
    ) -> Option<(usize, usize)> {
        let columns = columns.max(2);
        // Rewrapped rows replace the old geometry, including inactive allocations.
        self.spares.clear();
        self.spare_bytes = 0;
        let visible_rows = visible_rows.max(1);
        if self
            .multicells
            .as_deref()
            .is_some_and(|store| store.iter().next().is_some())
        {
            // No rectangle is flattened into scalar keys or reservation spaces.
            return multicell::resize(self, columns, visible_rows, tracked, cursor);
        }
        // The first old visible cell defines the history boundary after rewrap.
        // Reflow may produce more/fewer physical grid rows at the new width.
        let visible_origin = self.visible(0).map(|row| row.id);
        let old = std::mem::take(&mut self.rows);
        // Move ownership of old rows out of the ring so the builder may
        // allocate new-width rows without borrowing the ring concurrently.
        let mut builder = Builder::new(columns, self.next_id, old.len());
        for row in &old {
            // The row walk is in document order, including retained history.
            builder.consume(row);
        }
        // The final old row may have WRAPPED set without a successor after
        // eviction; keep its partial logical line instead of dropping it.
        if builder.rows.is_empty()
            || builder
                .new
                .last()
                .is_some_and(|last| last.logical == builder.logical && builder.column > 0)
        {
            builder.rows.push_back(builder.current.clone());
        }
        let cursor_state = cursor.map(|(line, col)| builder.locate(line, col));
        let visible_state = visible_origin.map(|line| builder.locate(line, 0));
        // Both cursor and tracked markers use the same logical offset rule.
        tracked.remap(|line, col| builder.locate(line, col));
        self.next_id = builder.next_id;
        self.rows = builder.rows;
        // Only after replacing storage do the ring's width/height reflect
        // the new geometry used by subsequent visible-row indexing.
        self.columns = columns;
        // Primary height policy chooses the visible tail using rebuilt identities.
        self.apply_reflowed_height(visible_rows, tracked, cursor_state, visible_state)
    }

    /// Applies the configured history line and byte ceilings after reflow.
    pub(super) fn trim_history(&mut self, tracked: &mut TrackedPositions) {
        // The byte cap changes with width, but the requested line ceiling
        // remains the user's original setting rather than a previous cap.
        let row_bytes = Self::row_storage_bytes(self.columns);
        self.scrollback_lines = self.max_lines.min(self.scrollback_bytes / row_bytes);
        // Reflow changes widths and sparse allocations of historical rows.
        // Recompute once here; ordinary LF updates exactly two boundary rows.
        self.history_bytes = self
            .rows
            .iter()
            .take(self.scrollback_len())
            .map(|row| Self::retained_row_bytes(row))
            .sum();
        while self.scrollback_len() > self.scrollback_lines {
            // Retiring the handle precedes dropping the row allocation.
            if let Some(mut row) = self.rows.pop_front() {
                self.history_bytes = self
                    .history_bytes
                    .saturating_sub(Self::retained_row_bytes(&row));
                self.retire_multicells(&mut row);
                tracked.evict(row.id);
            }
        }
        self.enforce_byte_limit(tracked);
    }
}
