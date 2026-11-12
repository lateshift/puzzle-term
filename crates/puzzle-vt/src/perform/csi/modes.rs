//! DECSET, DECRST and DECRQM semantics from xterm ctlseqs (§6.5).
//!
//! Mode reports use the state table rather than inferring state from a
//! rendering side effect. A query for an unknown mode answers zero, letting
//! programs distinguish unsupported extensions from supported reset modes.
//! `Modes` carries report state, while a few hot-path booleans mirror it:
//! DECAWM, LNM and IRM are read during printing or C0 line feeds. Updates
//! happen together in this one handler; queries always use the table.
//! The alternate-screen variants differ: 47 selects the buffer, 1047
//! selects and clears it, 1048 only saves/restores cursor state, and 1049
//! combines save, switch, clear and restore. The primary grid is never
//! copied, so returning from a full-screen program reveals its old text.

use crate::terminal::Terminal;
use vte::Params;

/// Applies a mode list or answers each DECRQM query in input order.
pub(super) fn dispatch(term: &mut Terminal, params: &Params, intermediates: &[u8], action: char) {
    let private = intermediates.first() == Some(&b'?');
    // vte keeps each `;`-separated parameter in a bounded small structure.
    // A list affects modes in wire order; duplicate entries are harmless.
    for part in params.iter() {
        let number = part.first().copied().unwrap_or(0);
        if action == 'p' {
            // XTerm DECRQM uses 1 set, 2 reset and 0 unrecognized. A reset
            // supported mode must never be reported as unknown.
            let state = match term.modes.get(private, number) {
                Some(true) => 1,
                Some(false) => 2,
                None => 0,
            };
            let prefix = if private { "?" } else { "" };
            // DECRQM is a query: reply bytes are buffered until feed returns.
            // The Session will write them to the transport in parser order.
            term.replies
                .extend_from_slice(format!("\x1b[{prefix}{number};{state}$y").as_bytes());
        } else {
            set_mode(term, private, number, action == 'h');
        }
    }
}

/// Performs side effects only for modes Puzzle advertises as supported.
fn set_mode(term: &mut Terminal, private: bool, number: u16, enabled: bool) {
    // Unsupported numbers have no side effects even if a future xterm
    // version gives them meaning. The catalogue controls our promise.
    if term.modes.get(private, number).is_none() {
        return;
    }
    term.modes.set(private, number, enabled);
    // These mirrors let the ASCII hot path avoid a mode-table scan per
    // printed cell while DECRQM still sees the same current values.
    match (private, number) {
        (true, 5522) if !enabled => {
            // Revocation survives a later re-enable before Session drains VT facts.
            term.clipboard_grant_epoch = term.clipboard_grant_epoch.wrapping_add(1);
        }
        (false, 4) => term.insert_mode = enabled,
        (false, 20) => term.lnm = enabled,
        (true, 7) => {
            term.autowrap = enabled;
            if !enabled {
                term.cursor.pending_wrap = false;
            }
        }
        (true, 6) => term.cup(1, 1),
        (true, 69) => {
            term.lrmm_enabled = enabled;
            // Disabling DECLRMM invalidates the old narrow horizontal
            // region; a later CR must return to physical column zero.
            let (width, _) = term.size();
            term.margins.left = 0;
            term.margins.right = width - 1;
            term.cup(1, 1);
        }
        (true, 47 | 1047) => term.switch_screen(enabled, enabled && number == 1047),
        (true, 1048) => {
            if enabled {
                term.save_cursor();
            } else {
                term.restore_cursor();
            }
        }
        (true, 1049) => {
            // Save on the primary screen before switching. On exit, switch
            // first so DECRC reads the primary screen's own saved slot.
            if enabled {
                term.save_cursor();
                term.switch_screen(true, true);
            } else {
                term.switch_screen(false, false);
                term.restore_cursor();
            }
        }
        (true, 3) if term.modes.get(true, 40) == Some(true) => {
            // DECCOLM is intentionally non-resizing but still resets screen
            // content and margins when xterm's allow-132-columns mode is on.
            term.erase_display(2, false);
            term.reset_margins();
        }
        _ => {}
    }
}
