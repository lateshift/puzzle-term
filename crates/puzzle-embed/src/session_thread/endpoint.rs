//! Nonblocking desktop messages and a render-owned bounded snapshot wait (D-50).
//!
//! Handle contains senders and scalar counters only, never a Session pointer.
//! Frames owns the publication receiver on the frame-building thread. Its wait
//! ends at the caller's deadline; an unanswered request cannot stall other work.
//! Draining observes every reply before choosing its content: registrations and
//! timing evidence belong to their answer even when another dirty slot replaces it.
//! NothingNew returns capacity but cannot cancel an older not-yet-drawn publication.
//! Lifetime cancellation posts a data wake only when the last sending capability
//! disappears; neither an idle read nor a full pool can strand host shutdown.
//! Instrumented host messages carry separate arrival and send stamps. This lets
//! the actual writer endpoint include native adapter work in key latency while
//! control residence still starts at channel submission. CPU timestamps never
//! travel from a host thread: only the owner starts that interval at application.
//! Writer completions carry values on the priority inbox, never Session borrows.
//! Their weak endpoint cannot keep a closed session alive.
//! Releasing immutable storage sends readiness separately from byte credit.
//! Close uses the same inbox so a non-reading peer cannot prevent teardown.
use super::*;
use crate::pty_pool::DataWake;
use puzzle_term::{SessionMessage, frame_snapshot::FrameSnapshot};

/// The control inbox is independent of potentially saturated output buffers.
pub(super) enum Control {
    /// Ends the owner even while live hosts retain control senders at shutdown.
    Shutdown,
    /// Writer returns credit only after full OS consumption, or an explicit failure.
    ReplyCompletion(puzzle_term::platform::reply::ReplyCompletion),
    /// The bounded decode handoff has capacity again; retry retained image bytes.
    DecodeReady,
    /// Keys, geometry and policies retain their original sender order.
    Message(HostMessage),
    /// Native callback translation runs on the same owner as key routing.
    Input(super::input::InputMessage),
    /// View-local host points are interpreted with presented geometry on this owner.
    Pointer(crate::SurfacePointer, super::pointer::Action),
    /// Policy changes precede later raw callbacks in this control FIFO.
    InputPolicy(puzzle_term::session::OptionAsAlt),
    /// Frame requests carry rendering cache identity, never a builder loan.
    Frame(FrameRequest),
    /// Test/measurement-only queries run on the worker and return owned values.
    #[cfg(any(test, feature = "lock-metrics", feature = "test-support"))]
    Probe(super::probe::Request),
}

/// Host arrival and channel-send clocks remain separate from owner CPU time.
pub(super) struct HostMessage {
    /// The source owner applies this typed value without a callback into its sender.
    pub(super) value: SessionMessage,
    /// Optional per-input clock values never affect control ordering or policy.
    /// None retains the original uninstrumented path. This metadata describes
    /// host work only; it cannot change the semantic message or its priority.
    pub(super) timing: Option<crate::InputTiming>,
    /// Channel residence begins after native adapter work, on the sending endpoint.
    #[cfg(feature = "lock-metrics")]
    pub(super) sent: Instant,
    /// Key latency may begin earlier, at the real RawInput callback.
    #[cfg(feature = "lock-metrics")]
    pub(super) arrival: Instant,
}

/// Cache values and refresh time needed to publish a sealed snapshot.
pub(super) struct FrameRequest {
    /// Correlation distinguishes an older late answer from the current display turn.
    pub(super) id: u64,
    /// Sender timestamp measures total request latency independently of display time.
    pub(super) sent: Instant,
    /// The rendering owner bounds its own wait with this same deadline.
    pub(super) deadline: Instant,
    /// CPU timing starts on the Session owner when this control is applied.
    pub(super) applied: Option<(Instant, Option<std::time::Duration>)>,
    /// The injected time advances Session-owned timers at the requested display turn.
    pub(super) now: Instant,
    /// Context incarnation invalidates every rotating slot after glyph-store resets.
    pub(super) atlas_generation: u64,
    /// Rendering's independent style epoch participates in copy invalidation.
    pub(super) style_epoch: u64,
}

