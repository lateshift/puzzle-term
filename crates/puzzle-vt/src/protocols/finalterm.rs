//! finalterm protocol interpretation; common state stays in Terminal (D-58).
//! Reference: <https://iterm2.com/documentation-escape-codes.html#shell-integrationfinalterm>.
//! OSC 633 is the VS Code alias for the same A/B/C/D boundaries.
//! Unknown marks are ignored without changing a pending prompt or command.
//! Mark timestamps are assigned later by the Session, which owns the clock.
//! Prompt boundaries become tracked marks; storage and lifetime remain in Zones.
use super::strings::number;
use crate::terminal::{Terminal, TerminalEvent};
use crate::{
    row::RowFlags,
    zones::{ZoneKind, ZoneMark},
};

/// Records one OSC 133/633 boundary at the current stable line identity.
pub(crate) fn shell_mark(term: &mut Terminal, params: &[&[u8]]) {
    // FinalTerm OSC 133 and VS Code OSC 633 share these four boundaries.
    let kind = match params.get(1).copied() {
        Some(b"A") => ZoneKind::Prompt,
        Some(b"B") => ZoneKind::Input,
        Some(b"C") => ZoneKind::Output,
        Some(b"D") => ZoneKind::Finished,
        _ => return,
    };
    let exit = if kind == ZoneKind::Finished {
        // Only the D mark carries an optional command exit status.
        number(params.get(2)).map(i32::from)
    } else {
        None
    };
    let Some(line) = term.cursor_line() else {
        // A failed line lookup after malformed resize cannot create an
        // anchor pointing at an unrelated future row.
        return;
    };
    // Zone storage assigns the identity and retains a tracked position, so marks
    // survive scrolling/reflow without the parser retaining screen coordinates.
    let id = term.zones.push(
        ZoneMark {
            // The bounded zone owner assigns an untrusted-OSC-independent identity.
            id: 0,
            line,
            column: term.cursor.column,
            kind,
            exit,
        },
        &mut term.tracked,
    );
    if kind == ZoneKind::Prompt {
        // Prompt flags let row consumers find a likely navigation target
        // without scanning every zone mark during a redraw.
        let row_index = term.cursor.row;
        if let Some(row) = term.screen_mut().row_mut(row_index) {
            row.flags.insert(RowFlags::PROMPT);
        }
    }
    // The owner supplies one injected time for this parsed boundary identity.
    term.events.push(TerminalEvent::ZoneBoundary { id, kind });
    // Consumers receive the boundary fact before its higher-level lifecycle event.
    // The registered lifecycle handler relies on PromptStarted to expire records.
    term.events.push(match kind {
        ZoneKind::Prompt => TerminalEvent::PromptStarted,
        ZoneKind::Input => TerminalEvent::InputStarted,
        ZoneKind::Output => TerminalEvent::OutputStarted(super::kitty::shell::command(params)),
        ZoneKind::Finished => TerminalEvent::CommandFinished(exit),
    });
}
