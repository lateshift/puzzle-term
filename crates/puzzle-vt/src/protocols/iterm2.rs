//! iTerm2 OSC 9 notification body (D-58).
//! Reference: <https://iterm2.com/documentation-escape-codes.html>.
//! Synchronized output originated in iTerm2; its common mode bit stays in Modes.
use super::strings::{clean, payload};
use crate::terminal::{Terminal, TerminalEvent};

/// Produces a bounded notification request; Session owns rate limiting and host I/O.
pub(crate) fn notification(term: &mut Terminal, params: &[&[u8]]) {
    // The notification command is policy-controlled downstream; VT
    // only enforces a finite printable body here.
    let body = clean(payload(params, 1), 1024);
    term.events.push(TerminalEvent::Notification {
        title: String::new(),
        body,
    });
}
