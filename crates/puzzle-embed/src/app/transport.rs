//! Host-owned sources use the native fixed pool without a child or drain thread.
//!
//! Only the host accesses admission state, after Surface affinity checks. App
//! teardown shares that small capability solely to close it; the Session never
//! locks it. It receives the same bounded buffers as a Puzzle-owned PTY instead.
//! Completion sends status before dropping the producer, so all accepted output
//! is parsed before ChildExited. A control message would overtake pending bytes.
//!
//! Session writes, resize and close become owned events. The worker invokes only
//! wakeup; App::tick delivers the payloads on the creating host thread. Returning
//! pool storage also wakes the host so a paused source can retry its unaccepted
//! suffix. The caller must retain that suffix; Puzzle never queues it elsewhere.
use super::*;
use crate::pty_pool::{self, Consumer, Producer};
use puzzle_term::{
    messages::ExitInfo,
    platform::{PtySize, Transport},
};
use std::sync::Mutex;

/// The creating host chooses one source for the surface's entire lifetime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransportMode {
    /// Puzzle owns the login shell, PTY, drain pool and ordered writer.
    #[default]
    Pty,
    /// The host supplies output and consumes input/resize/close events in tick.
    Host,
}

/// Clones share lifecycle only; no worker acquires this host-side admission mutex.
#[derive(Clone)]
pub(crate) struct Feed(Arc<Mutex<State>>);
/// Producer lifetime, rather than control-message order, seals the byte stream.
struct State {
    /// None after EOF or cancellation; no later output may be accepted.
    producer: Option<Producer>,
    /// Status is sent before producer EOF becomes visible on the data channel.
    exit: mpsc::Sender<ExitInfo>,
}
impl Feed {
    // Affinity guarantees that only the creating host thread calls admission.
    // The mutex exists because Surface clones are Send and App owns cancellation;
    // it does not guard a Session, and no pipeline thread can contend for it.
    // The copy is bounded by available payloads, not the caller's total input size.
    // A full pool returns immediately and retains no reference to the input slice.
    // EOF is sealed by taking the producer so retained Surface clones cannot reopen it.
    /// Copies only credited output, without a Session wait or unbounded allocation.
    pub(crate) fn output(&self, bytes: &[u8]) -> Result<usize, ApiError> {
        self.0
            .lock()
            .map_err(|_| ApiError::Closed)?
            .producer
            .as_mut()
            .ok_or(ApiError::Closed)?
            .admit(bytes)
            .map_err(Into::into)
    }
    /// Finishes after the accepted prefix; the host must first submit every suffix.
    pub(crate) fn exited(&self, info: ExitInfo) -> Result<(), ApiError> {
        let mut state = self.0.lock().map_err(|_| ApiError::Closed)?;
        if state.producer.is_none() {
            return Err(ApiError::Closed);
        }
        state
            .exit
            .send(info)
            .map_err(|e| ApiError::Platform(e.to_string()))?;
        // Release-store EOF follows status publication and every successful admission.
        state.producer.take();
        Ok(())
    }
    /// App cancellation wakes the owner before any joins, even with Feed clones alive.
    pub(crate) fn cancel(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.producer.take();
        }
    }
}

