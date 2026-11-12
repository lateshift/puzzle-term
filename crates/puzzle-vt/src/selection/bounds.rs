//! Normalizes gesture endpoints once for both highlighting and copy (§6.9).
//!
//! Word and line expansion use the same physical cell boundaries in both
//! consumers. Semantic marks use a cursor boundary after the final character;
//! converting it to the inclusive selection convention avoids copying the
//! first character of the next prompt. Wide spacers expand to their lead cell.
//!
//! Gesture coordinates name cells, not byte offsets in the Unicode string.
//! A wide glyph has a leading cell and a spacer; either half belongs to the
//! same copyable cluster. Expansion therefore finishes by normalizing those
//! two outer boundaries instead of discarding a clicked spacer.
//!
//! Logical lines are defined by the WRAPPED bit on each predecessor row.
//! Triple-click expands to the first and last physical rows of that group.
//! Word lookup uses the same continuity: a word split by the right margin
//! remains one word, while a hard newline always terminates the scan.
//! Whitespace and the configured punctuation set form the separator class.
//! Clicking a separator selects its contiguous class rather than its neighbour.
//!
//! OSC 133 marks differ from pointer coordinates: C and D are cursor positions
//! between characters. D at column zero belongs to the next physical row.
//! Its preceding row is the last selected row, and the copy owner remembers
//! the terminating newline separately. This prevents the following prompt
//! from becoming part of the range or its version-validation witnesses.
//! Missing marks are normal when shell integration is disabled or incomplete.
//! No fallback guesses a command boundary from prompt-looking text.

use super::{SelectPoint, SelectionKind};
use crate::view::CellText;
use crate::{CellFlags, Terminal, zones::ZoneKind};

/// Expands inclusive cell endpoints according to a user gesture.
/// Missing cells or incomplete OSC 133 output zones return None.
pub fn expanded(
    term: &Terminal,
    kind: SelectionKind,
    mut first: SelectPoint,
    mut last: SelectPoint,
    separators: &str,
) -> Option<(SelectPoint, SelectPoint)> {
    // BufferView resolves retained IDs through the active screen only.
    let view = term.buffer();
    let (width, _) = term.size();
    // Expand first, then normalize Unicode cluster boundaries for every kind.
    match kind {
        SelectionKind::Word => {
            // Independently grow the outer word ends, retaining intervening text.
            first = word_edge(term, first, separators, false)?;
            last = word_edge(term, last, separators, true)?;
        }
        SelectionKind::Line => {
            // A triple click selects every physical segment of the logical line.
            // WRAPPED flags connect physical segments without introducing a hard break.
            first.line = *view.logical_line(first.line)?.start();
            first.column = 0;
            last.line = *view.logical_line(last.line)?.end();
            last.column = width.saturating_sub(1);
        }
        SelectionKind::Semantic => {
            // Shell marks describe cursor boundaries, rather than inclusive cells.
            let (start, end) = output_span(term, first)?;
            first = start;
            last = end;
            // Column zero is a boundary before the next row, not a selected blank cell.
            if end.column == 0 {
                // Exclude the next prompt row; extraction remembers its hard newline.
                // Use retained order: region edits can leave gaps in numeric line IDs.
                let index = term.screen().ring.index_of(end.line)?;
                last.line = term.screen().ring.retained(index.checked_sub(1)?)?.id;
                last.column = width - 1;
            } else {
                // A same-row D points immediately after the final output cell.
                last.column = end.column - 1;
            }
        }
        SelectionKind::Simple | SelectionKind::Block => {}
    }
    normalize(term, kind, first, last)
}

/// Includes outer Unicode and rectangle footprints after gesture-specific expansion.
fn normalize(
    term: &Terminal,
    kind: SelectionKind,
    mut first: SelectPoint,
    mut last: SelectPoint,
) -> Option<(SelectPoint, SelectPoint)> {
    let view = term.buffer();
    let width = term.size().0;
    // Copying either half of a cluster includes the entire character.
    // A clicked spacer expands left instead of becoming an empty copy.
    if first.column < width
        && view
            .line(first.line)?
            .cell(first.column)?
            .raw
            .flags
            .contains(CellFlags::WIDE_SPACER)
    {
        first.column = first.column.saturating_sub(1);
    }
    // Selecting a wide lead includes its second cell in the painted interval.
    if last.column < width
        && view
            .line(last.line)?
            .cell(last.column)?
            .raw
            .flags
            .contains(CellFlags::WIDE)
    {
        last.column = (last.column + 1).min(width - 1);
    }
    // Normal gestures select the entire logical block; rectangular gestures keep
    // their physical box and the extractor emits any intersected payload once.
    if kind != SelectionKind::Block && term.screen().ring.multicells.is_some() {
        (first, last) = super::multicell::expand(term, first, last);
    }
    Some((first, last))
}

