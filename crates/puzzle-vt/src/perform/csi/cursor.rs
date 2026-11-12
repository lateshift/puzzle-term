//! Cursor addressing and margin setting from xterm ctlseqs (§6.5).
//!
//! Most coordinates in CSI are one-based and default to one. `move_to`
//! converts them into bounded grid coordinates, while `cup` additionally
//! applies DECOM's origin. The mode-69 switch disambiguates CSI s: SCOSC
//! without horizontal margins, DECSLRM when DECLRMM is enabled.
//! The same final byte has several historic aliases: H/F are CUP/HVP,
//! G/backtick are CHA/HPA, and e is VPR. Routing those aliases to one
//! movement method avoids subtly different clamp rules. CHT/CBT consult
//! the tab-stop vector, because HTS and TBC can change it at runtime.
//! DECSTBM's omitted bottom parameter means the last visible row, whereas
//! an explicit bottom at or above the top is invalid and ignored.

use crate::terminal::Terminal;
use vte::Params;

use super::{count, raw};

/// Executes one cursor or tab CSI final after vte's syntax recognition.
pub(super) fn dispatch(term: &mut Terminal, params: &Params, action: char) {
    let n = count(params, 0);
    // Read screen dimensions once; every later absolute coordinate is
    // bounded to that visible grid before it reaches a cell index.
    let (width, height) = term.size();
    match action {
        // Movement aliases differ in spelling but share xterm clamp rules.
        'A' => term.move_relative(-(n as i32), 0),
        'B' | 'e' => term.move_relative(n as i32, 0),
        'C' | 'a' => term.move_relative(0, n as i32),
        'D' => term.move_relative(0, -(n as i32)),
        'E' => {
            // CNL/CPL also return to the active left margin, unlike CUD/CUU.
            term.move_relative(n as i32, 0);
            term.carriage_return();
        }
        'F' => {
            term.move_relative(-(n as i32), 0);
            term.carriage_return();
        }
        'G' | '`' => term.move_to(term.cursor.row, n.saturating_sub(1)),
        'd' => term.move_to(n.saturating_sub(1), term.cursor.column),
        'H' | 'f' => term.cup(n, count(params, 1)),
        'I' => term.tab_many(n, true),
        'Z' => term.tab_many(n, false),
        'g' => match raw(params, 0) {
            // TBC 0 clears only the current stop; 3 removes all stops.
            0 => term.tabs[term.cursor.column] = false,
            3 => term.tabs.fill(false),
            _ => {}
        },
        'r' => {
            // DECSTBM values are one-based, inclusive, and require at
            // least two rows so the region can actually scroll.
            let bottom = if raw(params, 1) == 0 {
                height
            } else {
                count(params, 1)
            };
            if n < bottom && bottom <= height {
                term.margins.top = n - 1;
                term.margins.bottom = bottom - 1;
                // xterm homes the cursor after setting vertical margins.
                term.cup(1, 1);
            }
        }
        's' if term.modes.get(true, 69) == Some(true) => {
            // DECSLRM shares CSI s with SCOSC. Mode 69 is the only legal
            // disambiguator; no private prefix is present on this CSI.
            let right = if raw(params, 1) == 0 {
                width
            } else {
                count(params, 1)
            };
            if n < right && right <= width {
                term.margins.left = n - 1;
                term.margins.right = right - 1;
                // A new horizontal origin changes where CUP(1,1) lands.
                term.cup(1, 1);
            }
        }
        's' => term.save_cursor(),
        // SCORC restores the same saved cursor slot as ESC 8.
        'u' => term.restore_cursor(),
        _ => {}
    }
}
