//! Text, VT and HTML exports of retained terminal contents (§6.13).
//!
//! VT export uses ordinary xterm CSI/SGR/OSC bytes so feeding it through
//! `Terminal::advance` reconstructs the grid rather than relying on an
//! internal memory image. Styles are emitted before each written cell;
//! this is intentionally not a compressed stream because serialization
//! is a cold API and correctness matters more than escape count.
//! HTML escapes both text and attribute data; it never carries a live URL
//! without escaping because terminal output is untrusted (§16).
//!
//! The VT form is meant for reinjection through the same public byte
//! parser as application output. That lets a saved screen be restored by
//! a browser or harness without reaching into private row memory. An
//! absolute CUP precedes each meaningful cell; rows are not inferred from
//! the prior glyph's width, which matters for wide text and sparse erases.
//! The fixed-cell model distinguishes an unwritten blank (`ch == 0`)
//! from a printed space. BCE blanks with nondefault backgrounds use ECH,
//! preserving that distinction when possible. A wide spacer is emitted
//! only by replaying its lead glyph, never as an independent character.
//!
//! SGR is reset before every printed cell. The longer stream avoids a
//! style-state dependency between distant cells and makes exact cell
//! comparisons simpler than a minimal-diff encoder. Serialized text is
//! cold-path data, outside P1's per-byte parser and per-frame loops.
//!
//! The HTML form is presentation data for an embedding page. It escapes
//! untrusted content before interpolation and leaves palette/default
//! colors through the current VT palette and defaults installed by Session.
//! Bare VT buffers without a host theme retain destination CSS system colors.
/// Shared resolved HTML runs prevent buffer and clipboard rendition drift.
mod html;
/// Rectangular persistence uses ordinary wire operations with explicit positioning.
mod multicell;
/// Clipboard snippets reuse this module’s rendition and color conversion.
mod selection;
/// HTML behavioral tests exercise whole-buffer and selected-cell exports.
#[cfg(test)]
mod tests;
use super::*;
pub use selection::CellRange;

/// External representation of a retained line range.
///
/// These are explicit variants so a caller cannot confuse an ANSI byte
/// stream with safe HTML or plain copy text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// Newline-separated text with soft wraps joined.
    ///
    /// It follows the same extraction rules as copy (§6.9).
    PlainText,
    /// Xterm escape bytes that reproduce styles, cells and cursor.
    ///
    /// Replay goes through `Terminal::advance`, not a private loader.
    Vt,
    /// Inline styled spans suitable for a host's sanitized preview.
    ///
    /// Application text is escaped before becoming markup.
    Html,
}

impl BufferView<'_> {
    /// Serializes a line range in one of the three §6.13 formats.
    ///
    /// The range is inclusive in stable line IDs. Missing/evicted IDs
    /// produce no row rather than being reinterpreted as grid indices.
    #[must_use]
    pub fn serialize(self, range: RangeInclusive<LineId>, format: Format) -> String {
        match format {
            Format::PlainText => self.text(range, TextOptions::default()),
            Format::Vt => vt(self, range),
            Format::Html => html(self, range),
        }
    }
}

/// Recreates the selected visible cells using normal CSI cursor and SGR.
///
/// This is deliberately straightforward: the parser's own semantics are
/// the authority for cursor movement, BCE erase and style application.
fn vt(view: BufferView<'_>, range: RangeInclusive<LineId>) -> String {
    // Start from a known blank grid so a replay into a reused terminal
    // cannot leave stale cells in columns omitted from the saved range.
    let mut output = String::from("\x1bc\x1b[3J\x1b[2J\x1b[H");
    // Alternate-screen content must be selected before cell writes; a
    // later 1049h would clear everything we just reconstructed.
    if view.active() == ActiveScreen::Alternate {
        output.push_str("\x1b[?1049h");
    }
    if view
        .term
        .screen()
        .ring
        .multicells
        .as_ref()
        .is_some_and(|store| store.iter().next().is_some())
    {
        return multicell::vt(view, range, output);
    }
    let mut previous_wrapped = false;
    for (row_number, line) in view.lines(range).enumerate() {
        let visible_row = row_number.min(view.size().1.saturating_sub(1));
        if row_number >= view.size().1 && !previous_wrapped {
            // A line feed on the bottom row advances retained history
            // through the parser's ordinary full-width scroll path.
            // New rows must begin on default paper. The preceding exported
            // cell may carry colored SGR; BCE must not tint skipped blank cells.
            output.push_str(&format!("\x1b[0m\x1b[{};1H\n", view.size().1));
        }
        write_row(&mut output, visible_row, line);
        previous_wrapped = line.row.flags.contains(RowFlags::WRAPPED);
        if previous_wrapped {
            // CUP cannot encode a soft-wrap bit. Force the same xterm
            // delayed-wrap transition, then restore its two temporary
            // cells before the next retained row is replayed.
            write_wrap(&mut output, visible_row, view.size(), line);
        }
    }
    // Reapply runtime modes after drawing so DECOM and reverse video do
    // not change the absolute placement used above.
    write_modes(view, &mut output);
    write_cursor(view, &mut output);
    output
}

