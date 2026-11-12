//! Desktop D-50 owner loop: Session moves here by value and never leaves during a run.
//!
//! Control and drain inboxes are distinct. Every control is applied before the
//! next at-most-16-KiB slice, even if the drain has filled its four-MiB pool.
//! A control send also posts a nonblocking wake to that pool's data inbox.
//! Idle waiting owns no Session guard, renderer resource or native window.
//! Three snapshot allocations circulate by value; the owner never waits for a
//! slot. A request without a free slot remains pending until a later request.
//! NothingNew answers still return their slot, preserving the fixed allocation count.
//! Failed receiver lifetime ends the worker rather than retaining unreachable VT.
//! Request clocks partition control-queue delay and owner publication work; only
//! the latter has a meaningful thread-CPU interval on this worker.
//! Recording samples parsed offsets here, where source rows cannot change concurrently.
//! A held exit sleeps on controls after data EOF, so search and close remain usable.
//! Writer observers measure the transport endpoint; a swallowed key clears its stamp.
//! Shutdown order: the host posts explicit Shutdown before joining rendering,
//! then releases its native window and joins this owner. Shutdown wakes both
//! data-idle and held-EOF waits even while the host retains sending handles.
use crate::pty_pool::Consumer;
use puzzle_term::{HostEvent, PublishOutcome, Session};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::Instant,
};

/// Sending controls and circulating slots grants no access to the owned Session.
mod endpoint;
/// Main-thread result delivery has a separate coalescing wake from frame damage.
mod events;
/// Existing feed attribution follows the sole owner without a Session guard.
mod feeding;
/// Geometry coalesces before reflow while every input retains its FIFO boundary.
mod geometry;
/// Bounded decode handoff and ordered EOF completion.
mod images;
/// Raw host callback translation shares the Session owner, never a host-side adapter.
mod input;
/// Host points are resolved against the sealed geometry returned after presentation.
pub(crate) mod pointer;
/// Writer credit waits and completions use the priority control inbox.
mod replies;
pub(crate) use endpoint::DecodeEndpoint;
use endpoint::{Control, FrameRequest};
pub use endpoint::{FrameReply, Frames, Handle};
pub use events::Events;
/// Explicit diagnostic request/reply is unavailable in ordinary live builds.
#[cfg(any(test, feature = "lock-metrics", feature = "test-support"))]
mod probe;
/// Owned request/answer timestamps never compare CPU clocks from different threads.
mod timing;
pub use timing::PublicationTiming;

/// Platform lifecycle and notification capabilities, all installed before ownership moves.
#[derive(Default)]
pub struct Config {
    /// Presence keeps the owner alive after drain EOF for held exits and search input.
    pub exit: Option<Receiver<puzzle_term::messages::ExitInfo>>,
    /// Results post one event-loop wake after their values have been queued.
    pub notify: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Private parser boundaries remain ordered on the sole Session owner.
    pub recorder: Option<crate::recording::Recorder>,
    /// App-owned bounded image decoder; absent in parser-only diagnostics.
    pub decoder: Option<crate::image_decode::worker::Sender>,
}