/// Session-owned effects contain no native object or host UI callback.
struct HostTransport {
    /// Native effects reach the creating host thread through App::tick.
    effects: super::services::Effects,
    /// The host transport queue is the equivalent of the local PTY writer queue.
    metrics: Arc<crate::pipeline_metrics::Metrics>,
    /// Precharged immutable buffers survive callback return until actual consumption.
    leases: std::collections::VecDeque<(u64, puzzle_term::platform::reply::ReplyLease)>,
}
impl Transport for HostTransport {
    /// Keys, paste and replies preserve the sole Session producer's FIFO order.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.effects.post(AppEvent::TransportInput(bytes.into()))?;
        // Enqueue succeeded; later tick delivery and the host's OS write are
        // separate host work, not a synchronous operation on this Session.
        self.metrics.control.queued();
        Ok(())
    }
    /// Host callbacks are admission only; completion returns through Surface.
    fn deferred_replies(&self) -> bool {
        true
    }
    /// Owned data and receipt travel together until the host consumes them.
    fn write_reply(
        &mut self,
        reply: puzzle_term::platform::reply::ReplyWrite,
    ) -> Result<puzzle_term::platform::reply::ReplyDelivery, String> {
        // Callback return is not writer consumption. Keep the shared allocation
        // immutable until the owner validates a later whole-write receipt.
        // This extra reference shares the payload and does not clone its bytes.
        if let Some(lease) = reply.bytes.lease() {
            self.leases.push_back((reply.ticket.id(), lease));
        }
        self.effects.post(AppEvent::TransportReply(reply))?;
        Ok(puzzle_term::platform::reply::ReplyDelivery::Pending)
    }
    /// Validate globally before releasing a protocol's precharged immutable storage.
    fn reply_completed(&mut self, id: u64) {
        if self
            .leases
            .front()
            .is_some_and(|(expected, _)| *expected == id)
        {
            self.leases.pop_front();
        }
    }
    /// Coalesced terminal dimensions are requests to the host's byte source.
    fn resize(&mut self, size: PtySize) -> Result<(), String> {
        self.effects.post(AppEvent::TransportResize(size))
    }
    /// Policy-completed close is delivered only from the main-thread tick.
    fn close(&mut self) -> Result<(), String> {
        self.effects.post(AppEvent::TransportClose)
    }
}

/// Cold source assembly preserves the same owner/data/status transfer as the PTY path.
pub(super) fn start(
    app: &App,
    id: SurfaceId,
) -> (
    Box<dyn Transport>,
    Consumer,
    mpsc::Receiver<ExitInfo>,
    Pipeline,
) {
    let (producer, mut consumer) = pty_pool::pair();
    // A returned payload is credit even if no title or frame event is produced.
    consumer.notify_returned(Arc::clone(&app.config.wakeup));
    let (exit, receive) = mpsc::channel();
    let feed = Feed(Arc::new(Mutex::new(State {
        producer: Some(producer),
        exit,
    })));
    let effects = super::services::Effects {
        id,
        posts: app.posts.clone(),
        wake: Arc::clone(&app.config.wakeup),
    };
    (
        Box::new(HostTransport {
            effects,
            metrics: consumer.metrics_handle(),
            leases: Default::default(),
        }),
        consumer,
        receive,
        Pipeline::host(feed),
    )
}

/// Checked host methods keep byte admission distinct from input sent to the source.
impl Surface {
    /// Accepts a prefix into the fixed 4-MiB pool. Never blocks for a worker.
    /// Retry the remaining suffix after wakeup/tick; zero means no credit now.
    /// Only TransportMode::Host surfaces support this operation.
    pub fn feed_output(&self, bytes: &[u8]) -> Result<usize, ApiError> {
        self.check()?;
        self.host_feed()?.output(bytes)
    }
    /// Returns program-reply credit only after the entire callback payload was
    /// consumed by the host's writer, in FIFO order. Queueing a write is not
    /// completion. Invalid/duplicate IDs are rejected on the Session owner.
    /// Calls are nonblocking: scalar validation happens on the owning Session.
    /// No UI callback may synchronously query or wait for that owner.
    /// The transport hook releases its retained lease only after FIFO validation.
    pub fn reply_complete(&self, id: u64) -> Result<(), ApiError> {
        self.check()?;
        self.host_feed()?;
        if id == 0 {
            return Err(ApiError::InvalidArgument("reply ID is zero"));
        }
        self.send(puzzle_term::SessionMessage::ReplyCompleted(id))
    }

    /// Reports source completion after every output suffix has been accepted.
    /// All queued bytes are parsed before exit policy runs and SessionExited arrives.
    pub fn exited(&self, info: ExitInfo) -> Result<(), ApiError> {
        self.check()?;
        self.host_feed()?.exited(info)
    }
    /// A PTY source cannot be silently mixed with host-fed bytes.
    fn host_feed(&self) -> Result<&Feed, ApiError> {
        self.feed
            .as_ref()
            .ok_or(ApiError::InvalidArgument("surface owns a PTY"))
    }
}
