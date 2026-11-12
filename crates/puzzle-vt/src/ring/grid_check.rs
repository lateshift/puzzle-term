//! Debug/test oracle for ordinary cell layout and sparse cluster ownership (§6.2).
//!
//! Rectangular text has its own check_multicells oracle. This companion catches
//! general grid defects before they can corrupt another representation: orphaned
//! wide halves, ambiguous layout flags and stale cluster/extra references.
//! Both retained history and visible rows are checked through logical RowCells
//! access, never the recycled bytes behind a lazily erased suffix.
//!
//! The sparse-extra cursor advances once alongside the cells. A successful
//! check is linear in stored rows/cells/extras, allocates nothing and performs
//! no line-identity lookup. Errors identify coordinates without echoing text.
//! The entire module is compiled out when debug assertions are disabled.
use super::RowRing;
use crate::{Cell, CellExtra, CellFlags, Row};

/// Coordinate and static reason stay separate until the public diagnostic boundary.
type Failure = (usize, &'static str);

/// General grid ownership is independent of optional rectangular-text storage.
impl RowRing {
    /// Checks wide pairs and both directions of sparse extra/cluster ownership.
    ///
    /// Returns the first line/column and structural error; no source text is logged.
    /// This diagnostic is debug-only and is never called by production printing.
    pub fn check_grid(&self) -> Result<(), String> {
        for row in self.iter() {
            if let Err((column, reason)) = check_row(row) {
                // Formatting is confined to failure, preserving allocation-free
                // successful checks inside existing functional/allocation tests.
                return Err(format!("line {} column {column}: {reason}", row.id.0));
            }
        }
        Ok(())
    }
}

/// Merges the sorted sparse extras with logical cells in one forward pass.
fn check_row(row: &Row) -> Result<(), Failure> {
    let mut extras = row.extras.iter().peekable();
    for (column, cell) in row.cells.iter().enumerate() {
        // A duplicate or descending extra remains behind the current column.
        // Do not use binary search: malformed ordering must itself be detected.
        if extras.peek().is_some_and(|extra| extra.column < column) {
            return Err((column, "duplicate or unsorted sparse extra"));
        }
        let extra = if extras.peek().is_some_and(|extra| extra.column == column) {
            extras.next()
        } else {
            None
        };
        check_cell(row, column, cell, extra).map_err(|reason| (column, reason))?;
    }
    if let Some(extra) = extras.next() {
        // This also catches a second reference at the last logical column.
        return Err((extra.column, "sparse extra outside row or out of order"));
    }
    Ok(())
}

/// One cell must have exactly the partners/storage its representation promises.
fn check_cell(
    row: &Row,
    column: usize,
    cell: &Cell,
    extra: Option<&CellExtra>,
) -> Result<(), &'static str> {
    let lead = cell.flags.contains(CellFlags::WIDE);
    let spacer = cell.flags.contains(CellFlags::WIDE_SPACER);
    let block = cell.flags.contains(CellFlags::MULTICELL);
    if lead && spacer || block && (lead || spacer) {
        return Err("conflicting cell layout flags");
    }
    if lead
        && !row
            .cells
            .get(column + 1)
            .is_some_and(|next| next.flags.contains(CellFlags::WIDE_SPACER))
    {
        return Err("wide lead has no spacer");
    }
    if spacer
        && !column
            .checked_sub(1)
            .and_then(|previous| row.cells.get(previous))
            .is_some_and(|previous| previous.flags.contains(CellFlags::WIDE))
    {
        return Err("wide spacer has no lead");
    }
    // Extra presence is exact, including decoration-only entries. A stale flag
    // can suppress fallback text or make a future join consume unrelated data.
    if cell.flags.contains(CellFlags::HAS_EXTRAS) != extra.is_some() {
        return Err("cell extra flag differs from sparse entry");
    }
    let cluster = extra.and_then(|extra| extra.cluster.as_deref());
    // Block keys can numerically equal a sentinel; MULTICELL selects their
    // separate store. Ordinary CLUSTER cells must own actual nonempty text.
    if (!block && cell.ch == Cell::CLUSTER) != cluster.is_some() {
        return Err("cluster sentinel differs from cluster extra");
    }
    if cluster.is_some_and(str::is_empty) || spacer && cluster.is_some() {
        return Err("empty cluster or cluster owned by spacer");
    }
    Ok(())
}

#[cfg(test)]
/// Deliberately broken witnesses prove each direction of the diagnostic.
mod tests;
