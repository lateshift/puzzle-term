//! Completion-based credit for program replies, independent of any OS transport.
//!
//! A ticket identifies an admitted prefix of the one Session-owned writer lane.
//! The writer returns tickets in FIFO order after actual consumption, not after
//! enqueueing. Cumulative offsets make duplicate, forged and out-of-order
//! completions rejectable. Foreign completion IDs are also checked against the
//! bounded FIFO of actual admission boundaries. User keystrokes
//! do not spend this credit; their existing writer order remains unchanged.
//! This is the generic robustness repair approved by the reviewer 2026-10-14.

/// Optional protocols precharge reusable payload storage before admitting state.
mod buffer;
pub use buffer::{ReplyBytes, ReplyLease, ReusableReply};

/// D-29 allocates 64 reusable 64-KiB payloads per session (§4.4/§16).
/// Replies use the same existing allowance; this is not a new budget.
pub const PENDING_BYTE_ALLOWANCE: usize = 64 * 65_536;

/// An opaque, non-reusable receipt for bytes admitted to the ordered writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyTicket {
    /// Cumulative admitted byte offset, never wrapped or reset on RIS.
    end: u64,
    /// Exact payload charge; the transport must complete this whole ticket.
    bytes: usize,
}

impl ReplyTicket {
    /// Stable correlation value for foreign hosts; not an authorization token.
    pub fn id(self) -> u64 {
        self.end
    }

    /// Number of program-reply bytes this completion releases.
    pub fn bytes(self) -> usize {
        self.bytes
    }
}

/// One owner-local ledger bounds every transport's outstanding program replies.
#[derive(Default)]
pub struct ReplyCredit {
    /// End of the last successfully admitted reply, including completed bytes.
    issued: u64,
    /// End of the last FIFO writer completion.
    completed: u64,
    /// Foreign hosts may return IDs only; retain the exact issued boundaries.
    tickets: std::collections::VecDeque<ReplyTicket>,
}

impl ReplyCredit {
    /// Remaining credit, excluding bytes the writer has not yet consumed.
    pub fn available(&self) -> usize {
        PENDING_BYTE_ALLOWANCE - self.pending()
    }

    /// Exact outstanding bytes; admitted-minus-completed always fits the allowance.
    pub fn pending(&self) -> usize {
        // Admission proves the difference fits usize on native and wasm32.
        (self.issued - self.completed) as usize
    }

    /// Reserves one whole reply before handing its owned bytes to a transport.
    /// None is backpressure or counter exhaustion, never permission to drop bytes.
    pub fn admit(&mut self, bytes: usize) -> Option<ReplyTicket> {
        if bytes == 0 || bytes > self.available() {
            return None;
        }
        let end = self.issued.checked_add(bytes as u64)?;
        self.issued = end;
        let ticket = ReplyTicket { end, bytes };
        self.tickets.push_back(ticket);
        Some(ticket)
    }

    /// Returns credit only for the next whole FIFO completion.
    /// Replays, gaps and future receipts leave every counter unchanged.
    pub fn complete(&mut self, ticket: ReplyTicket) -> Result<(), &'static str> {
        if self.tickets.front() != Some(&ticket)
            || self.completed.checked_add(ticket.bytes as u64) != Some(ticket.end)
            || ticket.end > self.issued
            || ticket.bytes == 0
        {
            return Err("reply completion is not the next admitted write");
        }
        self.tickets.pop_front();
        self.completed = ticket.end;
        Ok(())
    }

    /// Resolves a foreign host's correlation ID without accepting invented prefixes.
    pub fn complete_id(&mut self, id: u64) -> Result<(), &'static str> {
        let ticket = self
            .tickets
            .front()
            .copied()
            .ok_or("no reply awaits completion")?;
        if ticket.id() != id {
            return Err("reply completion does not identify the next write");
        }
        self.complete(ticket)
    }
}

/// Transport ownership keeps payload memory within the Session's charged lane.
/// Hosts must not signal completion merely because they received this value.
pub struct ReplyWrite {
    /// Receipt returned only after the full byte slice is consumed, in order.
    pub ticket: ReplyTicket,
    /// Exact-sized allocation: unused vector capacity cannot escape byte accounting.
    pub bytes: ReplyBytes,
}

/// Whether a transport completed synchronously or will send a later completion.
pub enum ReplyDelivery {
    /// In-memory transports consumed the bytes during this call.
    Complete(ReplyTicket),
    /// OS/foreign transports retain credit until their writer actually completes.
    Pending,
}

#[cfg(test)]
/// Whole-prefix admission, duplicate refusal and exact capacity boundary proofs.
mod tests;

/// Completion is actual writer consumption, or an explicit broken transport.
/// A failed transport is never mistaken for successfully delivered reply bytes.
pub enum ReplyCompletion {
    /// A callback released immutable reply storage after acknowledging its writer.
    Ready,
    /// The full admitted payload was consumed in FIFO order.
    Written(ReplyTicket),
    /// No further reply can be promised; the host must close this session.
    Failed(String),
}
