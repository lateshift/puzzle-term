//! Byte-bounded stdin queue and completion notices (design §12.3 input credit).
//!
//! A message-count limit cannot admit arbitrarily fragmented legal input: a
//! peer might spend its one-MiB grant in one-byte frames. Coalescing bytes into
//! a fixed-capacity ring bounds storage independently of message boundaries.
//! The socket never waits for this queue; the granted byte ledger guarantees
//! space. Only the OS writer blocks, without holding the queue mutex.
//!
//! Credit returns after each successful partial write, not after enqueue or
//! after an entire paste. This lets an echoing child drain incrementally even
//! when output also needs ACKs. Those ACKs remain readable on the socket.
use crate::protocol::{INPUT_WINDOW, PAYLOAD_CHUNK};
use std::{
    collections::VecDeque,
    io::Write,
    sync::{Condvar, Mutex},
};
use tokio::sync::mpsc::Sender;

/// One reusable byte ring; no allocation is proportional to packet count.
pub(crate) struct Input {
    /// Predicate and bytes share the lock, preventing a lost close/enqueue wake.
    state: Mutex<State>,
    /// Idle writers wait for real input or shutdown, never a timer.
    ready: Condvar,
}

/// Queue metadata belongs to the same owner as the pending payload.
struct State {
    /// FIFO bytes already debited against the client's remaining credit.
    bytes: VecDeque<u8>,
    /// Teardown ends a sleeping writer without requiring another key.
    closed: bool,
}

/// Admission is synchronous; backpressure is expressed to the client on the wire.
impl Input {
    /// Allocates the entire input allowance once, before granting it to the peer.
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(State {
                bytes: VecDeque::with_capacity(INPUT_WINDOW as usize),
                closed: false,
            }),
            ready: Condvar::new(),
        }
    }

    /// Copies a credited payload without waiting for the child to read it.
    pub(crate) fn push(&self, bytes: &[u8]) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "input queue poisoned")?;
        // Credit includes the active writer chunk, so the ring must always
        // have this space. Fail visibly rather than grow if accounting breaks.
        if state.closed || state.bytes.len() + bytes.len() > INPUT_WINDOW as usize {
            return Err("input queue unavailable within granted credit".into());
        }
        // Packet boundaries have no meaning to the terminal. Coalescing here
        // avoids one allocation per tiny frame while retaining arrival order.
        state.bytes.extend(bytes);
        if !bytes.is_empty() {
            self.ready.notify_one();
        }
        Ok(())
    }

    /// Takes one chunk into caller-owned scratch; releases the lock before I/O.
    fn take(&self, scratch: &mut Vec<u8>) -> Result<bool, String> {
        let mut state = self.state.lock().map_err(|_| "input queue poisoned")?;
        while state.bytes.is_empty() && !state.closed {
            state = self.ready.wait(state).map_err(|_| "input queue poisoned")?;
        }
        // Teardown is the only path allowed to abandon queued input: the
        // child is being signalled and no surviving session can consume it.
        if state.closed {
            return Ok(false);
        }
        scratch.clear();
        let count = state.bytes.len().min(PAYLOAD_CHUNK);
        // drain preserves byte order across both packet and ring boundaries.
        scratch.extend(state.bytes.drain(..count));
        Ok(true)
    }

    /// Closes only during session teardown; live queued bytes are never discarded.
    pub(crate) fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
            self.ready.notify_all();
        }
    }

    /// Reports OS-accepted bytes through a bounded wake-driven completion channel.
    pub(crate) fn write(
        &self,
        mut writer: Box<dyn Write + Send>,
        completed: &Sender<Result<u32, String>>,
    ) -> Result<(), String> {
        // A reusable chunk limits active-write storage independently of the
        // number of frames coalesced in the ring. Credit includes these bytes.
        let mut scratch = Vec::with_capacity(PAYLOAD_CHUNK);
        while self.take(&mut scratch)? {
            let mut rest = scratch.as_slice();
            while !rest.is_empty() {
                // Short writes must restore only their accepted prefix. Granting
                // the whole chunk here would let a peer exceed physical storage.
                let count = match writer.write(rest) {
                    Ok(0) => return Err("PTY input write returned zero".into()),
                    Ok(count) => count,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(format!("PTY input: {error}")),
                };
                // Completion backpressure bounds notices too. The coordinator
                // stays free to process socket ACKs while this writer waits.
                if completed.blocking_send(Ok(count as u32)).is_err() {
                    return Ok(());
                }
                rest = &rest[count..];
            }
        }
        Ok(())
    }
}
