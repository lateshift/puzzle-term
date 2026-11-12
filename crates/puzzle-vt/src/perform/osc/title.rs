//! OSC 0/1/2 title changes and CSI 22/23 t stack state (§6.5, §16).
//!
//! Window titles are untrusted bytes. Filter controls before retaining
//! text, then stop at 1024 Unicode scalars so multi-byte UTF-8 cannot
//! bypass the character limit. The stack keeps at most ten snapshots;
//! pushing an eleventh discards the oldest one instead of growing memory.
//! OSC 0 sets both icon and title, OSC 1 only the icon, and OSC 2 only the
//! title (xterm ctlseqs). Keeping them separate matters when a shell sets
//! its icon once but updates the title for each command. CSI 22/23 t uses
//! a stack so nested programs can restore the previous values on exit.

use crate::terminal::{Terminal, TerminalEvent};

/// Applies a bounded icon/title update from an OSC payload.
pub(super) fn dispatch(term: &mut Terminal, code: u16, params: &[&[u8]]) {
    // Decode lossy UTF-8 before counting scalars: malformed byte streams
    // become replacement characters rather than panicking or leaking C0.
    let text = super::payload(params, 1)
        .chars()
        .filter(|ch| !ch.is_control())
        .take(1024)
        .collect::<String>();
    if matches!(code, 0 | 1) {
        // The fact is separate from the stored value for Session observers.
        term.icon_name.clone_from(&text);
        term.events
            .push(TerminalEvent::IconNameChanged(text.clone()));
    }
    if matches!(code, 0 | 2) {
        // Title text is emitted only after control stripping and length cap.
        term.title = text.clone();
        term.events.push(TerminalEvent::TitleChanged(text));
    }
}

impl Terminal {
    /// Saves title state for CSI 22 t, evicting the oldest of ten entries.
    pub(crate) fn push_title(&mut self) {
        // xterm's title stack is bounded here by the §16 ten-entry limit.
        if self.title_stack.len() == 10 {
            self.title_stack.remove(0);
        }
        self.title_stack
            .push((self.icon_name.clone(), self.title.clone()));
    }

    /// Restores requested icon/title fields for CSI 23 t.
    pub(crate) fn pop_title(&mut self, kind: u16) {
        // A missing stack entry leaves the current window labels intact.
        let Some((icon, title)) = self.title_stack.pop() else {
            return;
        };
        if kind != 2 {
            // CSI 23;2 t restores title only; all other forms include icon.
            self.icon_name = icon.clone();
            self.events.push(TerminalEvent::IconNameChanged(icon));
        }
        if kind != 1 {
            // CSI 23;1 t restores icon only; all other forms include title.
            self.title = title.clone();
            self.events.push(TerminalEvent::TitleChanged(title));
        }
    }
}
