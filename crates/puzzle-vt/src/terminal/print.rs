//! Character printing, delayed wrap and wide-cell integrity (§6.2, §6.7).
//!
//! xterm's DECAWM leaves the cursor on the final column until another
//! printable arrives. Control moves clear that pending wrap, so printing
//! and cursor movement must not use an eager common increment operation.
//! The parser gives this module Unicode scalars, not bytes. ISO-2022 GL
//! translation therefore applies only when the scalar came from ASCII.
//! A two-column character owns its leading cell and a spacer immediately
//! after it. The spacer never becomes independent ink or copied text.
//! Overwriting either half must erase the partner first; otherwise a row
//! would contain an orphaned wide flag and later layout would desynchronize.
//! A zero-width scalar extends the prior cell's cluster in sparse metadata.
//! It leaves cursor position and pending wrap untouched, matching the
//! terminal's per-code-point width model (D-10) for combining marks.
//! Cluster bytes are capped because hostile combining runs can otherwise
//! consume memory without advancing the cursor at all (§16).
//! REP remembers the last scalar so CSI b can replay it without looking at
//! a cursor position that intervening controls may already have moved.
use unicode_width::UnicodeWidthChar;

/// Constant-space UAX #29 suffix state and property lookup.
pub(super) mod boundary;
/// Grid witness preventing reuse of suffix state after edits or cursor moves.
mod tail;
pub(super) use boundary::Property;
pub(super) use tail::Tail;
/// Scalar property facts and joined suffixes are rebuilt together after edits.
mod cache;
pub(super) use cache::GraphemeCache;

/// Opt-in UAX #29 grapheme joining and width adjustment.
pub(super) mod grapheme;

use super::{Charset, Terminal};
use crate::{
    cell::{Cell, CellFlags},
    row::{CellExtra, RowFlags},
};

impl Terminal {
    /// Draws one decoded scalar and advances or delays the cursor.
    pub(crate) fn print_char(&mut self, input: char) {
        // Rare CSI REP/OSC text reuses the same loop with the current watch mode.
        if self.write_clock.has_watches() {
            self.print_char_watched::<true>(input);
        } else {
            self.print_char_watched::<false>(input);
        }
    }

    /// Slice-selected print loop; ordinary output contains no write-watch branches.
    pub(crate) fn print_char_watched<const WATCH: bool>(&mut self, input: char) {
        #[cfg(test)]
        crate::print_work::note(crate::print_work::Operation::Scalar);
        // GL shifts apply to printable 7-bit bytes, not arbitrary Unicode.
        // A single shift is consumed even when a decoded printable is later
        // dropped at the right margin; it never leaks to another character.
        let charset = self.single_shift.take().unwrap_or(self.gl);
        // Sparse underline and link records belong to the slow path because
        // they must be copied into row extras for every printed cell.
        // Ordinary ASCII with no old wide/extras at the target cell is the
        // high-volume path. One cell store avoids sparse-metadata lookup.
        if self.charsets[charset] == Charset::Ascii
            && input.is_ascii_graphic()
            && !self.insert_mode
            && self.current_link.is_none()
            && self.underline_color.is_none()
            && self.fast_ascii::<WATCH>(input)
        {
            self.last_printed = Some(input);
            return;
        }
        let ch = if input.is_ascii() {
            map_charset(self.charsets[charset], input)
        } else {
            input
        };
        // Mode 2027 lets a following code point extend the preceding
        // grapheme instead of consuming its own wcwidth columns (§6.7).
        // Printable ASCII cannot extend a prior grapheme. The same non-ASCII gate
        // keeps terminal redraws free of heap work (D-31, P1).
        if !ch.is_ascii()
            && self.modes.get(true, 2027) == Some(true)
            && self.extend_grapheme::<WATCH>(ch)
        {
            return;
        }
        // Most terminal output is ASCII. Avoid Unicode width tables there.
        // The parser has already dispatched C0 bytes to `execute`, so every
        // ASCII graphic here occupies exactly one terminal column.
        let width = self.cell_width(ch);
        if width == 0 {
            // Width-zero scalars do not trigger pending autowrap.
            self.join_previous::<WATCH>(ch);
            return;
        }
        let (columns, _) = self.size();
        if !self.fit_before_print(width, columns) {
            return;
        }
        if !self.write_scalar::<WATCH>(ch, width) {
            return;
        }
        self.last_printed = Some(ch);
        self.advance_after_print(width, columns);
    }

