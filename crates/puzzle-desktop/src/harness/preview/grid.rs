//! Compact visible VT text for scenario assertions and review dumps (§13.1).
//!
//! Rendering now belongs to `FrameBuilder`; this module only names the text
//! a person sees in a grid row. Wide spacer cells are skipped, while
//! multi-scalar clusters borrow their complete string from sparse extras.
//! The dump path may allocate because it runs only for scripted evidence,
//! never in the production frame or parser hot path (P1).
use puzzle_term::vt::{Cell, CellFlags, Row, Terminal};

/// Extracts the text a user would copy from one visible row.
pub(super) fn row_text(term: &Terminal, index: usize) -> Result<String, String> {
    let row = term.screen().row(index).ok_or("row outside visible grid")?;
    let mut text = String::new();
    for (column, cell) in row.cells.iter().enumerate() {
        // A wide placeholder stores the second column of the prior glyph;
        // including it here would duplicate user-visible text.
        if cell.flags.contains(CellFlags::WIDE_SPACER) {
            continue;
        }
        if cell.flags.contains(CellFlags::MULTICELL) {
            // A slot is not a scalar. Only the first retained fragment emits its payload.
            if let Some(block) = term.screen().ring.multicell(cell.ch)
                && block.column == column
                && block.fragments.first().is_some_and(|f| f.line == row.id)
            {
                text.push_str(&block.text);
            }
            continue;
        }
        append_cell_text(&mut text, row, column, cell);
    }
    // Assertions compare meaningful row prefixes. Unspecified right-side
    // padding is removed so a fixture need not name every blank column.
    Ok(text.trim_end_matches(' ').to_owned())
}

/// Resolves one inline scalar or sparse grapheme without altering the grid.
fn append_cell_text(text: &mut String, row: &Row, column: usize, cell: &Cell) {
    if cell.ch == Cell::CLUSTER {
        // A cluster sentinel has no Unicode value itself. The actual
        // grapheme is in the extra for this same physical column.
        text.push_str(
            row.extras
                .iter()
                .find(|extra| extra.column == column)
                .and_then(|extra| extra.cluster.as_deref())
                .unwrap_or(" "),
        );
        return;
    }
    if cell.ch == 0 {
        text.push(' ');
        return;
    }
    // Invalid scalar storage is visible as replacement ink rather than
    // causing a preview or grid assertion to panic on PTY input (R8).
    text.push(char::from_u32(cell.ch).unwrap_or('\u{fffd}'));
}
