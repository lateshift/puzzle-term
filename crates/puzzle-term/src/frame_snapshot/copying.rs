//! Reuses sparse payload capacities while sealing row copies (P1/D-50).
//!
//! Copying cells and locating the displayed caret are separate operations. VT
//! cursor row numbers refer to the application's physical screen, whereas a
//! snapshot may show history or omit rows after a user fold. Caret identity must
//! therefore be resolved against the rows this slot actually owns.
//! Rectangular text blocks may expose a lower fragment while their first row is
//! outside the viewport. Their sealed extent supplies the caret's display row;
//! a missing identity/extent hides it rather than drawing on unrelated text.
//! Nothing here queries the live parser after publication or moves VT's cursor.
use super::{CellFlags, CursorSnapshot, Row, Terminal};
use puzzle_vt::row::CellExtra;

/// Reuses sparse cluster strings when a damaged row is copied into its snapshot slot.
pub(super) fn copy_extras(target: &mut Row, source: &Row) {
    // Shorter rows drop only surplus records; shared positions retain their allocations.
    // A removed cluster releases its payload; matching slots below can reuse theirs.
    target.extras.truncate(source.extras.len());
    for (index, extra) in source.extras.iter().enumerate() {
        // Exhaustive matching makes a future sparse field a compile error here.
        let CellExtra {
            cluster,
            column,
            underline_color,
            link_id,
        } = extra;
        if let Some(owned) = target.extras.get_mut(index) {
            // Option delegates Some→Some copying to String's capacity-preserving method.
            // None removes a scalar attachment; Some retains the old String capacity.
            owned.cluster.clone_from(cluster);
            // Sparse slots may move columns or change paint/link metadata after an edit.
            // These values belong to this publication, never to the live row afterward.
            owned.column = *column;
            owned.underline_color = *underline_color;
            owned.link_id = *link_id;
        } else {
            // New sparse positions allocate only during cold growth, as ordinary cells do.
            target.extras.push(extra.clone());
        }
    }
}

/// Blanks, hidden cells and wide spacers cannot keep a text-ink timer alive.
pub(super) fn row_blinks(row: &Row) -> bool {
    use puzzle_vt::cell::Attrs;
    // A row summary avoids scanning all unchanged cell arrays on later publications.
    row.cells.iter().any(|cell| {
        cell.attrs.contains(Attrs::BLINK)
            && !cell.attrs.contains(Attrs::HIDDEN)
            && !cell.flags.contains(CellFlags::WIDE_SPACER)
            // For blocks ch is a slot, so zero or 32 is not a blank scalar.
            && (cell.flags.contains(CellFlags::MULTICELL)
                || (cell.ch != 0 && cell.ch != u32::from(' ')))
    })
}

/// Samples width while the Session owner seals this slot.
pub(super) fn cursor_from_terminal(term: &Terminal) -> Option<CursorSnapshot> {
    let view = term.buffer().cursor()?;
    let row_index = term.cursor.row;
    let cell = *term
        // The renderer cannot query VT; width belongs to the same sealed grid.
        .screen()
        .row(row_index)?
        .cells
        .get(view.column)?;
    let wide = cell.flags.contains(CellFlags::WIDE);
    Some(CursorSnapshot {
        view,
        row_index,
        wide,
        cell,
    })
}

/// Cursor identity is resolved against this sealed projection, never live row indices.
impl super::FrameSnapshot {
    /// Cursor isolation follows source identity or the sealed rectangle extent.
    /// This small per-row query runs before row-copy cache comparison.
    pub(super) fn cursor_column_at(&self, index: usize, line: puzzle_vt::LineId) -> Option<usize> {
        // Rectangle cursors can name a lower fragment; ordinary cursors match
        // the retained identity even when it moved to another display index.
        self.cursor.and_then(|cursor| {
            if cursor.cell.flags.contains(CellFlags::MULTICELL) {
                self.block_cursor_column(index, cursor)
            } else {
                (cursor.view.line == line).then_some(cursor.view.column)
            }
        })
    }

    /// History/folding can hide the caret; atomic fragments share one visual extent.
    pub(super) fn place_cursor(&mut self) {
        self.cursor = self.cursor.and_then(|mut cursor| {
            // Ordinary rows resolve directly; a block can instead name a lower fragment.
            cursor.row_index = self
                .metadata
                .iter()
                .position(|row| row.id == Some(cursor.view.line))
                // A hidden/offscreen source has no visible caret unless a block fragment remains.
                .or_else(|| {
                    self.multicells.as_deref().and_then(|blocks| {
                        blocks
                            .records
                            .iter()
                            .find(|block| {
                                cursor.cell.flags.contains(CellFlags::MULTICELL)
                                    && block.key == cursor.cell.ch
                            })
                            .map(|block| block.row)
                    })
                })?;
            Some(cursor)
        });
    }
}
