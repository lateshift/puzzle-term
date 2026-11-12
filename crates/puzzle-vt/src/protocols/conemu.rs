//! ConEmu OSC 9;4 progress reports (D-58).
//! Reference: <https://conemu.github.io/en/AnsiEscapeCodes.html#ConEmu_specific_OSC>.
use super::strings::number;
use crate::terminal::{Terminal, TerminalEvent};

/// Interprets the bounded state and percentage from an OSC 9;4 report.
pub(crate) fn progress(term: &mut Terminal, params: &[&[u8]]) {
    let state = number(params.get(2)).unwrap_or(0).min(4) as u8;
    let percent = number(params.get(3)).unwrap_or(0).min(100) as u8;
    term.events.push(TerminalEvent::Progress { state, percent });
}
