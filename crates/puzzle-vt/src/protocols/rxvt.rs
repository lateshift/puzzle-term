//! rxvt protocol interpretation; common state stays in Terminal (D-58).
//! Reference: <https://cvs.schmorp.de/rxvt-unicode/doc/rxvtperl.3.man>.
//! OSC 777 separates notification title and body; mode 1015 uses decimal coordinates.
use super::strings::{clean, payload};
use crate::terminal::{Terminal, TerminalEvent};

/// Converts the `notify;title;body` form to a bounded policy request.
pub(crate) fn notification_777(term: &mut Terminal, params: &[&[u8]]) {
    // Non-notify OSC 777 extensions are outside the supported dialect.
    if params.get(1) != Some(&b"notify".as_slice()) {
        return;
    }
    let title = clean(
        params
            .get(2)
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .unwrap_or_default(),
        256,
    );
    let body = clean(payload(params, 3), 1024);
    term.events
        .push(TerminalEvent::Notification { title, body });
}

/// Encodes rxvt's decimal mouse coordinates after common tracking/modifier policy.
pub(crate) fn mouse_report(button: u16, x: u32, y: u32) -> Vec<u8> {
    // Unlike SGR, rxvt reports releases through the numeric button value.
    // The caller has already applied the modifier and release bits.
    format!("\x1b[{};{x};{y}M", button.saturating_add(32)).into_bytes()
}
