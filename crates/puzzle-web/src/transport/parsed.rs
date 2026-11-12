//! ACKs and write barriers follow actual parsing, not message receipt (§12.3).
//!
//! Session may consume several queued writes in one turn, or only a prefix
//! of one. A provenance ledger attributes that count to the original FIFO
//! spans; host-injected output must not return credit to the bridge.
use super::protocol::ACK_THRESHOLD;
use std::collections::VecDeque;

/// One accepted output span retains the connection that owns its ACK.
struct Span {
    /// Unconsumed bytes; zero-sized writes need no span.
    left: usize,
    /// None denotes API output, not WebSocket output.
    connection: Option<u64>,
}

/// Monotonic offsets let each JS write await its own parsed prefix.
#[derive(Default)]
pub struct Parsed {
    /// Total bytes accepted through both API and application output paths.
    pub accepted: u64,
    /// Total bytes actually consumed by Session, not merely buffered.
    pub consumed: u64,
    /// FIFO attribution, so a replacement socket cannot ACK an old socket.
    spans: VecDeque<Span>,
    /// Credit accumulated since the last 64 KiB ACK for one connection.
    ack: Option<(u64, usize)>,
}

/// Completion is observed after the host flushes replies from the same turn.
impl Parsed {
    /// Records an accepted write and returns its completion barrier.
    pub fn accept(&mut self, len: usize, connection: Option<u64>) -> u64 {
        self.accepted += len as u64;
        if len > 0 {
            self.spans.push_back(Span {
                left: len,
                connection,
            });
        }
        self.accepted
    }

    /// Attributes a Session parse count and emits only mature bridge ACKs.
    pub fn advance(&mut self, mut bytes: usize) -> Result<Vec<(u64, u32)>, &'static str> {
        if bytes as u64 > self.accepted - self.consumed {
            return Err("parser consumed bytes absent from provenance ledger");
        }
        // Validate the whole count before advancing any offset. An invalid
        // attribution cannot falsely satisfy a waiting write barrier.
        self.consumed += bytes as u64;
        let mut acks = Vec::new();
        while bytes > 0 {
            let span = self.spans.front_mut().ok_or("parser span missing")?;
            let n = bytes.min(span.left);
            if let Some(id) = span.connection {
                // A reconnected transport starts a fresh credit ledger. An
                // old partial ACK cannot be charged to the new application.
                let ack = self.ack.get_or_insert((id, 0));
                if ack.0 != id {
                    *ack = (id, 0);
                }
                ack.1 += n;
                // ACK the complete accumulated amount, rather than rounded
                // blocks: each byte belongs to exactly one returned grant.
                if ack.1 >= ACK_THRESHOLD {
                    acks.push((id, ack.1 as u32));
                    ack.1 = 0;
                }
            }
            span.left -= n;
            bytes -= n;
            if span.left == 0 {
                self.spans.pop_front();
            }
        }
        Ok(acks)
    }
}

#[cfg(test)]
/// Split slices cannot resolve or ACK output that is still queued.
mod tests;
