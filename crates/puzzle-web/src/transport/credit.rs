//! Input credit leaves the socket free to carry control messages (§12.3).
//!
//! A queued paste may be larger than the server's fixed input ring. Holding
//! its unsent suffix here prevents an ACK from being stranded behind input
//! the server cannot accept. No client-side timer polls for more credit.
use super::protocol::{INPUT_WINDOW, PAYLOAD_CHUNK};
use std::collections::VecDeque;

/// Input writes preserve FIFO order while the server is not reading stdin.
#[derive(Default)]
pub struct InputCredit {
    /// Bytes currently permitted by 0x84, independently of output ACKs.
    available: usize,
    /// Already encoded inputs, including replies, retain their exact order.
    pending: VecDeque<Pending>,
    /// Whole writes retain their completion identity across codec-sized chunks.
    in_flight: VecDeque<Flight>,
    /// No server can acknowledge bytes that have not been sent yet.
    sent: usize,
    /// Initial window grants no writer completion credit.
    initialized: bool,
    /// Completed reply IDs return to Session after the socket borrow ends.
    completed: VecDeque<u64>,
    /// Partial delivery of the first queued write without copying its tail.
    offset: usize,
}

/// The socket host calls these methods only on input and credit events.
impl InputCredit {
    /// Grants validated credit; a malformed peer cannot overflow the ledger.
    pub fn grant(&mut self, bytes: u32) -> Result<(), &'static str> {
        // A server can return credit in many small grants. Validate their sum
        // against the shared ring bound before changing usable input capacity.
        let next = self
            .available
            .checked_add(bytes as usize)
            .ok_or("input credit overflow")?;
        if bytes == 0 || next > INPUT_WINDOW as usize {
            return Err("input credit exceeds bridge window");
        }
        if self.initialized {
            if bytes as usize > self.sent {
                return Err("credit exceeds written input");
            }
            // Each grant covers a contiguous prefix of actual socket sends.
            // A logical write can span many such grants and codec-sized fragments.
            // Its unsent tail stays in remaining, but cannot be ACKed via sent.
            let mut remaining = bytes as usize;
            while remaining != 0 {
                let Some(first) = self.in_flight.front_mut() else {
                    return Err("credit has no outstanding input");
                };
                let count = remaining.min(first.remaining);
                first.remaining -= count;
                remaining -= count;
                if first.remaining == 0 {
                    let id = first.reply;
                    // Dropping the lease precedes returning its receipt to Session.
                    self.in_flight.pop_front();
                    if let Some(id) = id {
                        self.completed.push_back(id);
                    }
                }
            }
            self.sent -= bytes as usize;
        } else {
            if bytes != INPUT_WINDOW {
                return Err("initial credit differs from bridge window");
            }
            self.initialized = true;
        }
        self.available = next;
        Ok(())
    }

    /// Appends a complete write, never letting a key overtake a stalled paste.
    pub fn push(&mut self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            self.pending.push_back(Pending {
                bytes: InputBytes::Owned(bytes),
                reply: None,
            });
        }
    }

    /// Program replies retain their Session receipt until actual bridge writer credit.
    pub fn push_reply(&mut self, bytes: Vec<u8>, id: u64) {
        self.pending.push_back(Pending {
            bytes: InputBytes::Owned(bytes),
            reply: Some(id),
        });
    }

    /// A precharged immutable payload stays borrowed until the bridge writer ACK.
    pub fn push_lease(&mut self, lease: Box<dyn LeaseBytes>, id: u64) {
        self.pending.push_back(Pending {
            bytes: InputBytes::Leased(lease),
            reply: Some(id),
        });
    }

    /// Taking a completion never grants more socket permission by itself.
    /// Draining is owner-local; initial window grants never populate this queue.
    /// FIFO whole-write boundaries preserve protocol reply sequence order.
    pub fn take_completed(&mut self) -> Option<u64> {
        self.completed.pop_front()
    }

    /// Spends available credit on one codec-sized payload; None means sleep.
    pub fn take_chunk(&mut self) -> Option<Vec<u8>> {
        let first = self.pending.front()?;
        // A window spans several codec messages; a short final tail spends
        // only its own bytes and leaves the rest of the grant available.
        let len = self
            .available
            .min(PAYLOAD_CHUNK)
            .min(first.bytes.bytes().len() - self.offset);
        if len == 0 {
            return None;
        }
        // The queued allocation remains intact until its final chunk. A
        // later grant can resume precisely where the previous one stopped.
        // Start exactly one receipt when the first fragment leaves the queue.
        // Following fragments update sent without creating another receipt.
        if self.offset == 0 {
            // One receipt covers a logical write, not an arbitrary socket packet.
            self.in_flight.push_back(Flight {
                remaining: first.bytes.bytes().len(),
                reply: first.reply,
                retained: None,
            });
        }
        let bytes = first.bytes.bytes()[self.offset..self.offset + len].to_vec();
        self.sent += len;
        self.offset += len;
        self.available -= len;
        // The final socket send still precedes remote PTY consumption.
        // Move a lease into Flight so removing Pending cannot release it early.
        // The later grant drops Flight before returning its Session receipt.
        if self.offset == first.bytes.bytes().len() {
            if let Some(pending) = self.pending.pop_front()
                && let InputBytes::Leased(lease) = pending.bytes
                && let Some(flight) = self.in_flight.back_mut()
            {
                // Keep precharged storage immutable until actual server consumption,
                // not merely until the last socket fragment has been sent.
                flight.retained = Some(lease);
            }
            self.offset = 0;
        }
        Some(bytes)
    }
}

/// User writes and program replies share precisely the same pending FIFO.
struct Pending {
    /// Owned payload until its final codec fragment is sent.
    bytes: InputBytes,
    /// None for a user key/paste; Session receipt for a program reply.
    reply: Option<u64>,
}

/// Server credit retires one logical write only after all its bytes were consumed.
struct Flight {
    /// Includes unsent tail; global sent prevents acknowledging that tail prematurely.
    remaining: usize,
    /// Reply credit cannot return before the last byte of this write.
    reply: Option<u64>,
    /// Only leased protocol storage needs retention after its final send.
    retained: Option<Box<dyn LeaseBytes>>,
}

/// A transport-boundary lease exposes bytes but no platform object or mutation.
pub trait LeaseBytes: Send {
    /// Used payload only; unused capacity and callback receipt prefixes stay private.
    fn bytes(&self) -> &[u8];
}
impl<T: AsRef<[u8]> + Send> LeaseBytes for T {
    /// Arc-backed native-core leases and test leases share this narrow view.
    fn bytes(&self) -> &[u8] {
        self.as_ref()
    }
}

/// Large user pastes keep their Vec; optional protocol replies share prepaid storage.
enum InputBytes {
    /// Existing input ownership needs no additional copy or virtual call.
    Owned(Vec<u8>),
    /// One rare transport seam object, retained through actual writer completion.
    Leased(Box<dyn LeaseBytes>),
}
impl InputBytes {
    /// The codec sees the same slice for both ownership forms.
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Leased(lease) => lease.bytes(),
        }
    }
}

#[cfg(test)]
/// Pastes and parser replies share the same credit and FIFO.
mod tests;
