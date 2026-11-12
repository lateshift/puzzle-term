//! Optional work is bounded by the host's existing parsing quantum (draft §9).
//! One OSC request stays in Registry's reusable buffer while deferred. Later
//! application bytes stay in the normal Session/pool backlog, so controls and
//! frames keep their existing independent path and no extra queue is created.
use super::Registry;

/// The Session declares turns; wire boundaries cannot manufacture extra credits.
impl Registry {
    /// Begins once per public feed, including an empty continuation at child EOF.
    pub(crate) fn begin_turn(&mut self) {
        // Empty registration storage has no callbacks, allocation or source scan.
        for registration in &mut self.handlers {
            registration.handler.begin_turn();
        }
    }

    /// Ends once, even when feed_to_sync_end first drains an older pending suffix.
    pub(crate) fn end_turn(&mut self) {
        for registration in &mut self.handlers {
            registration.handler.end_turn();
        }
    }

    /// True means request is still owned by this boundary, not free parser scratch.
    pub(crate) fn pending(&self) -> bool {
        self.pending
    }

    /// External waits sleep on completion instead of replenishing work credits in a spin.
    pub(crate) fn waiting_reply(&self) -> bool {
        self.waiting_reply
    }

    /// A completion or released callback makes the pending request eligible to retry.
    pub(crate) fn reply_ready(&mut self) {
        self.waiting_reply = false;
    }

    /// Counts retained wire work so EOF and platform continuation cannot miss it.
    pub(crate) fn pending_bytes(&self) -> usize {
        // A real captured request always includes its introducer and terminator.
        // Its bytes are counted even when there is no ordinary pending suffix.
        let wire = if self.pending {
            self.request.sequence_bytes
        } else {
            0
        };
        // A user export may need turns while the PTY is quiet. Its virtual
        // pending unit contains no application byte and cannot consume input.
        wire.saturating_add(usize::from(self.owner_work_pending()))
    }
}
