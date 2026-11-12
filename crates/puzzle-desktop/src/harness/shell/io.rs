//! Headless PTY transport that records write boundaries.
//!
//! The diagnostic needs the bytes the terminal wrote, not a predicted
//! encoder result. A reply to nvim's CSI query and a user key both cross
//! Transport::write, so the wrapper records each call independently.
//! The inner PTY writer still receives the unmodified slice immediately;
//! logging cannot reorder input or alter nvim's mode negotiation.
use crate::pty::LocalPty;
use puzzle_term::platform::{PtySize, Transport, Waker};
use std::{
    io::Read,
    sync::{Arc, Mutex, mpsc},
    time::Instant,
};
/// Recording wrapper passes the exact bytes to the real PTY.
pub(super) struct RecordedPty {
    /// OS master writer and resize handle.
    pub(super) inner: LocalPty,
    /// Shared evidence visible after Session takes transport ownership.
    pub(super) writes: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Transport for RecordedPty {
    /// Retains write boundaries so a duplicated key is observable.
    /// A joined byte buffer would make one key plus one accidental IME
    /// commit harder to distinguish from a single encoded event.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writes
            .lock()
            .map_err(|error| error.to_string())?
            .push(bytes.to_vec());
        self.inner.write(bytes)
    }
    /// nvim starts at a fixed grid in this regression.
    fn resize(&mut self, size: PtySize) -> Result<(), String> {
        self.inner.resize(size)
    }
    /// Closing the wrapper closes the same OS writer.
    fn close(&mut self) -> Result<(), String> {
        self.inner.close()
    }
}

/// Headless output does not need a display wakeup.
/// Session still asks for frames after parsed damage, but the scenario
/// explicitly advances two turns and inspects cells rather than pixels.
pub(super) struct NoFrame;
impl Waker for NoFrame {
    /// The scenario explicitly advances frames.
    fn request_frame(&mut self) {}
    /// Timers are irrelevant to input byte negotiation.
    fn wake_at(&mut self, _instant: Instant) {}
}

/// A reader thread keeps PTY blocking behavior off the scenario executor.
/// The synchronous channel backs up the child if it floods output, as the
/// live window does; an unbounded diagnostic queue could mask starvation.
pub(super) fn read_output(mut reader: Box<dyn Read + Send>, sender: mpsc::SyncSender<Vec<u8>>) {
    let mut buffer = [0_u8; 8192];
    while let Ok(count) = reader.read(&mut buffer) {
        if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
            break;
        }
    }
}