/// Recreates xterm's soft-wrap transition without changing stored ink.
fn write_wrap(output: &mut String, row: usize, size: (usize, usize), line: LineView<'_>) {
    let (columns, rows) = size;
    output.push_str(&format!("\x1b[{};{}H\x1b[0mX", row + 1, columns));
    // The second printable consumes pending wrap. At the bottom margin it
    // also scrolls exactly once, replacing the next iteration's LF.
    output.push('X');
    // The wrap glyph advanced the cursor to column one; erase the
    // temporary cell at column zero, not its right-hand neighbour.
    let next_row = (row + 1).min(rows - 1);
    output.push_str(&format!("\x1b[{};1H\x1b[0m\x1b[X", next_row + 1));
    let shifted_row = if row + 1 == rows {
        row.saturating_sub(1)
    } else {
        row
    };
    let lead = if line.cells()[columns - 1]
        .flags
        .contains(CellFlags::WIDE_SPACER)
    {
        columns - 2
    } else {
        columns - 1
    };
    write_cell(output, shifted_row, lead, line);
}

/// Writes one physical row with explicit cursor and graphic state.
fn write_row(output: &mut String, row_number: usize, line: LineView<'_>) {
    for (column, cell) in line.cells().iter().enumerate() {
        // The lead glyph writes both columns. Emitting the spacer
        // separately would erase or corrupt that new wide pair.
        if cell.flags.contains(CellFlags::WIDE_SPACER) {
            continue;
        }
        // An untouched default blank is already present after ED 2;
        // BCE blanks with color still need an explicit erase.
        if cell.ch == 0 && cell.bg == PackedColor::DEFAULT {
            continue;
        }
        write_cell(output, row_number, column, line);
    }
}

/// Reprints one stored lead cell through the normal parser path.
///
/// This helper is also used to restore a pending-wrap cursor: CUP alone
/// would clear that state even if it placed the cursor in the same column.
fn write_cell(output: &mut String, row_number: usize, column: usize, line: LineView<'_>) {
    let Some(cell) = line.cells().get(column) else {
        return;
    };
    output.push_str(&format!("\x1b[{};{}H", row_number + 1, column + 1));
    output.push_str(&sgr(
        *cell,
        line.row
            .extras
            .iter()
            .find(|extra| extra.column == column)
            .and_then(|extra| extra.underline_color),
    ));
    // DECSCA is a print-time flag: selective erase later reads it
    // from the reconstructed cell, not from current pen state.
    if cell.flags.contains(CellFlags::PROTECTED) {
        output.push_str("\x1b[1\"q");
    } else {
        output.push_str("\x1b[0\"q");
    }
    // ECH leaves `ch == 0` with the current background color,
    // unlike printing a literal space with code point U+0020.
    if cell.ch == 0 {
        output.push_str("\x1b[X");
    } else if let Some(view_cell) = line.cell(column) {
        // A stored cluster may have been written under mode 2027 even
        // though the mode changed before export. Its recorded span is
        // authoritative: select the width model before replaying it.
        if matches!(view_cell.text, CellText::Cluster(_)) {
            output.push_str(if cell.flags.contains(CellFlags::WIDE) {
                "\x1b[?2027h"
            } else {
                "\x1b[?2027l"
            });
        }
        // OSC 8 belongs to the cell's semantic style, not SGR. Opening
        // and closing around one glyph also avoids leaking a link to an
        // unrelated sparse cell later in the stream.
        if let Some(uri) = view_cell.link {
            output.push_str(&format!("\x1b]8;;{uri}\x1b\\"));
        }
        push_text(output, view_cell.text);
        if view_cell.link.is_some() {
            output.push_str("\x1b]8;;\x1b\\");
        }
    }
}

/// Restores the current row and column after mode side effects.
fn write_cursor(view: BufferView<'_>, output: &mut String) {
    output.push_str(&format!("\x1b[{} q", view.term.cursor_style));
    if let Some(cursor) = view.cursor() {
        // CUP is last: mode changes such as DECOM can home the cursor.
        let row = view
            .term
            .screen()
            .ring
            .iter()
            .skip(view.term.screen().ring.scrollback_len())
            .position(|line| line.id == cursor.line)
            .unwrap_or(0);
        output.push_str(&format!("\x1b[{};{}H", row + 1, cursor.column + 1));
        if cursor.pending_wrap
            && let Some(line) = view.line(cursor.line)
        {
            let lead = if line
                .cells()
                .get(cursor.column)
                .is_some_and(|cell| cell.flags.contains(CellFlags::WIDE_SPACER))
            {
                cursor.column.saturating_sub(1)
            } else {
                cursor.column
            };
            // Printing the final glyph again is what recreates delayed
            // wrap; xterm has no cursor-position sequence for this flag.
            write_cell(output, row, lead, line);
        }
    }
}