/// Session-thread state is intentionally not shared with either host owner.
struct Owner {
    /// VT, interaction models, policy and ordered writer have exactly one owner.
    session: Session,
    /// Weak completion route never holds its own control channel open.
    decode_reply: DecodeEndpoint,
    /// No decode or decompression can run on this latency-critical owner.
    decoder: Option<crate::image_decode::worker::Sender>,
    /// Suppress duplicate queue registration until an explicit readiness wake.
    decode_blocked: bool,
    /// Modifiers and composition belong to one ordered native callback stream.
    input: crate::keyboard::Adapter,
    /// Keys and frame requests cannot queue behind drain payloads.
    control: Receiver<Control>,
    /// The fixed pool retains partial slices without copying their unread suffix.
    data: Consumer,
    /// Slots returned by rendering retain their individual row-copy histories.
    free: Receiver<puzzle_term::frame_snapshot::FrameSnapshot>,
    /// Publication moves ownership; the receiver never needs a Session reference.
    published: mpsc::Sender<FrameReply>,
    /// An unchanged synchronized turn keeps one free slot until a safe boundary.
    deferred_slot: Option<puzzle_term::frame_snapshot::FrameSnapshot>,
    /// Host facts are deferred values; foreign UI callbacks run elsewhere.
    events: mpsc::Sender<HostEvent>,
    /// Latest request replaces a superseded refresh, never a queued key.
    frame: Option<FrameRequest>,
    /// A byte counter permits throughput reporting without a synchronous query.
    parsed: Arc<AtomicU64>,
    /// Native child status is applied only after every accepted output byte is parsed.
    exit: Option<Receiver<puzzle_term::messages::ExitInfo>>,
    /// A held child retains Session until explicit shutdown or the last host sender drops.
    live: bool,
    /// Control-channel disconnection ends ownership without a synchronous host call.
    stopping: bool,
    /// A toolkit callback observes only completed queued host results.
    signal: events::Signal,
    /// Recording is absent in ordinary sessions and never exposes a core reference.
    recorder: Option<crate::recording::Recorder>,
    /// Pool, drain and feed share only fixed diagnostic counters.
    pipeline: Arc<crate::pipeline_metrics::Metrics>,
}

/// Independent endpoint owners returned after Session moves to its worker.
pub struct Started {
    /// Input senders can move to native callbacks without sharing VT state.
    pub handle: Handle,
    /// Snapshot receiver remains exclusively on the frame-building thread.
    pub frames: Frames,
    /// Results are consumed on the host thread, never on the Session owner.
    pub events: Events,
    /// Finite harness runs join after draining all accepted bytes.
    pub worker: thread::JoinHandle<Result<(), String>>,
}

/// Transfers Session to its drain-backed owner; optional status preserves held exits.
pub fn start(mut session: Session, data: Consumer, config: Config) -> Result<Started, String> {
    // Observation starts before the worker can feed output. A title emitted in
    // the first drain chunk must not disappear before the host attaches its inbox.
    session.enable_host_events()?;
    // Control carries values only; no request can borrow Session or the builder.
    // Its queue is separate from bounded bytes, retaining input priority under flood.
    let (control_tx, control) = mpsc::channel();
    // Opposite channel directions express slot ownership in the type system:
    // the worker receives free slots and the rendering owner receives publications.
    let (frames, free, published) = Frames::channels()?;
    // Foreign UI effects leave after the mutable core turn, never as callbacks
    // into main/render while this worker is parsing or copying visible rows.
    let (events, events_rx) = mpsc::channel();
    let (events_rx, signal) = Events::pair(events_rx, config.notify);
    // Only this platform counter is shared. Core Session and VT contain no
    // atomics, lock guards or knowledge of the host's synchronization strategy.
    let parsed = Arc::new(AtomicU64::new(session.parsed_bytes_total()));
    let pipeline = data.metrics_handle();
    let handle = Handle::new(control_tx, &data, parsed.clone());
    // The closure takes Session by move exactly once. It never returns a
    // reference, and its join result contains only completion or an error.
    let decode_reply = handle.decode_endpoint();
    // Bind before any reply is admitted; writer completion always has a wake route.
    session.set_reply_completion(handle.reply_completion());
    let worker = thread::spawn(move || {
        Owner {
            session,
            decode_reply,
            decoder: config.decoder,
            decode_blocked: false,
            input: crate::keyboard::Adapter::default(),
            control,
            data,
            free,
            published,
            deferred_slot: None,
            events,
            frame: None,
            parsed,
            live: config.exit.is_some(),
            exit: config.exit,
            stopping: false,
            signal,
            recorder: config.recorder,
            pipeline,
        }
        .run()
    });
    Ok(Started {
        handle,
        frames,
        events: events_rx,
        worker,
    })
}

