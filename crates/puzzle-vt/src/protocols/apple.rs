//! apple protocol interpretation; common state stays in Terminal (D-58).
//! Reference: <file:///etc/zshrc_Apple_Terminal>.
//! Apple's /etc/zshrc_Apple_Terminal emits the file URI; attribution evidence
//! and the source-attribution limits are recorded in docs/protocols/apple.md.
//! The current-directory URI is an application hint; it never triggers filesystem access.
use super::strings::{clean, payload};
use crate::terminal::{Terminal, TerminalEvent};

/// Accepts only a bounded file URI as the application's current directory.
pub(crate) fn cwd(term: &mut Terminal, params: &[&[u8]]) {
    // A clean URI is recorded as a hint; no host filesystem is touched.
    let value = clean(payload(params, 1), 2048);
    if !value.starts_with("file://") {
        return;
    }
    term.cwd = Some(value.clone());
    term.events.push(TerminalEvent::CwdChanged(value));
}
