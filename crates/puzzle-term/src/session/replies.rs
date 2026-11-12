//! The Session owns reply admission; transports own only admitted writer work.
//!
//! Keys and paste retain one FIFO with replies. A large clipboard result stays
//! as bounded Arc-backed data and encodes packets incrementally; later keys are
//! accepted immediately into this owner queue, behind that response's DONE.
//! Only writer completion releases program-reply credit. Parser callers leave
//! subsequent application text in their existing pool/backlog while blocked.
//!
//! There are two byte owners: this lane's staged bytes and admitted writer payloads.
//! available subtracts both, so moving storage never doubles the allowance.
//! can_parse reserves a complete bounded semantic callback before entering vte.
//! Its derived headroom covers OSC expansion and all fixed CSI/DCS responses.
//! Each query then yields before another callback can use that same reservation.
//! Most replies are tiny; staging groups them into the existing codec chunk size.
//! Input and extension boundaries flush staging before issuing later receipts.
//! A synchronous in-memory transport avoids ownership copies and ticket allocation.
//! An asynchronous writer owns exact-sized bytes until it reports consumption.
//! Bytes queued behind a lazy stream already spend credit before they are retained.
//! Large authorized data keeps its protocol-bounded source allocation, not Base64.
//! Only a packet is encoded when writer credit and this turn's work bound permit.
//! DONE/EOF retains the stream's FIFO position until the generator says finished.
//! This prevents an input key or later query from splitting a logical response.
//! Keys are accepted on the owner immediately; they need no reply admission.
//! Their actual OS delivery still follows bytes already destined for that peer.
//! Neither completing a receipt nor an input command can encode an unbounded stream.
//! pump performs bounded encoding and returns control to the Session owner.
//! Empty generators advance by removal; nonempty ones advance their source cursor.
//! A capacity pause returns the unchanged generator to the front of the queue.
//! Precharged replies reserve their shared allocation, not merely its used prefix.
//! They are already admitted when queued, so pump must not reserve them again.
//! A full protocol state table can return an error from its precharged allocation.
//! An immutable callback lease can outlive writer completion; readiness is separate.
//! No program output is accepted past the boundary while either resource is busy.
//! Transport failures become errors to the Session; they never manufacture credit.
//! Cancellation belongs to the host lifecycle and wakes a blocked native writer.
//! The FIFO contains values only, with no Session references or platform locks.
//! The web invokes the same owner methods on its single thread.
use crate::platform::{
    Transport,
    reply::{ReplyBytes, ReplyCredit, ReplyDelivery, ReplyWrite, ReusableReply},
};
use std::collections::VecDeque;

/// One semantic VT callback is bounded by the existing OSC buffer. Base64 of
/// an unknown key expands by 4/3; wrappers and vte's <=16 OSC fields fit in the
/// remaining 2/3. Fixed CSI/DCS replies are smaller. Optional handlers must obey
/// this same output bound. This is headroom derived from §6.4, not a new budget.
pub(super) const CALLBACK_HEADROOM: usize = 2 * puzzle_vt::osc::MAX_BUFFER_BYTES;

/// An ordered source may remain unencoded while a non-reading peer exhausts credit.
enum Write {
    /// Already admitted immutable storage, with no second payload allocation.
    Precharged(ReplyWrite),
    /// Generated protocol bytes, already included in queued-byte admission.
    Reply(Vec<u8>),
    /// User data is trusted input and keeps existing large-paste semantics.
    Input(Vec<u8>),
    /// Clipboard storage has its own existing 64-MiB bound; wire bytes are lazy.
    Stream(Box<dyn puzzle_vt::reply_stream::ReplySource>),
}