/// Cloneable sending authority, with no synchronous callback into Session.
#[derive(Clone)]
pub struct Handle {
    /// Unbounded controls cannot block native input behind drain backpressure.
    control: Arc<mpsc::Sender<Control>>,
    /// The last sender drop wakes an owner blocked on the still-open drain inbox.
    _lifetime: Arc<WakeOnDrop>,
    /// Unique requests permit an ordered final geometry barrier in finite probes.
    requests: Arc<AtomicU64>,
    /// A full data inbox is already awake; posting the extra token never blocks.
    wake: DataWake,
    /// Exact parser progress is observational, not a query of live terminal state.
    parsed: Arc<AtomicU64>,
    /// Fixed-pool backlog remains within its original four-MiB allocation bound.
    pending: Arc<AtomicUsize>,
    /// These atomic observations cannot reach mutable Session or drawing state.
    #[cfg(feature = "lock-metrics")]
    pipeline: Arc<crate::pipeline_metrics::Metrics>,
}

/// Message send always precedes its corresponding idle wake token.
impl Handle {
    /// Cancels the owner before the host joins its render thread (lifecycle order).
    pub fn shutdown(&self) {
        // The data wake covers an idle PTY; the control itself covers held EOF.
        // Retained rendering/UI handles cannot keep a cancelled Session alive.
        let _ = self.post(Control::Shutdown);
    }
    /// Connects sending capabilities after all receiving owners have been constructed.
    pub(super) fn new(
        control: mpsc::Sender<Control>,
        data: &Consumer,
        parsed: Arc<AtomicU64>,
    ) -> Self {
        // Derive observations and wake authority from the same drain endpoint.
        let wake = data.wake_handle();
        Self {
            control: Arc::new(control),
            _lifetime: Arc::new(WakeOnDrop(wake.clone())),
            requests: Arc::new(AtomicU64::new(0)),
            wake,
            parsed,
            pending: data.pending_handle(),
            #[cfg(feature = "lock-metrics")]
            pipeline: data.metrics_handle(),
        }
    }

    /// A weak mailbox cannot keep its own Session alive during shutdown.
    pub(super) fn reply_completion(
        &self,
    ) -> Box<dyn Fn(puzzle_term::platform::reply::ReplyCompletion) + Send + Sync> {
        let control = Arc::downgrade(&self.control);
        let wake = self.wake.clone();
        Box::new(move |completion| {
            if let Some(control) = control.upgrade() {
                let _ = control.send(Control::ReplyCompletion(completion));
                wake.notify();
            }
        })
    }

    /// Queues host input without acquiring a parser or frame-builder guard.
    pub fn send(&self, message: SessionMessage) -> Result<(), String> {
        #[cfg(feature = "lock-metrics")]
        let sent = Instant::now();
        self.post(Control::Message(HostMessage {
            value: message,
            timing: crate::InputTiming::capture().map(|mut timing| {
                timing.sent = Instant::now();
                timing
            }),
            #[cfg(feature = "lock-metrics")]
            sent,
            #[cfg(feature = "lock-metrics")]
            arrival: sent,
        }))
    }

    /// Includes native adapter work without calling a clock on the Session owner's behalf.
    #[cfg(feature = "lock-metrics")]
    pub fn send_arrived(&self, message: SessionMessage, arrival: Instant) -> Result<(), String> {
        self.post(Control::Message(HostMessage {
            value: message,
            timing: crate::InputTiming::capture().map(|mut timing| {
                // The supplied arrival precedes this API, not merely this send.
                // Preserve it in the separate host-residence portion of the trace.
                timing.residence = Some(timing.api.saturating_duration_since(arrival));
                timing.sent = Instant::now();
                timing
            }),
            sent: Instant::now(),
            arrival,
        }))
    }

    /// Requests a late snapshot using rendering's current cache identities.
    pub fn request_frame(
        &self,
        now: Instant,
        generation: u64,
        epoch: u64,
        deadline: Instant,
    ) -> Result<u64, String> {
        let id = self.requests.fetch_add(1, Ordering::Relaxed);
        self.post(Control::Frame(FrameRequest {
            id,
            sent: Instant::now(),
            deadline,
            applied: None,
            now,
            atlas_generation: generation,
            style_epoch: epoch,
        }))?;
        Ok(id)
    }

