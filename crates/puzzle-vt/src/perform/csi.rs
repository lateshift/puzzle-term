//! CSI family router for xterm cursor, editing and mode controls (§6.5).
//!
//! `vte` has already recognized the final byte and bounded its parameter
//! storage. Each family interprets defaults separately: most counts use
//! one when omitted or zero, while erase kinds use zero as a real value.
//! Unknown sequences have no effect, even when their parameters are huge.
//! The prefix and intermediate bytes matter as much as the final: CSI q
//! with a quote is DECSCA, while CSI q with a space is cursor style.

/// Cursor placement and tab movement in zero-based grid coordinates.
mod cursor;
/// Erase and row editing, including selective DEC forms.
mod edit;
/// ANSI/DEC mode changes and DECRQM state reports.
mod modes;
/// CSI identity, status and window-size report bytes.
mod replies;
/// Xterm Shift mouse ownership negotiation.
mod shift;

use crate::terminal::Terminal;
use vte::Params;

/// Routes one recognized CSI final to the relevant semantic family.
pub(super) fn dispatch<const WATCH: bool>(
    term: &mut Terminal,
    params: &Params,
    intermediates: &[u8],
    action: char,
) {
    match (intermediates, action) {
        // Kitty's prefix distinguishes mode negotiation from CSI u cursor
        // restore; query replies stay on the same ordered reply queue.
        (b">" | b"<" | b"?" | b"=", 'u') => {
            term.kitty_keys
                .control(term.active, params, intermediates[0], &mut term.replies);
        }
        // DEC modes use a private `?` prefix; treating them as ANSI modes
        // would produce the wrong state and DECRQM reply.
        (b"?", 'h' | 'l') | (b"", 'h' | 'l') | (b"?$", 'p') | (b"$", 'p') => {
            modes::dispatch(term, params, intermediates, action)
        }
        (b">", 's') => shift::dispatch(term, params),
        (b"", 'm') => super::sgr::dispatch(term, params),
        // Query replies share final bytes with some ordinary controls, so
        // their prefixes are matched before movement and edit families.
        (b"", 'c' | 'n' | 't') | (b">", 'c' | 'q') | (b"=", 'c') | (b"?", 'n') => {
            replies::dispatch(term, params, intermediates, action)
        }
        (
            b"",
            'A' | 'B' | 'C' | 'D' | 'E' | 'F' | 'G' | 'H' | 'I' | 'Z' | 'a' | 'd' | 'e' | 'f' | '`'
            | 's' | 'u' | 'g' | 'r',
        ) => cursor::dispatch(term, params, action),
        (b"", 'J' | 'K' | 'X' | '@' | 'P' | 'L' | 'M' | 'S' | 'T' | 'b')
        | (b"?", 'J' | 'K')
        | (b"\"", 'q')
        | (b" ", 'q')
        | (b"!", 'p') => edit::dispatch::<WATCH>(term, params, intermediates, action),
        _ => {}
    }
}

/// Reads a CSI parameter, retaining ECMA-48's default-zero distinction.
pub(super) fn raw(params: &Params, index: usize) -> u16 {
    params
        .iter()
        .nth(index)
        .and_then(|part| part.first())
        .copied()
        .unwrap_or(0)
}

/// Converts omitted or zero counts to one as xterm movement controls do.
pub(super) fn count(params: &Params, index: usize) -> usize {
    usize::from(raw(params, index).max(1))
}