/// No transport or callback can return credit by merely receiving a buffer.
#[derive(Default)]
pub(crate) struct Lane {
    /// Actual outstanding writer payloads, with non-reusable FIFO receipts.
    credit: ReplyCredit,
    /// Logical input order, including a clipboard stream's later DATA packets.
    queued: VecDeque<Write>,
    /// Encoded program bytes waiting behind an earlier stream/input entry.
    queued_bytes: usize,
    /// Coarse parser turns coalesce small replies into the existing 64-KiB chunk.
    staging: Vec<u8>,
    /// At most one small packet is encoded at a time; allocation is reused.
    scratch: Vec<u8>,
    /// Platform mailbox notification for immutable callbacks released after completion.
    pub(crate) notify: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl Lane {
    /// Scoped handlers transfer precharged storage at their ordered boundary.
    /// None consumes neither writer credit nor a semantic request sequence.
    pub(crate) fn precharged(&mut self, buffer: &mut ReusableReply, bytes: &[u8]) -> Option<u64> {
        let notify = self.notify.clone();
        let (ticket, lease) = buffer.prepare(
            bytes,
            |charge| {
                if charge > self.available() {
                    return None;
                }
                self.credit.admit(charge)
            },
            notify,
        )?;
        self.queued.push_back(Write::Precharged(ReplyWrite {
            ticket,
            bytes: ReplyBytes::Precharged(lease),
        }));
        Some(ticket.id())
    }

    /// Leaves a complete callback's structural headroom before accepting more VT.
    pub(super) fn can_parse(&self) -> bool {
        self.queued.is_empty() && self.available() >= CALLBACK_HEADROOM
    }

    /// True means an external writer completion, rather than a timer, is needed.
    pub(super) fn waiting_credit(&self) -> bool {
        self.available() < CALLBACK_HEADROOM
    }

    /// Pending semantic work must survive EOF even if there is no raw byte suffix.
    pub(super) fn pending(&self) -> bool {
        !self.queued.is_empty()
    }

    /// Counts encoded bytes in both the owner queue and the actual transport.
    pub(super) fn pending_bytes(&self) -> usize {
        self.credit.pending() + self.queued_bytes
    }

    /// Owner-held bytes cannot also be spent as transport credit.
    fn available(&self) -> usize {
        self.credit.available().saturating_sub(self.queued_bytes)
    }

    /// Transfers a reply FIFO only after verifying its entire retained payload fits.
    /// False leaves the original buffer intact; the ordered parser boundary stays put.
    pub(super) fn replies(
        &mut self,
        transport: &mut dyn Transport,
        bytes: &mut Vec<u8>,
    ) -> Result<bool, String> {
        if bytes.is_empty() {
            return self.pump(transport).map(|()| true);
        }
        if bytes.len() > self.available() {
            return Ok(false);
        }
        if self.queued.is_empty() && !transport.deferred_replies() {
            // Recording/in-memory hosts consume synchronously without a new allocation.
            transport.write(bytes)?;
            bytes.clear();
            return Ok(true);
        }
        if self.queued.is_empty() {
            if self.staging.len().saturating_add(bytes.len()) > crate::bridge::PAYLOAD_CHUNK {
                self.flush_staging(transport)?;
            }
            self.queued_bytes += bytes.len();
            self.staging.extend_from_slice(bytes);
            bytes.clear();
            if self.staging.len() >= crate::bridge::PAYLOAD_CHUNK {
                self.flush_staging(transport)?;
            }
            return Ok(true);
        }
        self.queued_bytes += bytes.len();
        self.queued.push_back(Write::Reply(std::mem::take(bytes)));
        self.pump(transport)?;
        Ok(true)
    }

    /// Accepts user input without waiting for credit, preserving the stream's position.
    pub(super) fn input(
        &mut self,
        transport: &mut dyn Transport,
        bytes: &[u8],
    ) -> Result<(), String> {
        if self.queued.is_empty() {
            self.flush_staging(transport)?;
            return transport.write(bytes);
        }
        self.queued.push_back(Write::Input(bytes.to_vec()));
        self.pump(transport)
    }

    /// A native read result takes one place in the FIFO before any later key/reply.
    pub(super) fn stream(
        &mut self,
        transport: &mut dyn Transport,
        response: Box<dyn puzzle_vt::reply_stream::ReplySource>,
    ) -> Result<(), String> {
        self.queued.push_back(Write::Stream(response));
        self.pump(transport)
    }

    /// Validates a completion once, then resumes queued work without a parser call.
    pub(super) fn complete(
        &mut self,
        _transport: &mut dyn Transport,
        id: u64,
    ) -> Result<(), String> {
        self.credit.complete_id(id).map_err(str::to_owned)?;
        _transport.reply_completed(id);
        Ok(())
    }

    /// Sends only ready front entries; no later key or response overtakes a stream.
    pub(super) fn pump(&mut self, transport: &mut dyn Transport) -> Result<(), String> {
        self.flush_staging(transport)?;
        // Existing feed/chunk size bounds encoding work per continuation turn.
        let mut encoded = 0;
        while let Some(front) = self.queued.pop_front() {
            match front {
                Write::Precharged(reply) => {
                    if let ReplyDelivery::Complete(ticket) = transport.write_reply(reply)? {
                        self.credit.complete(ticket).map_err(str::to_owned)?;
                    }
                }
                Write::Input(bytes) => transport.write(&bytes)?,
                Write::Reply(bytes) => {
                    // These bytes were charged when queued, so transfer cannot overbook.
                    self.queued_bytes -= bytes.len();
                    self.send(transport, bytes)?;
                }
                Write::Stream(mut response) => {
                    if self.available() < CALLBACK_HEADROOM
                        || encoded >= crate::bridge::PAYLOAD_CHUNK
                    {
                        self.queued.push_front(Write::Stream(response));
                        break;
                    }
                    self.scratch.clear();
                    if response.append_next(&mut self.scratch) {
                        encoded += self.scratch.len();
                        // Retain the stream's place until its final packet has been sent.
                        self.queued.push_front(Write::Stream(response));
                        let bytes = std::mem::take(&mut self.scratch);
                        self.send(transport, bytes)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// No query-sized allocation is sent to the native channel: this reusable
    /// scratch groups small replies, and input/stream/turn boundaries flush it.
    fn flush_staging(&mut self, transport: &mut dyn Transport) -> Result<(), String> {
        if self.staging.is_empty() {
            return Ok(());
        }
        let bytes = self.staging.clone();
        self.queued_bytes -= bytes.len();
        self.staging.clear();
        self.send(transport, bytes)
    }

    /// All asynchronous transports receive the identical exact-sized charged payload.
    fn send(&mut self, transport: &mut dyn Transport, bytes: Vec<u8>) -> Result<(), String> {
        if !transport.deferred_replies() {
            transport.write(&bytes)?;
            self.scratch = bytes;
            return Ok(());
        }
        let ticket = self
            .credit
            .admit(bytes.len())
            .ok_or("reply exceeded admitted writer credit")?;
        let reply = ReplyWrite {
            ticket,
            bytes: ReplyBytes::Owned(bytes.into_boxed_slice()),
        };
        if let ReplyDelivery::Complete(ticket) = transport.write_reply(reply)? {
            self.credit.complete(ticket).map_err(str::to_owned)?;
        }
        Ok(())
    }
}

#[cfg(test)]
/// A stalled writer retains exact replies, ordered keys and a bounded lazy read.
mod tests;