    /// Applies ambiguous and clustered flag widths outside the ASCII path.
    // OSC 66 shares this helper, but ordinary printing still needs its tiny
    // mode/width decision in the scalar loop rather than a call per character.
    #[inline(always)]
    pub(super) fn cell_width(&self, ch: char) -> usize {
        // UAX #11 ambiguous characters follow the user's CJK width choice;
        // ordinary ASCII still costs no Unicode table lookup.
        if ch.is_ascii_graphic() {
            1
        } else if self.modes.get(true, 2027) == Some(true)
            && ('\u{1f1e6}'..='\u{1f1ff}').contains(&ch)
        {
            // A regional indicator starts a two-cell flag candidate. If a
            // partner follows, grapheme joining retains the same pair.
            2
        } else if self.ambiguous_width_two {
            UnicodeWidthChar::width_cjk(ch).unwrap_or(1)
        } else {
            UnicodeWidthChar::width(ch).unwrap_or(1)
        }
    }

    /// Consumes delayed wrap and keeps a wide pair on one physical row.
    // Sharing with rectangular placement must not outline the common two-test
    // fit decision; the actual wrap work remains a separate uncommon branch.
    #[inline(always)]
    pub(super) fn fit_before_print(&mut self, width: usize, columns: usize) -> bool {
        // Pending wrap is cursor state, not an occupied extra column.
        if self.cursor.pending_wrap && self.autowrap {
            self.wrap_before_print();
        }
        // xterm drops a wide glyph in the final column with DECAWM off;
        // otherwise it wraps before printing so the pair stays intact.
        if width == 2 && self.cursor.column + 1 >= columns {
            if !self.autowrap {
                return false;
            }
            self.wrap_before_print();
        }
        true
    }

    /// Leaves a final-column write pending until the next printable arrives.
    pub(super) fn advance_after_print(&mut self, width: usize, columns: usize) {
        // A write ending in the final column retains the cursor there.
        // The following printable, rather than this write, performs LF.
        let last = self.cursor.column + width >= columns;
        if last {
            // The next printable may wrap; CR/LF/BS can cancel this state.
            self.cursor.column = columns - 1;
            self.cursor.pending_wrap = self.autowrap;
        } else {
            // A nonfinal write leaves the cursor at the next free cell.
            self.cursor.column += width;
            self.cursor.pending_wrap = false;
        }
    }

    /// Stores a plain ASCII cell when no paired or sparse data needs repair.
    fn fast_ascii<const WATCH: bool>(&mut self, ch: char) -> bool {
        // Pending wrap must pass through the shared slow path, which marks
        // the preceding row WRAPPED before indexing to the next row.
        if self.cursor.pending_wrap {
            return false;
        }
        let row_index = self.cursor.row;
        let column = self.cursor.column;
        let columns = self.primary.ring.columns();
        let mut cell = self.pen;
        cell.ch = ch as u32;
        let (screen, clock) = self.screen_clock_mut();
        let Some(row) = screen.row_mut(row_index) else {
            return false;
        };
        let Some(old) = row.cells.get(column) else {
            return false;
        };
        // Replacing a wide half or sparse cell requires partner/extra repair.
        // The fast store is valid only for a self-contained old cell.
        if old.flags.intersects(
            CellFlags::WIDE | CellFlags::WIDE_SPACER | CellFlags::HAS_EXTRAS | CellFlags::MULTICELL,
        ) {
            return false;
        }
        row.cells.store(column, cell);
        row.version = clock.written::<WATCH>(row, column..column + 1);
        screen.damage.mark(row_index);
        if column + 1 == columns {
            self.cursor.pending_wrap = self.autowrap;
        } else {
            self.cursor.column += 1;
        }
        true
    }

    /// Marks a soft continuation and indexes before the pending character.
    // Row rollover touches metadata and may scroll history; keep it separate
    // from the inlined fit tests paid by every ordinary scalar.
    #[cold]
    #[inline(never)]
    fn wrap_before_print(&mut self) {
        // WRAPPED describes the row that continues, not its successor.
        let row_index = self.cursor.row;
        let (screen, clock) = self.screen_clock_mut();
        if let Some(row) = screen.row_mut(row_index) {
            row.flags.insert(RowFlags::WRAPPED);
            // A continuation changes the line break, not preceding boundary cells.
            row.version = clock.written::<true>(row, row.cells.len()..row.cells.len() + 1);
        }
        // LF may recycle the oldest scrollback row without moving this ID.
        self.line_feed();
        // Implicit wrapping starts at column zero regardless of prior x.
        self.cursor.column = 0;
    }

