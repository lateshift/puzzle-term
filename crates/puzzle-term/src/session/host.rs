//! Typed outboxes for a single-thread platform with foreign callbacks (§4.3).
//!
//! A Session and its services remain Send. Hosts own the JS references and
//! move queued effects out after each synchronous turn, then release the
//! Session borrow before invoking callbacks. This is not a second bus: input
//! still passes normal interceptors, and events still obey observer masks.
//! Host answer messages complete one correlated clipboard transaction.
//! A trusted paste observation chooses mode 5522 before ordinary bracketed paste.
use super::Session;
use crate::{
    messages::Command,
    platform::{TransportEffect, WakeRequests},
    vt::input::KeyEvent,
};
use puzzle_bus::Origin;
use web_time::Instant;

/// Host handoffs preserve synchronous dispatch without exposing service state.
impl Session {
    /// Transfers clipboard effects after policy, without retaining foreign callbacks.
    pub fn take_clipboard_effect(&mut self) -> Option<crate::platform::ClipboardEffect> {
        self.services.clipboard.take_effect()
    }
    /// Returns queued output without consuming any additional parser budget.
    pub fn pending_bytes(&self) -> usize {
        // A deferred extension has consumed its framing, but its ordered work
        // still needs a continuation even at EOF with no following text bytes.
        self.pending
            .len()
            .saturating_add(self.osc.pending_bytes())
            .saturating_add(usize::from(self.replies.pending()))
    }

    /// Recording offsets count consumption wherever the owner drains pending output.
    pub fn parsed_bytes_total(&self) -> u64 {
        self.parsed_total
    }

    /// Takes ordered transport work after pump/feed.
    /// Native services perform I/O directly and return None. Deferred services
    /// move ownership of bytes instead of sharing a queue with platform code.
    pub fn take_transport_effect(&mut self) -> Option<TransportEffect> {
        self.services.transport.take_effect()
    }

    /// Takes coalesced scheduling requests; the platform supplies refresh time.
    /// Asking only after a core turn avoids a polling timer while idle (P2).
    pub fn take_wake_requests(&mut self) -> WakeRequests {
        self.services.waker.take_requests()
    }

    /// Runs normal policies before delegating an unmatched key to textarea text.
    /// True means native input owns this press; the encoder did not send it.
    /// A shortcut or pending leader returns false, so the host prevents native
    /// insertion. Older flushed leaders still encode normally in FIFO order.
    pub fn pump_native_key(&mut self, key: KeyEvent, origin: Origin, now: Instant) -> bool {
        // Drain earlier requests before assigning ownership to one press;
        // a pending leader remains state, not an unprocessed bus command.
        self.pump(now);
        // An exited screen must consume the key through its close handler.
        if self.exit_info().is_some() {
            self.submit(Command::Key(key), origin);
            self.pump(now);
            return false;
        }
        self.keybindings.native_key = Some(key.clone());
        self.keybindings.native_passed = false;
        self.submit(Command::Key(key), origin);
        self.pump(now);
        // A higher-priority veto may have bypassed the matcher entirely.
        // Clear the marker so it cannot affect a later unrelated command.
        self.keybindings.native_key = None;
        self.keybindings.native_passed
    }
}

impl Session {
    /// Completes native MIME work without injecting clipboard bytes as keystrokes.
    pub fn clipboard_answer(&mut self, answer: crate::clipboard::Answer) {
        self.flush_parser_replies();
        self.clipboard.answer(answer, &mut self.terminal.replies);
        if let Some(response) = self.clipboard.take_response()
            && let Err(error) = self
                .replies
                .stream(self.services.transport.as_mut(), Box::new(response))
        {
            tracing::warn!(event = "session.reply_write_failed", %error);
            self.close_requested = true;
        }
        self.flush_clipboard_replies();
    }
    /// Protocol replies use the same ordered writer as VT queries and input.
    pub(super) fn flush_clipboard_replies(&mut self) {
        self.flush_parser_replies();
    }
}

