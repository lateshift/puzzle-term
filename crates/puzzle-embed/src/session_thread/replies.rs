//! Writer completion and immutable-buffer readiness resume only this Session.
//! No queue receipt returns credit: the writer acknowledges actual consumption.
//! The owner can sleep on its priority inbox while program output stays in the
//! fixed data pool. Keys, frame requests and shutdown all wake that same inbox.
use super::*;

/// Completion is owner-local state work, separate from the blocking OS writer.
impl Owner {
    /// Invalid/replayed receipts cannot release bytes or terminate the owner.
    pub(super) fn apply_reply_completion(
        &mut self,
        completion: puzzle_term::platform::reply::ReplyCompletion,
    ) {
        use puzzle_term::platform::reply::ReplyCompletion;
        let message = match completion {
            // A released callback lease is readiness only, not byte consumption.
            ReplyCompletion::Ready => {
                self.session.reply_storage_ready();
                return;
            }
            // Return the opaque boundary; Session validates the FIFO before reuse.
            ReplyCompletion::Written(ticket) => {
                puzzle_term::SessionMessage::ReplyCompleted(ticket.id())
            }
            // Broken input closes the session instead of acknowledging lost bytes.
            ReplyCompletion::Failed(error) => puzzle_term::SessionMessage::ReplyFailed(error),
        };
        if let Err(error) = self.session.apply(message, Instant::now()) {
            tracing::warn!(event = "desktop.reply_completion_rejected", %error);
        }
    }
}

/// Program stdin backpressure suspends parsing, never priority controls or snapshots.
impl Owner {
    /// Sleep on the guaranteed writer/control wake rather than polling a full data pool.
    pub(super) fn wait_reply_credit(&mut self) -> bool {
        // Credits arrive on this same control inbox, so sleeping cannot lose a wake.
        // Shutdown is explicit and also reaches this inbox before any worker join.
        // Local user work must not wait for a non-reading program. The normal
        // bounded empty feed runs it, then this path sleeps when it is finished.
        if self.session.owner_work_pending() || !self.session.waiting_reply_credit() {
            return false;
        }
        // Send any earlier host effects before sleeping for the matching receipt.
        self.deliver();
        match self.control.recv() {
            Ok(control) => self.apply_control(control),
            // A disconnected control lane is also an unambiguous shutdown wake.
            Err(_) => self.stopping = true,
        }
        true
    }
}
