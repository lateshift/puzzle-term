//! Native UI effects consume owned results after Session policy has completed.
//!
//! No OS clipboard handle moves to the parser. Reads return a Paste message;
//! writes retain policy-approved text. Result delivery clears its wake latch
//! before draining, so a concurrently queued fact cannot lose its notification.
//!
//! Clipboard policy and clipboard I/O have different owners: OSC 52 and bindings
//! are authorized on Session, then this outbox carries the approved transaction.
//! Read results return through the same Paste policy as ordinary input, rather
//! than bypassing sanitization or bracketed-paste mode at the OS boundary.
//! HTML and plain text remain paired for one copy operation. The system adapter
//! chooses its supported representation without another Session query.
//!
//! Titles and native cursor shapes must be applied on the winit owner. Animation
//! and wake facts instead forward small values to the process rendering actor.
//! Damage carries original output arrival times, not this UI delivery timestamp;
//! coalescing event wakes must not turn continuous output into D-33 bursts.
//! A deadline is absolute: main-thread delivery cannot restart a sync or blink timer.
//! Returned region tables belong to the renderer's reusable allocation pool and
//! move onward without making the UI owner inspect overlay state.
//! Child closure is reported to WindowApp after the event batch, avoiding mutation
//! of its window collection while this InputWindow is borrowed for dispatch.
use super::InputWindow;
use puzzle_term::{
    HostEvent,
    messages::Event,
    platform::{Clipboard, ClipboardEffect},
};

impl InputWindow {
    /// Applies one App::tick result on the native owner; no event receiver lives here.
    pub(in crate::window) fn handle_result(&mut self, event: HostEvent) -> bool {
        match event {
            HostEvent::Drop(effect) => self.drop_effect(effect),
            HostEvent::InputState(state) => self.input_state = Some(state),
            HostEvent::Fact(Event::TitleChanged(title)) => self.window.set_title(&title),
            HostEvent::Fact(Event::SessionCloseRequested) => return true,
            HostEvent::CursorChanged(cursor) => self.apply_overlay_cursor(cursor),
            HostEvent::BoundAction { action, dark, .. } => self.binding_action(action, dark),
            HostEvent::Clipboard(effect) => {
                if let Err(error) = self.clipboard_effect(effect) {
                    tracing::warn!(event="desktop.clipboard_failed", %error);
                }
            }
            // Bell, exit, grid and search facts are delivered even when this minimal
            // harness needs no native operation for them. Real hosts choose their UI.
            _ => {}
        }
        false
    }

    /// OS transactions stay on the window owner; returned text re-enters policy normally.
    fn clipboard_effect(&self, effect: ClipboardEffect) -> Result<(), String> {
        // Construct the OS adapter here; the Session thread holds no AppKit capability.
        let mut clipboard = crate::clipboard::ArboardClipboard;
        match effect {
            ClipboardEffect::Mime(transaction) => {
                let answer = crate::clipboard::complete(transaction);
                self.session
                    .clipboard_answer(answer)
                    .map_err(|error| error.to_string())
            }
            ClipboardEffect::Read => crate::clipboard::paste(&self.session),
            ClipboardEffect::Write(text) => clipboard.write_text(&text),
            ClipboardEffect::WriteHtml { html, text } => clipboard.write_html(&html, &text),
        }
    }
}