impl Session {
    /// Ends a paste capability for raw input even when a shortcut consumes the event.
    pub fn user_activity(&mut self) {
        self.clipboard.revoke_paste_grant();
    }
    /// Native paste listings sample mode on its owner, so queued mode changes cannot race.
    pub fn clipboard_paste(&mut self, paste: crate::clipboard::Paste, now: Instant) {
        self.clipboard.revoke_paste_grant();
        if self.terminal.modes.get(true, 5522) == Some(true) {
            self.clipboard.paste_event(
                paste,
                self.terminal.clipboard_grant_epoch,
                &mut self.terminal.replies,
            );
            self.flush_clipboard_replies();
        } else if let Some(text) = paste.text {
            self.submit(Command::Paste(text), Origin::User);
            self.pump(now);
        }
    }
}

/// Platforms wake this owner by message, never by polling an exhausted writer.
impl Session {
    /// Bounded local optional work can run while a program is not reading stdin.
    /// Platforms service it before sleeping on external reply credit.
    pub fn owner_work_pending(&self) -> bool {
        self.osc.owner_work_pending()
    }
    /// True means parsing must wait for a completion; input/publication remain usable.
    pub fn waiting_reply_credit(&self) -> bool {
        self.replies.waiting_credit() || self.osc.waiting_reply()
    }
    /// Counts all queued and in-flight program bytes, excluding user paste data.
    /// Both raw callback bytes and already admitted transport bytes count here.
    /// User input retains its separate existing paste/backpressure policy.
    pub fn pending_reply_bytes(&self) -> usize {
        self.replies.pending_bytes() + self.terminal.replies.len()
    }
    /// Installs a nonblocking platform mailbox before the owner starts parsing.
    /// The readiness callback releases no bytes; only Written carries a receipt.
    /// One shared mailbox handles both resources without retaining a Session.
    /// The platform installs it before any application output is parsed.
    pub fn set_reply_completion(
        &mut self,
        complete: Box<dyn Fn(crate::platform::reply::ReplyCompletion) + Send + Sync>,
    ) {
        let complete: std::sync::Arc<
            dyn Fn(crate::platform::reply::ReplyCompletion) + Send + Sync,
        > = complete.into();
        let ready = complete.clone();
        self.replies.notify = Some(std::sync::Arc::new(move || {
            ready(crate::platform::reply::ReplyCompletion::Ready)
        }));
        self.services
            .transport
            .set_reply_completion(Box::new(move |result| complete(result)));
    }
    /// A foreign writer acknowledges one exact FIFO receipt, never a byte estimate.
    /// Unknown, replayed and out-of-order IDs leave the ledger unchanged.
    /// The concrete transport releases any immutable callback lease after validation.
    /// Handler wakeup then permits retry without consuming the request twice.
    pub fn reply_completed(&mut self, id: u64) -> Result<(), String> {
        self.replies
            .complete(self.services.transport.as_mut(), id)?;
        self.osc.reply_ready();
        Ok(())
    }
}

impl Session {
    /// A callback released its immutable lease after an earlier acknowledgement.
    pub fn reply_storage_ready(&mut self) {
        self.osc.reply_ready();
    }
}

/// Failed program input is a broken transport, never a successful completion.
impl Session {
    /// The host closes through its normal lifecycle lane; no writer bytes are credited.
    pub(super) fn fail_reply(&mut self, error: String) {
        tracing::error!(event = "session.reply_write_failed", %error);
        self.close_requested = true;
    }
}

/// Pointer results are owner-side facts, carried back to the host as values.
impl Session {
    /// Captured drags/leave route even without a terminal mouse report.
    /// Cursor facts also restore the default shape when no overlay retains hover.
    pub(super) fn apply_overlay_pointer(
        &mut self,
        event: puzzle_overlay::provider::PointerEvent,
        now: Instant,
    ) {
        let result = self.route_overlay_pointer(&event, now);
        // Host presentation adopts the result after this mutable borrow ends.
        self.host_events
            .push_back(super::host_events::HostEvent::CursorChanged(result.cursor));
    }
}
