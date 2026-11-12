//! Physical-cell checks for the D-31 tmux and emoji scenarios.
//!
//! Row text alone cannot expose an orphaned base glyph at the old column:
//! tmux backs up and redraws a completed emoji, and a stale base can remain
//! behind the visible string. These assertions inspect physical columns and
//! sparse cluster metadata before the GPU screenshot is taken.
//!
//! Wide spacers are storage for the leading glyph, not a second character.
//! Counting only leading cells makes a joined VS16 pair one grapheme while
//! still detecting the separate scalar produced by legacy mode.
//!
//! The fixture fixes tmux's client PTY at 40×6. A horizontal split has a
//! one-cell divider in column 20. `expect-column` inspects all six rows:
//! if a single pane redraw moves the divider, checking only its first row
//! would miss it. The assertion uses cell columns, rather than rendered
//! pixel x coordinates, so font metrics cannot disguise a VT width error.
//!
//! `expect-cell` can name either a plain scalar or a joined UTF-8 cluster.
//! Joined text is stored in Row.extras and the cell holds a sentinel. A
//! caller that compared the sentinel with the first Unicode scalar would
//! falsely pass even if VS16 had been discarded. Comparing the exact text
//! confirms the selector reached the same glyph origin.
//!
//! `expect-grapheme-count` checks a pane-sized range. It counts matching
//! sparse clusters and then separately looks for a plain copy of the base
//! scalar. This matters because the old tmux bug produced both a correct
//! joined emoji and a stale `✌`; counting only joined copies gave one and
//! incorrectly passed. Wide spacer cells never add to either count.
//!
//! These assertions read terminal state before image comparison. They
//! localize a failed scenario: a misplaced cell is a VT issue, whereas
//! correctly placed cells with unexpected pixels point to layout or GPU
//! rendering. The following `expect-ink` then catches a blank emoji tile.

use super::{Scenario, preview::quoted};

/// Requires one scalar at the same physical column of every visible row.
pub(super) fn expect_column(state: &Scenario, line: &str) -> Result<(), String> {
    // The script names a device-independent terminal cell column. It does
    // not depend on the current font's advance or the scenario's DPR.
    let column: usize = line
        .split_whitespace()
        .nth(1)
        .ok_or("missing column")?
        .parse()
        .map_err(|_| "invalid column")?;
    let wanted = quoted(line, "expect-column")?;
    let mut scalars = wanted.chars();
    let wanted = scalars.next().ok_or("empty column scalar")?;
    if scalars.next().is_some() {
        // The divider is one scalar. Rejecting longer strings prevents a
        // mistyped cluster from being silently reduced to its first scalar.
        return Err("expect-column needs exactly one scalar".into());
    }
    let term = state
        .terminal
        .as_ref()
        .ok_or("expect-column needs terminal")?;
    for index in 0..term.size().1 {
        // A split divider is continuous through every visible row; one
        // shifted redraw must fail even when most rows remain correct.
        let row = term.screen().row(index).ok_or("missing row")?;
        if row
            .cells
            .get(column)
            .is_none_or(|cell| cell.ch != wanted as u32)
        {
            return Err(format!("row {index}, column {column} is not {wanted:?}"));
        }
    }
    Ok(())
}

/// Checks a physical cell's scalar or sparse grapheme, including its span.
pub(super) fn expect_cell(state: &Scenario, line: &str) -> Result<(), String> {
    // Row and column are physical cell indexes. The wanted string remains
    // quoted so a multi-scalar grapheme is one scenario argument.
    let mut parts = line.split_whitespace();
    let _command = parts.next();
    let row: usize = parts
        .next()
        .ok_or("missing row")?
        .parse()
        .map_err(|_| "invalid row")?;
    let column: usize = parts
        .next()
        .ok_or("missing column")?
        .parse()
        .map_err(|_| "invalid column")?;
    let wanted = quoted(line, "expect-cell")?;
    // Compare the cell's sparse cluster when present; the visible first
    // scalar by itself cannot prove the selector joined the base.
    let term = state
        .terminal
        .as_ref()
        .ok_or("expect-cell needs terminal")?;
    let row_cells = term.screen().row(row).ok_or("missing row")?;
    if cell_matches(row_cells, column, wanted) {
        Ok(())
    } else {
        // Compact coordinates keep failure output readable for 40×6 VT
        // fixtures; dumping the entire row would hide the relevant cell.
        Err(format!("cell {row}:{column} does not contain {wanted:?}"))
    }
}

/// Counts one joined cluster and rejects an orphaned first scalar in a pane.
pub(super) fn expect_grapheme_count(state: &Scenario, line: &str) -> Result<(), String> {
    // The half-open range isolates one tmux pane. A matching cluster in the
    // other pane must not satisfy the right pane's assertion.
    let mut parts = line.split_whitespace();
    let _command = parts.next();
    let row: usize = parts
        .next()
        .ok_or("missing row")?
        .parse()
        .map_err(|_| "invalid row")?;
    let range = parts.next().ok_or("missing column range")?;
    let (start, end) = range.split_once('-').ok_or("invalid column range")?;
    // Reject indexes outside the actual row below, rather than letting a
    // silently clipped iterator certify an incomplete pane.
    let start: usize = start.parse().map_err(|_| "invalid start")?;
    let end: usize = end.parse().map_err(|_| "invalid end")?;
    let wanted = quoted(line, "expect-grapheme-count")?;
    let term = state
        .terminal
        .as_ref()
        .ok_or("expect-grapheme-count needs terminal")?;
    let row = term.screen().row(row).ok_or("missing row")?;
    let base = wanted.chars().next().ok_or("empty grapheme")? as u32;
    // The correct result has one full UTF-8 cluster. A wide spacer is not
    // counted because it has no cluster record of its own.
    let count = (start..end)
        .filter(|&column| cell_matches(row, column, wanted))
        .count();
    // Lazy rows have no raw slice; validate the range before logical iteration.
    if start > end || end > row.cells.len() {
        return Err("range outside row".into());
    }
    let lone = row
        .cells
        .iter()
        .skip(start)
        .take(end - start)
        .any(|cell| cell.ch == base);
    // tmux's two backspaces once left a separate first scalar. It can
    // coexist with the correct cluster, so both conditions are required.
    if count == 1 && !lone {
        Ok(())
    } else {
        Err(format!(
            "expected one {wanted:?}, got {count} and lone={lone}"
        ))
    }
}

/// Compares one cell without turning a wide spacer into a second glyph.
fn cell_matches(row: &puzzle_term::vt::Row, column: usize, wanted: &str) -> bool {
    // A missing physical column must fail rather than being treated as a
    // blank cell. This also bounds the scenario's requested region.
    let Some(cell) = row.cells.get(column) else {
        return false;
    };
    if cell.ch == puzzle_term::vt::Cell::CLUSTER {
        // Sparse text is authoritative for joined glyphs; Cell.ch is a
        // storage sentinel and cannot be compared to Unicode scalars.
        return row
            .extras
            .iter()
            .any(|extra| extra.column == column && extra.cluster.as_deref() == Some(wanted));
    }
    // Plain cells can only match one scalar. A bare `✌` must never satisfy
    // an assertion for the two-scalar `✌️` presentation sequence.
    wanted.chars().count() == 1 && wanted.chars().next().is_some_and(|ch| cell.ch == ch as u32)
}
