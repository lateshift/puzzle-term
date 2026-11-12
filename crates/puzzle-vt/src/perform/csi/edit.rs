//! CSI erasure, editing and bounded REP from xterm ctlseqs (§6.5).
//!
//! ED/EL use current background-color erase, including selective forms.
//! REP is bounded by the grid size (§16), so an attacker cannot turn one
//! sequence with an enormous count into unbounded CPU work.
//! The parser forwards only recognized syntax. This handler decides which
//! CSI parameters are counts and which are selectors; treating ED 0 as
//! count one would silently erase the wrong region. DECSED and DECSEL are
//! distinguished by their private `?` intermediate, and DECSCA by the
//! quote intermediate before its final `q`.
//! REP inherits the feed-selected watch instantiation, so repetition does not
//! add a runtime watch-mode choice to each emitted scalar.

use crate::{cell::CellFlags, terminal::Terminal};
use vte::Params;

use super::{count, raw};

/// Executes one editing final after CSI syntax has been recognized.
pub(super) fn dispatch<const WATCH: bool>(
    term: &mut Terminal,
    params: &Params,
    intermediates: &[u8],
    action: char,
) {
    if intermediates == b"!" && action == 'p' {
        // DECSTR restores modes without clearing the text already visible.
        term.soft_reset();
        return;
    }
    if intermediates == b"\"" && action == 'q' {
        // DECSCA 1 protects new cells; 0 and 2 make them erasable.
        // Protection is stored in the pen and copied into future cells.
        if raw(params, 0) == 1 {
            term.pen.flags.insert(CellFlags::PROTECTED);
        } else {
            term.pen.flags.remove(CellFlags::PROTECTED);
        }
        return;
    }
    if intermediates == b" " && action == 'q' {
        // DECSCUSR is appearance state; six is the last xterm style.
        // Unknown style numbers preserve the current cursor presentation.
        let style = raw(params, 0);
        if style <= 6 {
            term.cursor_style = style as u8;
        }
        return;
    }
    let selective = intermediates == b"?";
    // A single absent movement count means one, but ED and EL read their
    // raw zero selector below instead of this normalized value.
    let n = count(params, 0);
    match action {
        // ED/EL share the selective implementation; BCE uses the pen bg.
        'J' => term.erase_display(raw(params, 0), selective),
        'K' => term.erase_line(raw(params, 0), selective),
        'X' => term.erase_span(
            term.cursor.row,
            term.cursor.column,
            term.cursor.column.saturating_add(n),
            false,
        ),
        '@' => term.shift_chars(n, true),
        'P' => term.shift_chars(n, false),
        'L' | 'M' if (term.margins.top..=term.margins.bottom).contains(&term.cursor.row) => {
            // IL/DL scroll from the cursor to the region bottom, leaving
            // rows above it fixed even within the original DEC margins.
            let old_top = term.margins.top;
            term.margins.top = term.cursor.row;
            if action == 'L' {
                term.scroll_region_down(n);
            } else {
                term.scroll_region_up(n);
            }
            term.margins.top = old_top;
        }
        // SU/SD use the full active DEC region, independent of cursor row.
        'S' => term.scroll_region_up(n),
        'T' => term.scroll_region_down(n),
        'b' => {
            // A grid-sized cap prevents pathological REP work even though
            // vte has already bounded its numeric parameter to u16.
            let limit = term.size().0.saturating_mul(term.size().1);
            if let Some(ch) = term.last_printed {
                for _ in 0..n.min(limit) {
                    term.print_char_watched::<WATCH>(ch);
                }
            }
        }
        _ => {}
    }
}