    /// Posting never runs core policy on the sender's thread.
    pub(super) fn post(&self, control: Control) -> Result<(), String> {
        self.control.send(control).map_err(|e| e.to_string())?;
        self.wake.notify();
        Ok(())
    }

    /// Reads cumulative throughput without requesting a turn on the owner.
    pub fn parsed_total(&self) -> u64 {
        self.parsed.load(Ordering::Relaxed)
    }
    /// Backpressure counts bytes, not channel tokens or snapshot owners.
    pub fn pending_bytes(&self) -> usize {
        self.pending.load(Ordering::Acquire)
    }
}

/// A separate lifetime token runs exactly once, avoiding a racy reference-count test.
struct WakeOnDrop(DataWake);

impl Drop for WakeOnDrop {
    /// Handle drops its control sender before this token, so the wake reveals EOF.
    fn drop(&mut self) {
        // The drain may still be alive; releasing every host capability must
        // nevertheless wake its idle Session owner to observe control disconnection.
        self.0.notify();
    }
}

/// One allocation moves with its sealed row identities, whether dirty or unchanged.
pub struct FrameReply {
    /// The request that produced this answer, including NothingNew replies.
    pub id: u64,
    /// Owner clocks distinguish queued delay from actual publication work.
    pub timing: super::PublicationTiming,
    /// Sealed grid dimensions also describe an unchanged reply with a fresh slot.
    pub grid: (usize, usize),
    /// Only the receiving renderer may mutate this slot until it returns it.
    pub slot: FrameSnapshot,
    /// An unchanged answer must not be counted as a GPU presentation.
    pub published: bool,
    /// Recording pairs this offset with the rows captured on the same owner turn.
    pub parsed: u64,
}

/// View ownership can change only in a sealed publication, not an idle acknowledgement.
impl FrameReply {
    /// Receive one-time drawing registrations before returning a superseded slot.
    pub(crate) fn accept_views(&mut self, builder: &mut puzzle_term::frame_builder::FrameBuilder) {
        // NothingNew did not capture this slot: it may be fresh storage or an
        // old registration set. Only Published can authoritatively remove views.
        // A real publication still transfers views even if it will not be drawn.
        if self.published {
            builder.accept_snapshot_views(&mut self.slot);
        }
    }
}

/// Rendering owns its receiver exclusively; no receiver lock is required.
pub struct Frames {
    /// Slots return by move without awaiting Session or another surface.
    free: mpsc::Sender<FrameSnapshot>,
    /// The session owner publishes completed immutable source values.
    published: Receiver<FrameReply>,
}
impl Frames {
    /// Allocates exactly three slots before either owner can begin a publication.
    pub(super) fn channels()
    -> Result<(Self, Receiver<FrameSnapshot>, mpsc::Sender<FrameReply>), String> {
        // Each direction has one receiver: no mutex or shared receiver loan is needed.
        let (free_tx, free_rx) = mpsc::channel();
        let (published_tx, published_rx) = mpsc::channel();
        // Seeding precedes worker startup, so a first frame never waits for the renderer.
        for _ in 0..puzzle_term::frame_snapshot::PUBLICATION_SLOTS {
            free_tx
                .send(FrameSnapshot::default())
                .map_err(|e| e.to_string())?;
        }
        Ok((
            Self {
                free: free_tx,
                published: published_rx,
            },
            free_rx,
            published_tx,
        ))
    }
    /// Waits only within rendering's own remaining build opportunity.
    pub fn receive_until(&self, deadline: Instant) -> Option<FrameReply> {
        // Expired deadlines still accept an already published value without sleeping.
        self.published
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok()
    }
    /// Drains obsolete answers before building, adopting their view registrations first.
    pub fn try_receive(&self) -> Option<FrameReply> {
        self.published.try_recv().ok()
    }
    /// Drains a bounded opportunity, preserving dirty ink across NothingNew answers.
    /// `observe` adopts one-time views and records every answer before recycling.
    pub fn newest_until(
        &self,
        request_id: u64,
        deadline: Instant,
        observe: impl FnMut(&mut FrameReply),
    ) -> (Option<FrameReply>, bool) {
        // Receipt distinguishes a current answer from a queued late publication;
        // both are observed, but only the former ends this deadline's wait.
        select_replies(
            request_id,
            |wait| {
                if wait {
                    self.receive_until(deadline)
                } else {
                    // Once answered, only already queued replies may replace ink.
                    self.try_receive()
                }
            },
            |slot| self.recycle(slot),
            observe,
        )
    }
    /// A returned slot keeps its damage history for its next owner-side capture.
    pub fn recycle(&self, slot: FrameSnapshot) {
        let _ = self.free.send(slot);
    }
}

