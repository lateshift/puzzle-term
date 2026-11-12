//! A clipboard response advances one protocol packet at a time, without I/O.
//! Binary contents stay in their existing bounded shared allocations. Only the
//! next <=4096-byte DATA chunk is base64 encoded. Session can therefore wait for
//! writer credit without allocating the complete encoded clipboard response.
//!
//! The cursor serves both eager standalone VT replies and credit-driven Sessions.
//! It stores offsets into existing MIME data instead of a full encoded response.
//! Opening OK, DATA chunks and final DONE have one shared formatting implementation.
//! A failed read emits its protocol error and immediately becomes finished.
//! Successful writes emit DONE directly; they have no outgoing data body.
//! Successful reads emit OK even when the MIME list is empty, then DONE.
//! Empty MIME representations emit one DATA packet before advancing to the next.
//! offset advances by decoded bytes; the packet encoder supplies Base64 padding.
//! Every call either changes phase, advances bytes/item or returns finished.
//! The append-only API leaves buffer clearing/reuse to the transport-credit owner.
//! No clock or completion callback belongs in this pure protocol encoder.
use super::{MAX_READ_CHUNK_BYTES, header, packet, status_name};
use crate::clipboard::{MimeData, Operation, Request, Status};

/// Owned asynchronous result; no subsequent packet can overtake its DONE packet.
pub struct Response {
    /// Sanitized correlation and the original read/write operation.
    request: Request,
    /// Validated host outcome, mapped to the protocol by the packet encoder.
    status: Status,
    /// Arc-backed bounded payloads; no duplicate binary copies are constructed.
    items: Vec<MimeData>,
    /// Only scalar offsets survive a blocked writer.
    cursor: Cursor,
}

impl Response {
    /// Takes the validated native result without eagerly encoding any data.
    pub fn new(request: Request, status: Status, items: Vec<MimeData>) -> Self {
        Self {
            request,
            status,
            items,
            cursor: Cursor::default(),
        }
    }

    /// Appends exactly the next complete packet; false means the stream ended.
    /// Callers retain packet ordering and decide when credit permits another call.
    pub fn append_next(&mut self, out: &mut Vec<u8>) -> bool {
        self.cursor
            .append_next(&self.request, self.status, &self.items, out)
    }
}

/// The borrowed encoder and owned asynchronous encoder use the identical state machine.
#[derive(Default)]
pub(super) struct Cursor {
    /// Opening status precedes every DATA packet.
    started: bool,
    /// DONE or an error has already been emitted; later calls are inert.
    finished: bool,
    /// All chunks of one MIME remain contiguous on the wire.
    item: usize,
    /// Decoded offset in the current binary representation.
    offset: usize,
}

impl Cursor {
    /// Progress is one opening packet, one data chunk/item, or the final DONE.
    pub(super) fn append_next(
        &mut self,
        request: &Request,
        status: Status,
        items: &[MimeData],
        out: &mut Vec<u8>,
    ) -> bool {
        if self.finished {
            return false;
        }
        let read = matches!(request.operation, Operation::Read(_));
        if !self.started {
            self.started = true;
            if status != Status::Done || !read {
                // Kitty read errors have a smaller vocabulary than write errors.
                let status =
                    if read && matches!(status, Status::Io | Status::Invalid | Status::TooLarge) {
                        Status::Busy
                    } else {
                        status
                    };
                header(request, status_name(status), out);
                out.extend_from_slice(b"\x1b\\");
                self.finished = true;
            } else {
                header(request, "OK", out);
                out.extend_from_slice(b"\x1b\\");
            }
        } else if let Some(item) = items.get(self.item) {
            let end = (self.offset + MAX_READ_CHUNK_BYTES).min(item.data.len());
            // An empty representation still produces one DATA packet, then advances.
            packet(request, item, &item.data[self.offset..end], out);
            self.offset = end;
            if end == item.data.len() {
                self.item += 1;
                self.offset = 0;
            }
        } else {
            header(request, "DONE", out);
            out.extend_from_slice(b"\x1b\\");
            self.finished = true;
        }
        true
    }
}

/// Clipboard and other protocol streams use one generic Session admission lane.
impl crate::reply_stream::ReplySource for Response {
    fn append_next(&mut self, out: &mut Vec<u8>) -> bool {
        Response::append_next(self, out)
    }
}