/// Expands a word without crossing the configured separator set.
///
/// The selected character determines whether the run is word content or
/// separators; selecting punctuation should not jump into its neighbour.
fn word_edge(
    term: &Terminal,
    point: SelectPoint,
    separators: &str,
    right: bool,
) -> Option<SelectPoint> {
    let ring = &term.screen().ring;
    // The ring index, unlike a LineId increment, follows the current retained order.
    let mut row = ring.index_of(point.line)?;
    // Word clicks outside the right edge resolve to the last real grid cell.
    let mut column = point.column.min(term.size().0.saturating_sub(1));
    let class = |row: usize, column: usize| {
        let line = term.buffer().line(ring.retained(row)?.id)?;
        // A wide spacer belongs to its lead character, including its word class.
        if let Some(block) = line.multicell(column) {
            // Every covered cell has the same indivisible word classification.
            let ch = block.text.chars().next()?;
            return Some(!ch.is_whitespace() && !separators.contains(ch));
        }
        let cell = line.cell(column)?;
        let cell = if cell.width == 0 {
            line.cell(column.saturating_sub(1))?
        } else {
            cell
        };
        // Classification uses the cluster’s base scalar; marks do not split words.
        let ch = match cell.text {
            CellText::Scalar(ch) => ch,
            CellText::Cluster(text) => text.chars().next()?,
            CellText::Empty => ' ',
        };
        // Whitespace always separates words even if omitted from the user string.
        Some(!ch.is_whitespace() && !separators.contains(ch))
    };
    // Preserve the clicked class: punctuation runs and words are separate regions.
    let is_word = class(row, column)?;
    loop {
        // Adjacency crosses only soft wraps in the requested direction.
        let next = word_neighbour(term, row, column, right);
        // A hard line edge has no adjacent word cell in this logical line.
        let Some((next_row, next_column)) = next else {
            break;
        };
        // A missing retained neighbour or class change terminates expansion.
        if class(next_row, next_column) != Some(is_word) {
            break;
        }
        // Advance in physical coordinates only after accepting the adjacent cell.
        row = next_row;
        column = next_column;
    }
    Some(SelectPoint {
        line: ring.retained(row)?.id,
        column,
    })
}

/// Resolves OSC 133 output to half-open cursor boundaries, excluding the prompt.
/// Both marks must be retained and belong to the same completed command.
pub fn output_span(term: &Terminal, at: SelectPoint) -> Option<(SelectPoint, SelectPoint)> {
    // BufferView resolves retained IDs through the active screen only.
    let view = term.buffer();
    // Consider columns too: two commands may occupy the same physical row.
    let start = view
        .zones()
        .filter(|mark| {
            (mark.line, mark.column) <= (at.line, at.column) && mark.kind == ZoneKind::Output
        })
        .last()?;
    // Stop at the next command boundary; incomplete output must not capture a later command.
    let end = view.zones().find(|mark| {
        (mark.line, mark.column) > (start.line, start.column)
            && matches!(
                mark.kind,
                ZoneKind::Finished | ZoneKind::Output | ZoneKind::Prompt
            )
    })?;
    // Pointer semantic selection cannot extend past D into the next prompt.
    // An intervening prompt/output boundary means this command is unfinished.
    if end.kind != ZoneKind::Finished || (at.line, at.column) > (end.line, end.column) {
        return None;
    }
    // Keep half-open shell boundaries here; callers choose paint or copy conventions.
    Some((
        SelectPoint {
            line: start.line,
            column: start.column,
        },
        SelectPoint {
            line: end.line,
            column: end.column,
        },
    ))
}

/// Returns the adjacent physical cell only within the same soft-wrapped line.
fn word_neighbour(
    term: &Terminal,
    row: usize,
    column: usize,
    right: bool,
) -> Option<(usize, usize)> {
    let ring = &term.screen().ring;
    let width = term.size().0;
    // Inside a physical row, moving by one cell preserves logical continuity.
    if right && column + 1 < width {
        Some((row, column + 1))
    } else if !right && column > 0 {
        Some((row, column - 1))
    } else if right && term.buffer().line(ring.retained(row)?.id)?.continues() {
        // WRAPPED belongs to the predecessor row, including at the right edge.
        Some((row + 1, 0))
    } else if !right && row > 0 && term.buffer().line(ring.retained(row - 1)?.id)?.continues() {
        // Leftward movement consults the preceding row’s continuation bit.
        Some((row - 1, width - 1))
    } else {
        // Hard boundaries never connect two words, even with identical characters.
        None
    }
}
