//! ISO-2022 graphic-set controls documented in xterm ctlseqs.
//!
//! Designation changes a G-set; invocation chooses which set translates
//! printable bytes. SS2 and SS3 change only one subsequent printable.
//! xterm ctlseqs assigns ESC ( to G0, ) to G1, * to G2 and + to G3.
//! An SCS final chooses ASCII, DEC Special Graphics or UK replacement.
//! Unknown finals leave the existing designation intact rather than
//! inventing a mapping for a character set Puzzle does not advertise.
//! ESC controls without an intermediate affect cursor, scroll or reset
//! state. DECALN is the one supported `#` intermediate: it writes literal
//! E cells to expose physical grid alignment, then marks a full redraw.
//! The parser owns ST termination, so ESC backslash never arrives here.
use crate::terminal::Terminal;

/// Interprets the ESC final and its optional SCS intermediate.
pub(super) fn dispatch(term: &mut Terminal, intermediates: &[u8], byte: u8) {
    if intermediates == b"#" && byte == b'8' {
        alignment_test(term);
        return;
    }
    if let [prefix] = intermediates {
        // The intermediate selects which designation is changed; it does
        // not itself invoke that set into the printable GL range.
        let set = match prefix {
            b'(' => Some(0),
            b')' => Some(1),
            b'*' => Some(2),
            b'+' => Some(3),
            _ => None,
        };
        if let Some(set) = set {
            // Unknown designation finals leave the old set unchanged.
            term.designate(set, byte);
        }
        // An SCS sequence has no further locking-shift interpretation.
        return;
    }
    if !intermediates.is_empty() {
        // Future ISO-2022 intermediates are consumed safely rather than
        // interpreted as unrelated single-byte ESC controls.
        return;
    }
    match byte {
        // xterm ctlseqs: DECSC/DECRC include rendition and origin mode.
        b'7' => term.save_cursor(),
        b'8' => term.restore_cursor(),
        b'D' => term.line_feed(),
        // NEL is IND plus CR; LF alone respects LNM instead.
        b'E' => {
            term.line_feed();
            term.carriage_return();
        }
        b'M' => {
            // RI at the top margin inserts a blank line into the region;
            // elsewhere it moves only the cursor upward.
            if term.cursor.row == term.margins.top {
                term.scroll_region_down(1);
            } else {
                term.move_relative(-1, 0);
            }
        }
        b'H' => term.tabs[term.cursor.column] = true,
        b'c' => term.hard_reset(),
        b'=' => term.modes.set(true, 66, true),
        b'>' => term.modes.set(true, 66, false),
        // The xterm ctlseqs SS2/SS3 forms affect one graphic character.
        b'N' => term.single_shift(2),
        b'O' => term.single_shift(3),
        // LS2/LS3 stay active until another locking shift or SI/SO.
        b'n' => term.locking_shift(2),
        b'o' => term.locking_shift(3),
        _ => {}
    }
}

/// Fills literal alignment ink after retiring any rectangular cell ownership.
fn alignment_test(term: &mut Terminal) {
    term.source_cleared();
    // DECALN uses literal E in every cell to expose alignment faults.
    // It is diagnostic, so do not translate E through the active G-set.
    // Rectangles must be retired before literal cells replace their witnesses.
    // Ordinary alignment tests retain their existing one-pass fill.
    if term.screen().ring.multicells.is_some() {
        term.erase_display(2, false);
    }
    let (columns, rows) = term.size();
    for row_index in 0..rows {
        for column in 0..columns {
            let mut cell = term.pen;
            cell.ch = u32::from(b'E');
            let (screen, clock) = term.screen_clock_mut();
            if let Some(row) = screen.row_mut(row_index) {
                row.put(column, cell, clock);
            }
        }
    }
    term.screen_mut().damage.full = true;
}