    /// Repairs the addressed cells and writes a scalar through one row borrow.
    fn write_scalar<const WATCH: bool>(&mut self, ch: char, width: usize) -> bool {
        // Insert once before redirecting through any lower block reservation.
        if self.insert_mode {
            self.shift_chars(width, true);
        }
        // Sparse reservations can redirect a write. Keep the ordinary store
        // inside this loop so a rare retry adds no out-of-line call per scalar.
        // Preparation either refuses, or clears the complete destination span.
        // Thus a successful retry has no rectangular collision and writes once;
        // it cannot repeat preparation at an unchanged obstruction.
        loop {
            // The pen is a prepacked template: hot ASCII printing changes only
            // the scalar and, when necessary, the wide-pair flag (§6.2).
            let row_index = self.cursor.row;
            let column = self.cursor.column;
            // Sparse decorations are copied once at print time; older cells
            // retain their original hyperlink and underline after SGR changes.
            let link_id = self.current_link;
            let underline_color = self.underline_color;
            let mut cell = self.pen;
            cell.ch = ch as u32;
            if width == 2 {
                cell.flags.insert(CellFlags::WIDE);
            }
            let (screen, clock) = self.screen_clock_mut();
            if let Some(row) = screen.row_mut(row_index) {
                // Both target columns may belong to unrelated old wide pairs.
                // Repair them before putting the new pair in either position.
                if !clear_wide_pair::<WATCH>(row, column, clock)
                    || (width == 2 && !clear_wide_pair::<WATCH>(row, column + 1, clock))
                {
                    // Release the row borrow only for a flagged rectangular target.
                    // Preparation redirects or erases it before the same write retries.
                    if !self.prepare_multicell_print(width) {
                        return false;
                    }
                    continue;
                }
                row.put_observed::<WATCH>(column, cell, clock);
                if link_id.is_some() || underline_color.is_some() {
                    row.set_extra_observed::<WATCH>(
                        CellExtra {
                            column,
                            cluster: None,
                            underline_color,
                            link_id,
                        },
                        clock,
                    );
                }
                if width == 2 {
                    // BCE/protection belong to both halves of this one glyph.
                    row.put_observed::<WATCH>(column + 1, wide_spacer(cell), clock);
                }
            }
            // The frame builder can skip untouched rows after this write.
            self.screen_mut().damage.mark(row_index);
            return true;
        }
    }

    /// Extends the previous cell's cluster without consuming another column.
    fn join_previous<const WATCH: bool>(&mut self, mark: char) {
        let row_index = self.cursor.row;
        // At delayed wrap the cursor still names the cell just printed.
        // Otherwise it names the next cell, so look one column backward.
        let column = if self.cursor.pending_wrap {
            self.cursor.column
        } else if self.cursor.column > 0 {
            self.cursor.column - 1
        } else {
            return;
        };
        let Some((column, cell)) = self
            .screen()
            .row(row_index)
            .and_then(|row| cluster_base(row, column))
        else {
            return;
        };
        // Reuse the predecessor lookup already needed by legacy combining text.
        // A slot index must never be interpreted as a Unicode scalar.
        if cell.flags.contains(CellFlags::MULTICELL) {
            self.join_multicell_at(cell.ch, mark);
            return;
        }
        let (screen, clock) = self.screen_clock_mut();
        let Some(row) = screen.row_mut(row_index) else {
            return;
        };
        let Some(extra) = append_mark(row, column, cell, mark) else {
            return;
        };
        let cluster_bytes = extra.cluster.as_ref().map_or(0, String::len);
        // A sentinel tells consumers to read the extra rather than shape
        // the first scalar alone. Other cell style and width bits survive.
        let mut clustered = cell;
        clustered.ch = Cell::CLUSTER;
        row.put_observed::<WATCH>(column, clustered, clock);
        // `put` removed the old entry; reinstall the cluster while retaining
        // any link or underline data that already belonged to this cell.
        row.set_extra_observed::<WATCH>(extra, clock);
        self.note_cluster(cluster_bytes);
        self.screen_mut().damage.mark(row_index);
    }
}

/// Creates the textless second half with matching BCE and selective protection.
#[inline]
fn wide_spacer(lead: Cell) -> Cell {
    let mut spacer = Cell::erased(lead.bg);
    // Layout/protection are the only lead flags meaningful on a spacer; links
    // and cluster extras belong exclusively to the first occupied column.
    spacer.flags = CellFlags::WIDE_SPACER | (lead.flags & CellFlags::PROTECTED);
    spacer
}