/// Restores every supported input/visible mode after replaying content.
///
/// The fixed catalogue is shared with DECRQM; unsupported numbers never
/// become a mode merely because a serialized screen mentioned them.
fn write_modes(view: BufferView<'_>, output: &mut String) {
    for (private, number) in [(false, 4), (false, 20)].into_iter().chain(
        crate::modes::DEC_MODES
            .iter()
            .map(|(number, _)| (true, *number)),
    ) {
        let Some(enabled) = view.term.modes.get(private, number) else {
            continue;
        };
        // Switching screens was handled before content; doing so again
        // would clear an alternate grid or expose the wrong primary one.
        if private && matches!(number, 47 | 1047 | 1048 | 1049) {
            continue;
        }
        let prefix = if private { "?" } else { "" };
        output.push_str(&format!(
            "\x1b[{prefix}{number}{}",
            if enabled { 'h' } else { 'l' }
        ));
    }
}

/// Encodes one cell's independent graphic rendition from default state.
///
/// A reset per cell prevents old underline or truecolor attributes from
/// leaking into a new cell when the original row changed styles midline.
fn sgr(cell: Cell, underline_color: Option<PackedColor>) -> String {
    let mut codes = String::from("0");
    for (flag, code) in [
        (Attrs::BOLD, 1),
        (Attrs::DIM, 2),
        (Attrs::ITALIC, 3),
        (Attrs::BLINK, 5),
        (Attrs::INVERSE, 7),
        (Attrs::HIDDEN, 8),
        (Attrs::STRIKE, 9),
        (Attrs::OVERLINE, 53),
    ] {
        if cell.attrs.contains(flag) {
            codes.push_str(&format!(";{code}"));
        }
    }
    // Colon subparameters preserve the six underline styles; a plain
    // SGR 4 would collapse curly/dotted/dashed to a single straight line.
    if cell.attrs.underline_style() != 0 {
        codes.push_str(&format!(";4:{}", cell.attrs.underline_style()));
    }
    color_code(&mut codes, cell.fg, 38);
    color_code(&mut codes, cell.bg, 48);
    if let Some(color) = underline_color {
        color_code(&mut codes, color, 58);
    }
    format!("\x1b[{codes}m")
}

/// Preserves palette tags so future theme changes recolor replayed text.
///
/// An RGB cell uses three components; an indexed cell keeps its palette
/// index instead of freezing the current theme's resolved RGB value.
fn color_code(codes: &mut String, color: PackedColor, selector: u8) {
    match color.0 >> 24 {
        1 => codes.push_str(&format!(";{selector};5;{}", color.0 & 255)),
        2 => codes.push_str(&format!(
            ";{selector};2;{};{};{}",
            (color.0 >> 16) & 255,
            (color.0 >> 8) & 255,
            color.0 & 255
        )),
        _ => {}
    }
}

/// Prints a cluster once; a wide spacer is handled by its lead cell.
///
/// The cluster text is the original Unicode sequence, not its rendered
/// glyph or width, so the parser applies the same width model on replay.
fn push_text(output: &mut String, text: CellText<'_>) {
    match text {
        CellText::Empty => {}
        CellText::Scalar(ch) => output.push(ch),
        CellText::Cluster(cluster) => output.push_str(cluster),
    }
}

/// Serializes retained rows through the same resolved runs as HTML copy.
fn html(view: BufferView<'_>, range: RangeInclusive<LineId>) -> String {
    if view.term.screen().ring.multicells.is_some() {
        return multicell::selection::rich(
            view,
            CellRange {
                start: (*range.start(), 0),
                end: (*range.end(), view.size().0),
                block: false,
                trim_trailing: false,
                final_newline: false,
            },
            Format::Html,
        );
    }
    let mut output = String::from("<pre>");
    let mut runs = html::Runs::default();
    let mut previous_wrapped = true;
    for line in view.lines(range) {
        // A predecessor's wrap flag decides whether its text continues here.
        if !previous_wrapped {
            runs.finish(&mut output);
            output.push('\n');
        }
        for column in 0..line.cells().len() {
            if let Some(cell) = line.cell(column) {
                runs.cell(&mut output, cell, view.term);
            }
        }
        previous_wrapped = line.continues();
    }
    runs.finish(&mut output);
    output.push_str("</pre>");
    output
}