/// Only this thread invokes the core while the renderer builds earlier publications.
impl Owner {
    /// Drains priority input before every bounded VT call; idle is one blocking wait.
    fn run(&mut self) -> Result<(), String> {
        loop {
            // Control draining precedes bytes, so a saturated pool cannot starve a key.
            self.controls()?;
            // Dropped host capabilities cancel ownership even if the PTY is still open.
            if self.stopping {
                return Ok(());
            }
            // Publications have input priority too; glyph work never runs on this owner.
            self.publish()?;
            if self.wait_reply_credit() || self.wait_image_decode()? {
                continue;
            }
            if self.drain_pending() {
                continue;
            }
            // Session may retain a suffix only through its existing parse-budget rule.
            // The pool slice itself stays at 16 KiB; no performance-tuned limit is added.
            if self.feed_next()? {
                continue;
            }
            self.deliver();
            if self.data.closed() {
                if !self.live {
                    // Finite harnesses finish at drained EOF, independently of host lifetime.
                    return Ok(());
                }
                // A live child may hold its final screen. Its owner sleeps on
                // controls after EOF rather than polling a permanently closed data inbox.
                self.finish_exit()?;
                self.deliver();
                // After EOF only host input can change a held screen; recv is idle-free.
                match self.control.recv() {
                    Ok(control) => self.apply_control(control),
                    Err(_) => return Ok(()),
                }
                continue;
            }
            // Polling the data inbox may itself consume a control wake. Recheck
            // the priority inbox before blocking, otherwise that already-consumed
            // token could strand an idle key until an unrelated PTY read arrives.
            if self.controls()? {
                continue;
            }
            if self.stopping {
                return Ok(());
            }
            // A control posted after this recheck leaves a token for the blocking wait.
            self.data.wait_data();
        }
    }