/// Borrows bounded legacy text before moving its reusable cluster allocation.
/// Marks beyond the security cap leave the existing cell and its extras intact.
fn append_mark(row: &mut crate::Row, column: usize, cell: Cell, mark: char) -> Option<CellExtra> {
    // Width-zero fallback also handles legacy-mode clusters. Borrow its
    // text first; taking or cloning the old extra would either lose it or
    // allocate for every mark, including marks discarded by the byte cap.
    let old = row.extras.iter().find(|item| item.column == column);
    let mut scalar = [0_u8; 4];
    let previous = old
        .and_then(|extra| extra.cluster.as_deref())
        .unwrap_or_else(|| {
            char::from_u32(cell.ch)
                .unwrap_or('\u{fffd}')
                .encode_utf8(&mut scalar)
        });
    let length = previous.len();
    if length + mark.len_utf8() > 32 {
        return None;
    }
    let mut bytes = [0_u8; 32];
    bytes[..length].copy_from_slice(previous.as_bytes());
    let end = length + mark.encode_utf8(&mut bytes[length..]).len();
    let Ok(joined) = std::str::from_utf8(&bytes[..end]) else {
        return None;
    };
    // Decoration fields are scalar metadata; cluster ownership moves to
    // the new extra only after its entire bounded text is ready.
    let underline_color = old.and_then(|extra| extra.underline_color);
    let link_id = old.and_then(|extra| extra.link_id);
    let mut cluster = row.cluster_storage(column);
    cluster.push_str(joined);
    Some(CellExtra {
        column,
        cluster: Some(cluster),
        underline_color,
        link_id,
    })
}

/// Resolves a combining mark's owning cell, stepping past a wide spacer.
fn cluster_base(row: &crate::Row, mut column: usize) -> Option<(usize, Cell)> {
    // The caller has already selected the preceding occupied column.
    let mut cell = row.cells.get(column).copied()?;
    // A wide glyph's last occupied column is only a spacer. The leading
    // cell owns its scalar and any existing cluster metadata.
    if cell.flags.contains(CellFlags::WIDE_SPACER) && column > 0 {
        column -= 1;
        cell = row.cells.get(column).copied()?;
    }
    // An unwritten blank or orphaned spacer cannot start a cluster.
    // Slot zero is a valid rectangular owner; only ordinary zero means unwritten.
    // Keeping this distinction here shares the existing predecessor classification.
    if (cell.ch == 0 && !cell.flags.contains(CellFlags::MULTICELL))
        || cell.flags.contains(CellFlags::WIDE_SPACER)
    {
        None
    } else {
        Some((column, cell))
    }
}

/// Blanks the other half before either half of a wide glyph is replaced.
///
/// This is the repair rule in design §6.2. The blank keeps the previous
/// background so the overwritten pair does not leave a visible color hole.
fn clear_wide_pair<const WATCH: bool>(
    row: &mut crate::Row,
    column: usize,
    clock: &mut crate::WriteClock,
) -> bool {
    let Some(cell) = row.cells.get(column).copied() else {
        return true;
    };
    // Fold the rectangular case into the existing special-layout mask. The
    // common ordinary cell still takes only the pre-existing wide tests.
    if cell
        .flags
        .intersects(CellFlags::WIDE | CellFlags::MULTICELL)
    {
        if cell.flags.contains(CellFlags::MULTICELL) {
            // Its owner can span seven rows; repair after releasing this borrow.
            return false;
        }
        // Replacing the leading half invalidates the trailing placeholder.
        row.put_observed::<WATCH>(column + 1, Cell::erased(cell.bg), clock);
    } else if cell.flags.contains(CellFlags::WIDE_SPACER) && column > 0 {
        // Replacing the spacer invalidates the old leading glyph.
        row.put_observed::<WATCH>(column - 1, Cell::erased(cell.bg), clock);
    }
    true
}

/// Translates designated GL characters using xterm ctlseqs' DEC table.
fn map_charset(charset: Charset, ch: char) -> char {
    // xterm ctlseqs lists DEC graphics contiguously from ASCII ` through ~.
    // A literal mapping string keeps unsupported characters unchanged and
    // avoids a table of thirty-one branch arms in the regular print path.
    const DEC_SPECIAL: &str = "◆▒␉␌␍␊°±␤␋┘┐┌└┼⎺⎻─⎼⎽├┤┴┬│≤≥π≠£·";
    match charset {
        Charset::Ascii => ch,
        Charset::Uk if ch == '#' => '£',
        Charset::Uk => ch,
        Charset::DecSpecial if ('`'..='~').contains(&ch) => DEC_SPECIAL
            .chars()
            .nth(ch as usize - '`' as usize)
            .unwrap_or(ch),
        Charset::DecSpecial => ch,
    }
}

/// Applies the existing UAX suffix state to a borrowed, already bounded cluster.
pub(super) fn text_joins(text: &str, next: char) -> bool {
    // This is used only after a sparse block has already been resolved.
    // Ordinary scalar printing retains its cached constant-space state path.
    boundary::State::from_text(text).is_some_and(|state| state.joins(boundary::Property::of(next)))
}
