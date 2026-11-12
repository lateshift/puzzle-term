//! The PTY drain owns blocking reads and child reaping independently of rendering.
//!
//! Closing the native input owner kills its child so an outstanding read ends.
//! Final bytes enter the bounded pool before EOF and the exit notification.
//! The producer’s fixed buffer pool applies kernel backpressure at four MiB.
//! Reaping waits for the final read rather than discarding already-written output.
//! A signaled child reports its signal independently of a numeric exit code.
//! Unknown wait errors still retain the last screen through Session exit policy.
//! Neither PTY blocking operation needs a GPU, a text context, or the UI lock.
//! The parser receives the status after every queued output buffer is consumed.
use crate::pty_pool::Producer;
use puzzle_term::messages::ExitInfo;
use std::{io::Read, sync::mpsc::Sender, thread, time::Instant};
/// EOF and process signalling share a lifetime gate, never a Session lock.
mod lifecycle;
pub use lifecycle::Drain;

/// Moves the sole blocking read loop off the UI thread (§4.4).
pub fn start_reader(
    mut reader: Box<dyn Read + Send>,
    mut output: Producer,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    started: Instant,
    exit: Sender<ExitInfo>,
) -> Drain {
    let (lifecycle, completion) = lifecycle::State::new(child);
    let state = lifecycle.clone();
    let worker = thread::spawn(move || {
        loop {
            match output.read_once(reader.as_mut()) {
                Ok(false) => break,
                Ok(true) => {}
                Err(error) => {
                    tracing::debug!(%error, "PTY reader ended");
                    break;
                }
            }
        }
        // A closed pool consumer means cancellation, not PTY EOF. The child can
        // be stuck in macOS tty-close (E state) until unread output is consumed.
        // Keep the last read descriptor alive only for discard-through-EOF, then
        // close it BEFORE reaping; the Session/master and writer close separately.
        let mut discard = [0; crate::pty_pool::BUFFER_SIZE];
        loop {
            match reader.read(&mut discard) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break, // Unix EIO is the normal PTY slave-close boundary.
            }
        }
        drop(reader);
        // The producer drops only after EOF/EIO. The parser can then drain
        // every pooled byte before it submits this status to Session.
        // Reaping after the final read preserves output that remained in the
        // PTY kernel buffer after the process had already exited.
        let status = state.reap();
        let info = match status {
            Ok(status) => ExitInfo {
                // A signaled process has no trustworthy numeric exit code.
                code: status
                    .signal()
                    .is_none()
                    .then(|| i32::try_from(status.exit_code()).ok())
                    .flatten(),
                signal: status.signal().map(str::to_owned),
                // Runtime is monotonic and saturates at the wire's u64 bound.
                runtime_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            },
            Err(error) => {
                // A failed wait is still an exit boundary. Holding an unknown
                // signal makes the last screen readable instead of vanishing.
                tracing::warn!(event = "desktop.child_wait_failed", %error);
                ExitInfo {
                    code: None,
                    signal: Some("unknown".into()),
                    runtime_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                }
            }
        };
        let _ = exit.send(info);
        state.complete();
    });
    Drain::new(worker, lifecycle, completion)
}