    /// Applies priority controls and reports whether an idle wait must be reconsidered.
    fn controls(&mut self) -> Result<bool, String> {
        // An idle loop must distinguish an empty poll from newly handled input.
        // Receiving a frame request similarly requires a publication turn before sleep.
        let mut received = false;
        let mut geometry = geometry::Batch::default();
        loop {
            let control = match self.control.try_recv() {
                Ok(control) => control,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    // No sender remains to use this Session or request another publication.
                    self.stopping = true;
                    break;
                }
            };
            // All messages precede the next parse slice, including controls
            // posted while an earlier data poll consumed their notification token.
            received = true;
            match control {
                Control::Message(message) => {
                    if let Some(message) = geometry.push(message) {
                        geometry.flush(|value| self.apply_control(Control::Message(value)));
                        self.apply_control(Control::Message(message));
                    }
                }
                Control::Frame(_) => self.apply_control(control),
                other => {
                    geometry.flush(|value| self.apply_control(Control::Message(value)));
                    self.apply_control(other);
                }
            }
            // Cancellation has priority over queued probes or frame publications.
            if self.stopping {
                break;
            }
        }
        geometry.flush(|value| self.apply_control(Control::Message(value)));
        Ok(received)
    }

    /// Answers one refresh without waiting for the rendering owner to return a slot.
    fn publish(&mut self) -> Result<(), String> {
        let Some(request) = self.frame.take() else {
            return Ok(());
        };
        let Some(mut slot) = self
            .deferred_slot
            .take()
            .or_else(|| self.free.try_recv().ok())
        else {
            // No thread holds another's required slot; retain the newest request instead.
            self.frame = Some(request);
            return Ok(());
        };
        let outcome = self.session.publish_frame(
            request.now,
            &mut slot,
            request.atlas_generation,
            request.style_epoch,
        )?;
        // Read both clocks before sending; formatting or later observer work cannot
        // inflate the measured owner interval or masquerade as publication work.
        let timing = PublicationTiming::completed(&request);
        tracing::trace!(target: "puzzle_desktop::scheduling", event="desktop.owner_publication",
            id=request.id, ?outcome,
            synchronized=self.session.terminal().modes.get(true, 2026).unwrap_or(false));
        // Publication can drain an older budgeted suffix. Its source offset
        // must include that consumption before the sealed slot leaves this owner.
        self.parsed
            .store(self.session.parsed_bytes_total(), Ordering::Relaxed);
        if outcome == PublishOutcome::NothingNew
            && self.session.terminal().modes.get(true, 2026) == Some(true)
        {
            // The request is still outstanding, not an idle answer. Yield the
            // next bounded input slice at its semantic transaction end, then
            // publish before consuming the following transaction. Returning the
            // unused slot preserves all three allocations without a blocking wait.
            self.frame = Some(request);
            // Only this worker receives free slots; returning through the existing
            // channel would require a second sender, so retain it with the request.
            self.deferred_slot = Some(slot);
            return Ok(());
        }
        // NothingNew returns its allocation too, without manufacturing a presentation.
        let reply = FrameReply {
            id: request.id,
            timing,
            grid: self.session.terminal().size(),
            slot,
            published: outcome == PublishOutcome::Published,
            parsed: self.parsed.load(Ordering::Relaxed),
        };
        // The slot moves with its reply; failed receiver lifetime ends the owner
        // rather than leaking an allocation or keeping an unreachable child alive.
        self.published.send(reply).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Results move out in producer order after each core operation has completed.
    fn deliver(&mut self) {
        let mut delivered = false;
        while let Some(event) = self.session.take_event() {
            // Dropping an observational receiver cannot corrupt bytes or stop VT parsing.
            delivered |= self.events.send(event).is_ok();
        }
        // Post after values exist: an immediately running UI can safely drain them.
        // The notification latch spans this whole batch rather than one fact.
        if delivered {
            self.signal.delivered();
        }
    }

    /// A malformed host mutation is recoverable; it cannot kill the output owner (R8).
    fn apply_control(&mut self, control: Control) {
        match control {
            Control::Shutdown => self.stopping = true,
            Control::ReplyCompletion(completion) => self.apply_reply_completion(completion),
            Control::DecodeReady => self.decode_blocked = false,
            Control::Input(input) => self.apply_input(input),
            Control::Pointer(input, action) => self.apply_pointer(input, action),
            Control::InputPolicy(policy) => self.input.set_option_as_alt(policy),
            #[cfg(any(test, feature = "lock-metrics", feature = "test-support"))]
            Control::Probe(operation) => operation(&mut self.session),
            Control::Message(message) => {
                // Clock work is opt-in; every path still applies the same owner message.
                let key = matches!(
                    &message.value,
                    puzzle_term::SessionMessage::NativeKey(..)
                        | puzzle_term::SessionMessage::Command(
                            puzzle_term::messages::Command::Key(_)
                                | puzzle_term::messages::Command::Text(_),
                            _
                        )
                );
                if let Some(timing) = message.timing {
                    self.pipeline.control.begin_timed(timing, key);
                } else {
                    #[cfg(feature = "lock-metrics")]
                    self.pipeline
                        .control
                        .begin(message.sent, message.arrival, key);
                }
                if let puzzle_term::SessionMessage::GridResize {
                    columns,
                    rows,
                    surface,
                } = &message.value
                {
                    tracing::trace!(target:"puzzle_desktop::resize",event="embed.session_resize",columns,rows,?surface);
                }
                if let puzzle_term::SessionMessage::PtyResize(surface) = &message.value {
                    tracing::trace!(target:"puzzle_desktop::resize",event="embed.session_pty_resize",?surface);
                }
                let _stage = matches!(
                    message.value,
                    puzzle_term::SessionMessage::GridResize { .. }
                )
                .then(|| crate::resize_timing::Stage::new("session_grid_reflow"));
                if let Err(error) = self.session.apply(message.value, Instant::now()) {
                    tracing::warn!(event = "desktop.session_message_rejected", %error);
                }
                // Clear even a swallowed key before the next feed can write parser replies.
                self.pipeline.control.finish();
            }
            // A refresh may supersede a pending refresh, but never an encoded key.
            Control::Frame(mut request) => {
                // Only this worker reads its CPU clock. Queue delay stays a wall-time
                // field because a sender cannot read the owner's running CPU clock.
                request.applied = Some((Instant::now(), crate::thread_cpu::now()));
                self.frame = Some(request);
            }
        }
    }
}

#[cfg(test)]
/// Behavioral channel ownership and drain-lifecycle witnesses.
mod tests;

impl Owner {
    /// Private replay boundaries retain parsed-versus-input ordering on this owner.
    fn record_parse(&self, offered: usize) {
        if let Some(recorder) = &self.recorder
            && let Err(error) = recorder.emit(
                "parse",
                serde_json::json!({
                    "total": self.parsed.load(Ordering::Relaxed), "offered": offered
                }),
            )
        {
            tracing::warn!(event = "desktop.record_failed", %error);
        }
    }
}
