//! A short source-owner borrow for trusted registered OSC handlers (M19.2).
//! No Session reference escapes a callback and no position is supplied by PTY
//! bytes. Receipts are owned values; queries/release run on the same synchronous
//! owner on desktop and web. Unregistered Sessions never construct this context.
use super::anchors::{Endpoint, EndpointError, EndpointReceipt, Store};
use puzzle_vt::Terminal;

/// Generic tracked-source commands available only inside a registered callback.
pub struct OscContext<'a> {
    /// VT remains owned exclusively by the calling Session.
    pub(super) terminal: &'a mut Terminal,
    /// Every registration has its own receipt namespace and live ownership.
    pub(super) endpoints: &'a mut Store,
    /// Present only at an ordered request boundary, never during source invalidation.
    pub(super) replies: Option<&'a mut crate::session::replies::Lane>,
}

/// Endpoint operations are bounded, synchronous owner work, never host I/O.
impl OscContext<'_> {
    /// Current source generation, changed by clear/reset or a screen transition.
    pub fn epoch(&self) -> u64 {
        self.endpoints.epoch()
    }

    /// Actual shared owner-only marker pool, never copied into a frame.
    /// Charge this once across source consumers, including its retained slack;
    /// existing public selection/search/decoration marker storage is excluded.
    pub fn private_marker_storage_bytes(&self) -> usize {
        self.terminal.private_marker_storage_bytes()
    }

    /// Captures the current ordered text gap within a caller-accounted live cap.
    /// The registered handler supplies its admission allowance, not PTY input.
    /// Refusal leaves cursor, text, existing receipts and identity counter intact.
    pub fn capture_endpoint(&mut self, max_live: usize) -> Result<EndpointReceipt, EndpointError> {
        self.endpoints.capture(self.terminal, max_live)
    }

    /// Borrows current coordinates only for a receipt issued to this handler.
    /// Eviction, release or a destructive source transition returns None.
    pub fn query_endpoint(&self, receipt: &EndpointReceipt) -> Option<Endpoint> {
        self.endpoints.query(self.terminal, receipt)
    }

    /// Releases this receipt's tracker; existing copied values become inert.
    pub fn release_endpoint(&mut self, receipt: &EndpointReceipt) -> bool {
        self.endpoints.release(self.terminal, receipt)
    }
}

/// Optional protocols can precharge an error buffer without coupling core to a schema.
impl OscContext<'_> {
    /// Queues one already budgeted immutable reply without copying its allocation.
    /// None preserves the ordered request boundary. A handler that already
    /// authenticated it retains its result, returns AwaitReply and retries only
    /// admission after writer/callback completion, never semantic execution.
    /// Charge buffer.allocated_bytes once in the protocol's own state ledger,
    /// including while a lease is held by a writer, event or host callback.
    /// Do not mix this operation with legacy Vec replies in the same callback.
    pub fn reply_from_buffer(
        &mut self,
        buffer: &mut crate::platform::reply::ReusableReply,
        bytes: &[u8],
    ) -> Option<u64> {
        self.replies.as_mut()?.precharged(buffer, bytes)
    }
}
