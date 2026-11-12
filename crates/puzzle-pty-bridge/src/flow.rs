//! ACK-controlled PTY reads with a bounded overshoot (design §12.3).
//!
//! Credit includes bytes read but still queued, not just flushed WebSocket
//! messages. Reservations happen before OS reads. One reader can cross the
//! 1 MiB threshold by at most one 64 KiB chunk; no second read is permitted
//! until parsed-byte acknowledgements reduce it. ACKs cannot retire unsent
//! bytes, overflow a counter, or invent credit for future output.
use crate::protocol::{OUTPUT_WINDOW, PAYLOAD_CHUNK};
use std::sync::{Condvar, Mutex};

/// Counters protected together so wait/notify cannot lose a wakeup.
#[derive(Default)]
struct State {
    /// Read reservations plus completed reads, until acknowledged.
    pending: usize,
    /// Bytes accepted by the WebSocket, and therefore eligible for ACK.
    sent: usize,
    /// Socket cleanup releases any reader blocked on credit.
    closed: bool,
}

/// One flow ledger per shell; no cross-session credit sharing.
#[derive(Default)]
pub(crate) struct Flow {
    /// Reservation and acknowledgement update the same locked state.
    state: Mutex<State>,
    /// The reader sleeps while the browser catches up; no polling loop.
    ready: Condvar,
}

/// Credit transitions preserve the pending-byte bound even on disconnect.
impl Flow {
    /// Reserves a single bounded read, sleeping while outstanding credit is high.
    pub(crate) fn reserve(&self) -> Result<bool, String> {
        let mut state = self.state.lock().map_err(|_| "flow lock poisoned")?;
        while state.pending > OUTPUT_WINDOW && !state.closed {
            // Wait releases the mutex atomically; ack and close notify
            // while owning it, so neither signal can race the predicate.
            tracing::debug!(event = "bridge.backpressure", pending_bytes = state.pending);
            state = self.ready.wait(state).map_err(|_| "flow wait poisoned")?;
        }
        if state.closed {
            return Ok(false);
        }
        state.pending += PAYLOAD_CHUNK;
        Ok(true)
    }

    /// Returns unused reservation bytes after a short read (common on macOS).
    pub(crate) fn read_complete(&self, count: usize) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "flow completion poisoned")?;
        // The reader never supplies more than PAYLOAD_CHUNK bytes to read().
        state.pending -= PAYLOAD_CHUNK - count;
        Ok(())
    }

    /// Makes a successfully queued WebSocket output eligible for acknowledgment.
    pub(crate) fn sent(&self, count: usize) -> Result<(), String> {
        self.state.lock().map_err(|_| "flow send poisoned")?.sent += count;
        Ok(())
    }

    /// Rejects over-acknowledgement instead of granting fabricated read credit.
    pub(crate) fn ack(&self, count: u32) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "flow ack poisoned")?;
        let count = count as usize;
        if count > state.sent {
            return Err("ACK exceeds transmitted output".into());
        }
        state.sent -= count;
        state.pending -= count;
        self.ready.notify_all();
        Ok(())
    }

    /// Disconnect cannot leave a shell reader asleep forever on flow credit.
    pub(crate) fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
            self.ready.notify_all();
        }
    }
}

/// Credit forgery, pause/resume and disconnect races.
#[cfg(test)]
mod tests;
