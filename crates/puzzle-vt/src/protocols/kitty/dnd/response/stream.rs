//! Packet cursors keep OSC 72 reads/listings bounded by transport credit.
//!
//! Data and lists have different ending rules, so their cursor types are distinct.
//! Data owns an Arc reference to the already authorized bounded drop representation.
//! Creating it cannot clone the user's binary contents or access the clipboard.
//! offset is a decoded-byte position and increases by a protocol-sized raw chunk.
//! The common packet encoder supplies alphabet, padding and the original terminator.
//! The last nonempty packet has m=0, followed by the required empty EOF packet.
//! An empty representation therefore still emits one empty EOF before finishing.
//! finished prevents duplicate EOF when a caller probes again after completion.
//! The client number is captured once; later drag registration cannot reroute it.
//! Likewise each cursor keeps the MIME index and BEL/ST choice of its request.
//! List payloads are callback-local printable metadata, so they are copied once.
//! Lists can span several packets while holding their FIFO position in Session.
//! Each packet copies at most the protocol payload cap; no full-response Vec exists.
//! List continuation m=1 means another list chunk follows, not another MIME value.
//! A final list chunk ends the response without the binary-data EOF convention.
//! The eager encoder and these cursors share packet/data_packet to prevent drift.
//! A false return never appends bytes and allows the owner to remove the cursor.
//! A true return makes monotonic progress, independent of display or timer state.
//! Writer backpressure is enforced by Session before asking for the next packet.
use super::{MAX_PAYLOAD_BYTES, MAX_RAW_CHUNK_BYTES, data_packet, packet};
use crate::reply_stream::ReplySource;
use std::sync::Arc;

/// One permitted drop read retains its existing immutable source, never encoded output.
pub(crate) struct Data {
    /// Validated one-based MIME index echoed in every packet.
    index: i64,
    /// Registered multiplexer identity.
    client: u32,
    /// Existing bounded gesture allocation, shared without cloning its contents.
    bytes: Arc<[u8]>,
    /// Next decoded byte; monotonic progress ends at the source length.
    offset: usize,
    /// The final empty EOF has already been emitted.
    finished: bool,
    /// BEL/ST choice belongs to the original request.
    bell: bool,
}
impl Data {
    /// Captures only already validated, user-authorized immutable drop bytes.
    pub(crate) fn new(index: i64, client: u32, bytes: Arc<[u8]>, bell: bool) -> Self {
        Self {
            index,
            client,
            bytes,
            offset: 0,
            finished: false,
            bell,
        }
    }
}
impl ReplySource for Data {
    /// Every call advances the decoded cursor or emits exactly one final EOF.
    fn append_next(&mut self, out: &mut Vec<u8>) -> bool {
        if self.finished {
            return false;
        }
        if self.offset < self.bytes.len() {
            let end = (self.offset + MAX_RAW_CHUNK_BYTES).min(self.bytes.len());
            data_packet(
                out,
                self.index,
                self.client,
                &self.bytes[self.offset..end],
                end < self.bytes.len(),
                self.bell,
            );
            self.offset = end;
        } else {
            packet(
                out,
                &format!("t=r:x={}:m=0", self.index),
                self.client,
                b"",
                self.bell,
            );
            self.finished = true;
        }
        true
    }
}

/// MIME/status lists keep the same packetization as the original eager encoder.
pub(super) struct List {
    /// Validated protocol header; no program text is interpreted as control syntax.
    header: String,
    /// Registered client, omitted on wire when zero.
    client: u32,
    /// Bounded immutable list, copied once before the native callback returns.
    payload: Box<[u8]>,
    /// Monotonic byte offset through the list.
    offset: usize,
    /// Empty lists still emit one packet, then finish.
    finished: bool,
    /// Original request terminator or ST for unsolicited reports.
    bell: bool,
}
impl List {
    /// Owns callback-local metadata without retaining a foreign pointer.
    pub(super) fn new(header: &str, client: u32, payload: &[u8], bell: bool) -> Self {
        Self {
            header: header.into(),
            client,
            payload: payload.into(),
            offset: 0,
            finished: false,
            bell,
        }
    }
}
impl ReplySource for List {
    /// Plain list chunks need continuation flags, but no separate empty EOF.
    fn append_next(&mut self, out: &mut Vec<u8>) -> bool {
        if self.finished {
            return false;
        }
        let end = (self.offset + MAX_PAYLOAD_BYTES).min(self.payload.len());
        if self.payload.is_empty() {
            packet(out, &self.header, self.client, b"", self.bell);
        } else {
            packet(
                out,
                &format!("{}:m={}", self.header, u8::from(end < self.payload.len())),
                self.client,
                &self.payload[self.offset..end],
                self.bell,
            );
        }
        self.offset = end;
        self.finished = end == self.payload.len();
        true
    }
}