/// Selects complete ink while keeping receipt and clocks injectable for D-50 tests.
/// The receiver's bounded wait is owned by rendering, never by the Session owner.
pub(super) fn select_replies(
    request_id: u64,
    mut receive: impl FnMut(bool) -> Option<FrameReply>,
    mut recycle: impl FnMut(FrameSnapshot),
    mut observe: impl FnMut(&mut FrameReply),
) -> (Option<FrameReply>, bool) {
    let mut newest: Option<FrameReply> = None;
    let mut answered = false;
    // A ready reply from the previous refresh is not the answer just requested.
    // Keep the caller's bounded opportunity open until that request is answered.
    let mut current_answered = false;
    let mut reply = receive(true);
    while let Some(mut answer) = reply {
        answered = true;
        current_answered |= answer.id >= request_id;
        // Registrations and timing belong to every reply, even obsolete ones.
        observe(&mut answer);
        if answer.published {
            if let Some(old) = newest.replace(answer) {
                recycle(old.slot);
            }
        } else {
            // NothingNew cannot erase dirty ink left over from a missed refresh.
            recycle(answer.slot);
        }
        reply = receive(!current_answered);
    }
    (newest, answered)
}

impl Handle {
    /// Diagnostic serialization reads atomic observations, never a Session query.
    #[cfg(feature = "lock-metrics")]
    pub fn stage_report(&self) -> serde_json::Value {
        // Diagnostic counters describe platform observations only. The owned Session
        // is never queried, even while a renderer formats a completed cohort.
        self.pipeline.report()
    }
}

/// Decoder routes are weak: a closed surface cannot be retained by an App worker.
#[derive(Clone)]
pub(crate) struct DecodeEndpoint {
    /// Weak sender does not participate in the host endpoint's lifetime.
    control: std::sync::Weak<mpsc::Sender<Control>>,
    /// A posted result wakes both data-idle and held-screen owner waits.
    wake: DataWake,
}

impl DecodeEndpoint {
    /// Completion enters the same priority inbox as other platform results.
    pub(crate) fn complete(&self, result: puzzle_term::images::DecodeResult) {
        // Upgrade only for the send itself. A queued decode cannot keep a closed
        // host endpoint alive while its owning Session is trying to shut down.
        if let Some(control) = self.control.upgrade() {
            let message = HostMessage {
                value: SessionMessage::ImageDecoded(result),
                // Decoder completion is not user input and must never arm a key clock.
                timing: None,
                #[cfg(feature = "lock-metrics")]
                sent: Instant::now(),
                #[cfg(feature = "lock-metrics")]
                arrival: Instant::now(),
            };
            let _ = control.send(Control::Message(message));
            self.wake.notify();
        }
    }

    /// Capacity changes wake an owner that retained an unsubmitted request.
    pub(crate) fn ready(&self) {
        if let Some(control) = self.control.upgrade() {
            let _ = control.send(Control::DecodeReady);
            self.wake.notify();
        }
    }
}

impl Handle {
    /// One weak completion route can travel with a bounded decoder job.
    pub(crate) fn decode_endpoint(&self) -> DecodeEndpoint {
        DecodeEndpoint {
            control: Arc::downgrade(&self.control),
            wake: self.wake.clone(),
        }
    }
}
