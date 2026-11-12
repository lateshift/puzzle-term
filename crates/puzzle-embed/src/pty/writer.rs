//! Ordered PTY writes with an explicit worker lifetime for embedding Apps.
//!
//! One Session produces the queue, preserving paste/key/reply order. Cancellation
//! wakes the queue with a sentinel and discards queued input; signalling the tty
//! group releases a blocked OS write. Teardown joins after cancelling input.
use super::LocalPty;
use std::{io::Write, sync::mpsc, thread};

/// Input bytes and cancellation share ordering, never a polling control path.
pub(super) enum Request {
    /// Encoded bytes retain the Session owner's producer order.
    Bytes(Vec<u8>),
    /// Program bytes carry credit until the blocking OS write actually finishes.
    Reply(puzzle_term::platform::reply::ReplyWrite),
    /// Installed before the Session owner starts consuming application output.
    Completion(Box<dyn Fn(puzzle_term::platform::reply::ReplyCompletion) + Send + Sync>),
    /// No more input is accepted after the host cancels its session.
    Stop,
}

/// Join ownership returned to the App alongside the Session transport.
///
/// Like `JoinHandle`, dropping this value alone does not join. The App's pipeline
/// owner cancels the child and joins every worker, including on startup failure.
pub struct WriterWorker {
    /// A sentinel can wake an idle writer even while Session retains its sender.
    stop: mpsc::Sender<Request>,
    /// Reader shutdown remains bounded even if a descendant retains the tty.
    read_cancel: super::reader::Cancel,
    /// Cancellation discards queued input instead of writing a large paste before Stop.
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The sole OS writer is joined after Session can no longer enqueue input.
    worker: thread::JoinHandle<()>,
}

impl WriterWorker {
    /// Wakes the queue receiver without waiting for child IO or Session progress.
    pub fn cancel(&self) {
        // Stop queued pastes before waiting for the sentinel behind them. Acquire
        // on the worker pairs with this release; ordinary writes keep FIFO order.
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
        // The reader must also wake when descendants keep an otherwise silent slave.
        self.read_cancel.stop();
        // Failed delivery means the worker already ended after an OS write error.
        let _ = self.stop.send(Request::Stop);
    }

    /// Completes teardown after the child has been cancelled and Session joined.
    pub fn join(self) -> Result<(), String> {
        self.cancel();
        self.worker.join().map_err(|_| "PTY writer panicked".into())
    }
}

impl LocalPty {
    /// Moves live writes off the host thread and returns explicit join ownership.
    pub fn start_writer(&mut self) -> Result<WriterWorker, String> {
        // Headless smoke tests may retain direct writes. Live Apps retain the
        // returned worker until ordered teardown; no blocking write reaches UI.
        let mut writer = self.writer.take().ok_or("PTY writer was closed")?;
        let recorder = self.recorder.clone();
        let (send, receive) = mpsc::channel();
        // Cancellation is independent of queue depth, including a multi-megabyte paste.
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = cancelled.clone();
        let worker = thread::Builder::new()
            .name("puzzle-pty-writer".into())
            .spawn(move || {
                run_writer(writer.as_mut(), receive, stopping, recorder);
            })
            .map_err(|error| format!("start PTY writer: {error}"))?;
        self.queued = Some(send.clone());
        Ok(WriterWorker {
            stop: send,
            worker,
            read_cancel: self.read_cancel.clone(),
            cancelled,
        })
    }
}

/// One FIFO handles keys, pastes and charged replies without concurrent OS writes.
fn run_writer(
    writer: &mut dyn Write,
    receive: mpsc::Receiver<Request>,
    stopping: std::sync::Arc<std::sync::atomic::AtomicBool>,
    recorder: Option<crate::recording::Recorder>,
) {
    // The callback is installed as a FIFO control before output parsing begins.
    // A successful channel send alone cannot return any program-reply credit.
    // Only write_all completion proves the kernel accepted this payload prefix.
    // Cancellation discards work only as part of explicit transport shutdown.
    // Failure reports a broken lane, distinct from a consumed reply.
    let mut complete: Option<
        Box<dyn Fn(puzzle_term::platform::reply::ReplyCompletion) + Send + Sync>,
    > = None;
    for request in receive {
        // Cancellation is checked before every payload, independently of queue depth.
        if stopping.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        let (bytes, ticket): (&[u8], _) = match &request {
            Request::Bytes(bytes) => (bytes, None),
            Request::Reply(reply) => (&reply.bytes, Some(reply.ticket)),
            Request::Completion(_) => {
                if let Request::Completion(callback) = request {
                    complete = Some(callback);
                }
                continue;
            }
            Request::Stop => break,
        };
        // Shutdown signals release a blocked kernel write; teardown always joins us.
        if let Err(error) = writer.write_all(bytes) {
            tracing::warn!(event = "desktop.pty_write_failed", %error);
            if let Some(complete) = &complete {
                complete(puzzle_term::platform::reply::ReplyCompletion::Failed(
                    error.to_string(),
                ));
            }
            break;
        }
        tracing::debug!(target: "puzzle_desktop::input", event = "PtyWrite", byte_count = bytes.len());
        if let Some(recorder) = &recorder
            && let Err(error) = recorder.bytes("pty-write", bytes)
        {
            tracing::warn!(event = "desktop.record_failed", %error);
        }
        // Drop immutable leases before waking the producer. Otherwise it could
        // observe a still-borrowed precharged buffer and sleep without another wake.
        drop(request);
        // Credit means actual OS acceptance. Queue receipt is intentionally insufficient.
        if let Some(ticket) = ticket
            && let Some(complete) = &complete
        {
            complete(puzzle_term::platform::reply::ReplyCompletion::Written(
                ticket,
            ));
        }
    }
}
