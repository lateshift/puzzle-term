//! Common xterm OSC 52 clipboard request (§16).
//! Reference: <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html>.
//! Writes become typed policy requests; reads remain denied by default.
use crate::terminal::{Terminal, TerminalEvent};

/// Converts a clipboard write to a policy request; never answers a read.
pub(super) fn clipboard(term: &mut Terminal, params: &[&[u8]]) {
    // A missing data field is not a request to clear the clipboard.
    let Some(data) = params.get(2) else {
        return;
    };
    if *data == b"?" {
        // Denying reads is the default §16 security policy, independent
        // of whether a Session later permits writes after interception.
        tracing::debug!("OSC 52 clipboard read denied by default");
        return;
    }
    if let Some(text) = super::base64::decode(data) {
        // Session applies the ClipboardWrite policy before host I/O.
        term.events.push(TerminalEvent::ClipboardWrite(text));
    } else {
        tracing::warn!(
            event = "vt.osc52_invalid",
            "discarded invalid OSC 52 clipboard data"
        );
    }
}
