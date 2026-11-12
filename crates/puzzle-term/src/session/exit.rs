//! Direct held-exit notice after the PTY stream has ended (§4.7).
//!
//! The parser has no application left to speak for. Writing the notice into
//! cells avoids interpreting our own status text as an escape stream. Each
//! notice line follows an ordinary line feed, so a full screen's final error
//! scrolls into history instead of being overwritten by the notice.
use crate::{
    messages::ExitInfo,
    vt::{
        Terminal,
        cell::{Attrs, Cell},
        input::KittyModes,
        modes::Modes,
    },
};

/// Paints the process status and makes every next key a host-close request.
pub(super) fn show_notice(term: &mut Terminal, info: &ExitInfo) {
    // Only input negotiation is reset. Display modes such as DECSCNM must
    // still describe the application output that the held screen preserves.
    let defaults = Modes::default();
    for (private, number) in [
        // ANSI LNM affects Return encoding, but not the displayed cells.
        (false, 20),
        // DECCKM and DECNKM choose application cursor and keypad bytes.
        (true, 1),
        (true, 66),
        // The Meta switches choose whether Alt gains an ESC prefix.
        (true, 1036),
        (true, 1039),
        // All tracking variants must stop reporting pointer input to a
        // child that no longer exists, including motion and drag reports.
        (true, 9),
        (true, 1000),
        (true, 1002),
        (true, 1003),
        // Focus reports are input events, independent of the retained grid.
        (true, 1004),
        // Reset each mouse encoding as well as its tracking mode; a stale
        // encoding could affect a replacement child using the same session.
        (true, 1005),
        (true, 1006),
        (true, 1015),
        (true, 1016),
        // Alternate-scroll translates wheel movement into key input.
        (true, 1007),
        // Bracketed paste changes transport bytes and cannot survive exit.
        (true, 2004),
    ] {
        term.modes
            .set(private, number, defaults.get(private, number) == Some(true));
    }
    // The hidden cursor distinguishes a held transcript from a live prompt.
    term.modes.set(true, 25, false);
    // Kitty's stacks are screen-local; clear both screens so the dead child
    // cannot leave a progressive keyboard mode for subsequent host input.
    term.kitty_keys = KittyModes::default();
    term.cursor_style = 0;
    let (_, rows) = term.size();
    // Signals take precedence over exit codes because operating systems can
    // supply a placeholder code even when a signal caused termination.
    let status = if let Some(signal) = &info.signal {
        format!("[Process ended by signal {signal}]")
    } else if let Some(code) = info.code {
        format!("[Process exited with code {code}]")
    } else {
        // EOF does not guarantee a portable-pty exit code on every platform.
        "[Process exited]".to_owned()
    };
    append_line(term, &status);
    if rows > 1 {
        append_line(term, "Press any key to close.");
    }
}

/// Moves to a fresh row and writes dim status cells without parsing escapes.
fn append_line(term: &mut Terminal, text: &str) {
    // Clear delayed autowrap before LF so the notice always starts at column
    // zero, while the ordinary LF retains primary-screen scrollback.
    term.cursor.column = 0;
    term.cursor.pending_wrap = false;
    term.line_feed();
    let index = term.cursor.row;
    term.edit_row(index, |row| {
        // Clearing sparse extras prevents a former hyperlink or grapheme
        // from surviving under the direct notice's ASCII cells.
        row.extras.clear();
        row.cells
            .fill(Cell::erased(crate::vt::cell::PackedColor::DEFAULT));
        // Only actual notice columns become stored; its erased suffix remains
        // implicit. Taking the logical width clips text at the terminal edge.
        for (column, ch) in text.chars().take(row.cells.len()).enumerate() {
            let cell = &mut row.cells[column];
            cell.ch = u32::from(ch);
            cell.attrs = Attrs::DIM;
        }
        // Terminal records damage and a global write stamp after this closure.
    });
}
