//! Link URIs are revalidated here after interceptors can rewrite the request.
//! The opener receives no disallowed scheme even through direct API dispatch.
//! Byte encoding and host-service effects after command policy (§6.8).
//! Input commands return bytes only after the handler applies host actions.
//!
//! Input samples live VT modes at dispatch and writes before the pump returns.
//! UI selection/navigation commands have no bytes and cannot inject input.
//! ClipboardRead is internal owner plumbing, not a generic user paste operation.
//! A MIME result never enters the application's input encoder as ordinary text.
//! Legacy text and HTML copy paths retain their original clipboard semantics.
//! The MIME path instead waits for an explicit asynchronous completion message.
//! Drag effects use that same post-policy owner-to-host queue, never native callbacks here.
//! Reset and child exit remove a registered drop target without scanning ordinary input.
use super::Handler;
use crate::{
    messages::Command,
    vt::input::{InputModes, encode_key, encode_paste},
};
impl Handler<'_> {
    /// Converts accepted input with the modes active at this exact pump.
    pub(super) fn input_or_host_action(&mut self, command: &Command) -> Vec<u8> {
        // Keyboard modes belong to VT and may change with the last parsed
        // escape sequence, so they are sampled at dispatch rather than keydown.
        let modes = InputModes::from_terminal(self.terminal);
        match command {
            Command::Mouse(event) => self.mouse_bytes(*event),
            Command::Key(event) => {
                // A key may use kitty CSI u or the legacy encoding, depending
                // on the application's current negotiated flags.
                self.input_seen = true;
                encode_key(event, modes)
            }
            Command::Text(text) => {
                // IME text is already committed Unicode and bypasses the
                // key encoder's modifier interpretation.
                self.input_seen = true;
                // A commit ends the composition; retained preedit must not draw twice.
                self.preedit.text.clear();
                self.preedit.cursor = None;
                text.as_bytes().to_vec()
            }
            Command::Paste(text) => {
                // Bracketed paste markers are selected by the VT mode at
                // dispatch, matching applications that toggle it dynamically.
                self.input_seen = true;
                encode_paste(text, modes)
            }
            _ => self.host_action(command),
        }
    }

    /// Native effects run after policy and produce no application input bytes.
    fn host_action(&mut self, command: &Command) -> Vec<u8> {
        match command {
            // This internal command carries the already-authorized pending request.
            Command::ClipboardRead => {
                self.clipboard.issue(command, self.services.clipboard.as_mut(), &mut self.terminal.replies);
                Vec::new()
            }
            // All formats share the policy kind; only their host execution differs.
            Command::ClipboardWrite(content) => {
                self.write_clipboard(command, content);
                Vec::new()
            }
            Command::OpenLink(uri) => {
                // External dispatch and rewritten clicks share the same final safe filter.
                if self.link_policy.allows(uri, !self.web_search)
                    && let Err(error) = self.services.urls.open(uri) {
                    tracing::warn!(event = "session.link_open_failed", %error);
                }
                Vec::new()
            }
            Command::Drop(effect) => self.forward_drop_effect(effect),
            Command::DesktopNotification(request) => {
                // Completion arrives separately, allowing output parsing to continue meanwhile.
                self.notifications.request(request.clone(), self.now, self.services.notifier.as_mut(), &mut self.terminal.replies);
                Vec::new()
            }
            Command::Notify { title, body } => {
                // Notifications are facts for the host service and cannot
                // become arbitrary bytes in the child process's stdin.
                self.notifications.request(crate::vt::notification::Request::Post(crate::vt::notification::Notification {
                    title: title.clone(), body: body.clone(), ..Default::default()
                }), self.now, self.services.notifier.as_mut(), &mut self.terminal.replies);
                Vec::new()
            }
            // VT pointer requests have no native host implementation in M6.
            // An extension with no installed handler is similarly inert at
            // this boundary; bus observers already saw its typed payload.
            // These effect-only commands have already been observed by the host.
            Command::SetPointerShape(_)
            | Command::RingBell
            | Command::Ext(_)
            | Command::ChildExited(_) => Vec::new(),
            // State-changing commands are handled by earlier dispatch branches.
            Command::Key(_) | Command::Text(_) | Command::Paste(_) | Command::Mouse(_)
            | Command::SetSystemAppearance(_)
            | Command::BindingAction { .. }
            | Command::Preedit(_)
            | Command::Focus(_)
            | Command::Scroll(_)
            | Command::ScrollTo(_)
            | Command::Wheel { .. }
            | Command::Pointer(_)
            | Command::Copy(_)
            // Search never emits application bytes through the encoder path.
            | Command::Search(_)
            | Command::GlyphRasterized
            | Command::OverlayAction(_)
            | Command::Select(_) => Vec::new(),
        }
    }
    /// Tick delivers accepted drag effects on the host thread, outside Session ownership.
    fn forward_drop_effect(&mut self, effect: &crate::dnd::Effect) -> Vec<u8> {
        // The bus policy lane has already authorized this registration or acceptance.
        self.host_events
            .push_back(crate::HostEvent::Drop(effect.clone()));
        Vec::new()
    }
    /// Reset/exit append just one drop lifecycle fact; no ordinary pump scans events.
    pub(super) fn forward_drop_reset(&mut self) {
        // Reset appends disposal last, so inspecting just the tail preserves the unused fast path.
        if matches!(
            self.terminal.events.last(),
            Some(crate::vt::terminal::TerminalEvent::Drop(_))
        ) && let Some(crate::vt::terminal::TerminalEvent::Drop(effect)) =
            self.terminal.events.pop()
        {
            // The child can no longer negotiate: release its host registration immediately.
            self.host_events.push_back(crate::HostEvent::Drop(effect));
        }
    }
    /// Preserves text/HTML behavior while MIME writes use explicit host completion.
    fn write_clipboard(&mut self, command: &Command, content: &crate::messages::ClipboardContent) {
        use crate::messages::ClipboardContent;
        // The caller reached this handler only after the shared write interceptors.
        let result = match content {
            ClipboardContent::Text(text) => self.services.clipboard.write_text(text),
            // Rich copy includes its plain alternative in one native transaction.
            ClipboardContent::Html { html, text } => self.services.clipboard.write_html(html, text),
            ClipboardContent::Mime { .. } => {
                // Issuing an effect cannot acknowledge a still-pending OS operation.
                self.clipboard.issue(
                    command,
                    self.services.clipboard.as_mut(),
                    &mut self.terminal.replies,
                );
                return;
            }
        };
        // Only an executed, successful host-service call can confirm a copy.
        // The native service queues clipboard/write; it does not claim an OS completion.
        self.clipboard_written |= result.is_ok();
        if let Err(error) = result {
            tracing::warn!(event = "session.clipboard_write_failed", %error);
        }
    }
}
