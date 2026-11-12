//! Loaded font work uses browser tasks and the same admission endpoint as fallback.
//!
//! Rust-only ownership keeps the neutral driver Send. Parsing and cold hint
//! setup happen between paints; a result event wakes even a frozen screen.
//! Result backpressure ends the task chain until publication frees a slot.
use puzzle_term::text::preparation::{Completion, Driver, Ink, Job, faces::Faces};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

/// Parsed faces and bounded lanes belong to this shared text resource.
struct Work {
    /// Metadata is retained across jobs without duplicating font bytes.
    faces: Faces,
    /// Accepted work owns immutable bytes and exact placement geometry.
    jobs: Receiver<Job>,
    /// Completed geometry remains owned until the frame admits publication.
    results: SyncSender<Completion>,
    /// One completed result can wait for a full result lane without polling.
    pending: Option<Completion>,
    /// Recent whole-call p95 bounds the next indivisible background task unit.
    unit: puzzle_term::text::preparation::cost::RecentCost,
    /// Refresh-derived time allowance bounds a browser task's preparation turn.
    quantum: Duration,
}
/// Only nonblocking frame entry points cross the platform-neutral seam.
struct Endpoint {
    /// Parsed ownership is lent to inline work only through try_lock.
    work: Arc<Mutex<Work>>,
    /// Eight results match the platform source lanes; no throughput-tuned batch.
    results: Receiver<Completion>,
    /// A readiness peek cannot acknowledge a pending key.
    peeked: Option<Completion>,
    /// A full mailbox leaves the glyph retryable after existing work completes.
    jobs: SyncSender<Job>,
    /// One event-loop task chain owns a resource until idle or backpressure.
    armed: Arc<AtomicBool>,
    /// This private event wakes only sessions following this text resource.
    wake: String,
    /// Accepted jobs remain pending until their owned results are consumed.
    outstanding: usize,
    /// Absolute deadline includes intervening row/packing work.
    deadline: Option<web_time::Instant>,
    /// Inline time is removed from ordinary frame cost prediction.
    frame_time: Duration,
}
/// Creates an idle owner without parsing or preparing a font at installation.
pub(in crate::runtime) fn driver(wake: &str) -> Box<dyn Driver> {
    // Bound accepted ownership independently of the number of visible glyphs.
    let (jobs, input) = mpsc::sync_channel(128);
    // Result backpressure bounds prepared geometry when a terminal stops presenting.
    let (output, results) = mpsc::sync_channel(8);
    Box::new(Endpoint {
        work: Arc::new(Mutex::new(Work {
            // Parsed metadata is lazy; installing a text context opens no font tables.
            faces: Faces::default(),
            jobs: input,
            results: output,
            // One task-owned result can wait for lane capacity without preparing it twice.
            pending: None,
            unit: Default::default(),
            quantum: Duration::ZERO,
        })),
        results,
        peeked: None,
        jobs,
        armed: Arc::new(AtomicBool::new(false)),
        wake: wake.into(),
        outstanding: 0,
        // An inline call is disabled until the host has offered refresh slack.
        deadline: None,
        frame_time: Duration::ZERO,
    })
}
impl Driver for Endpoint {
    /// The host already reserved ordinary frame work and D-33's wake margin.
    fn begin_frame(&mut self, allowance: Duration) {
        // Absolute allowance includes time spent on earlier rows and atlas publication.
        self.deadline = Some(web_time::Instant::now() + allowance);
        self.frame_time = Duration::ZERO;
        // A busy task cannot make the rAF callback wait for parsed ownership.
        if let Ok(mut work) = self.work.try_lock() {
            // Background turns inherit measured host slack rather than a glyph batch count.
            work.quantum = allowance;
        }
    }
    /// An unopened face or cold hinter never starts on the frame path.
    fn prepare_now(&mut self, job: &Job) -> Option<Result<Option<Ink>, String>> {
        // An unopened frame admission phase always uses the background lane.
        let deadline = self.deadline?;
        // Frames borrow ready state nonblockingly; cold work remains task-owned.
        let mut work = self.work.try_lock().ok()?;
        // Variation, ppem and hinter readiness are checked before any preparation.
        let predicted = work.faces.predicted(job)?;
        if !puzzle_term::text::preparation::fits(
            deadline.saturating_duration_since(web_time::Instant::now()),
            predicted,
        ) {
            return None;
        }
        let started = web_time::Instant::now();
        // Owned prepared geometry is reused by drawing instead of discarded validation.
        let ink = work.faces.prepare(job);
        let cost = started.elapsed();
        // Reusable-call prediction excludes table parsing performed on cold task startup.
        work.faces.record(job, cost);
        // The host removes actual admission work from ordinary-frame cost samples.
        self.frame_time += cost;
        Some(ink)
    }
    /// Queue ownership, not a periodic timer, starts cold work.
    fn submit(&mut self, job: Job) -> bool {
        // A full queue declines promptly, keeping the glyph retryable.
        let accepted = self.jobs.try_send(job).is_ok();
        // Only accepted jobs can later deliver a matching completion.
        self.outstanding += usize::from(accepted);
        // Refused ownership cannot justify arming a task chain.
        if accepted {
            self.arm();
        }
        accepted
    }
    /// Publication acknowledgment resumes a task paused by result backpressure.
    fn poll(&mut self) -> Option<Completion> {
        // Readiness peeks preserve result ordering and do not acknowledge a glyph key.
        let result = self.peeked.take().or_else(|| self.results.try_recv().ok());
        if result.is_some() {
            self.outstanding = self.outstanding.saturating_sub(1);
            // Consuming a result resumes tasks previously paused by lane backpressure.
            self.arm();
        }
        result
    }
    /// A frozen screen requests a frame only after geometry is available.
    fn ready(&mut self) -> bool {
        // Repeated readiness checks retain exactly one completion without draining the lane.
        if self.peeked.is_none() {
            self.peeked = self.results.try_recv().ok();
        }
        self.peeked.is_some()
    }
    /// Explicit evidence tools may await accepted ownership outside frames.
    fn pending(&self) -> bool {
        // Queued and task-owned work stays pending until publication takes its result.
        self.outstanding != 0
    }
    /// Concurrent task work is excluded from this frame's inline sample.
    fn frame_time(&self) -> Duration {
        self.frame_time
    }
}
impl Endpoint {
    /// No timer survives an empty mailbox or a saturated result lane (P2).
    fn arm(&self) {
        // Several cold glyphs share one task chain rather than arming one timer per cell.
        if self.armed.swap(true, Ordering::Relaxed) {
            return;
        }
        let work = self.work.clone();
        let armed = self.armed.clone();
        let wake = self.wake.clone();
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                // A macrotask boundary gives browser input and paint a chance to run.
                super::idle::yield_task().await;
                let (more, published) = work.lock().map_or((false, false), |mut work| work.turn());
                // Release task ownership before synchronous wake dispatch permits resumption.
                if !more {
                    armed.store(false, Ordering::Relaxed);
                }
                // Only newly available geometry wakes a frozen screen.
                if published {
                    super::idle::notify(&wake);
                }
                // An empty or blocked task chain leaves no periodic polling behind.
                if !more {
                    break;
                }
            }
        });
    }
}
impl Work {
    /// Time limits a turn; zero frame slack still permits worker progress off-frame.
    fn turn(&mut self) -> (bool, bool) {
        let started = web_time::Instant::now();
        // The return value distinguishes a ready-result wake from mere task activity.
        let mut published = false;
        loop {
            // Publish the previous geometry before allocating another prepared result.
            if let Some(done) = self.pending.take() {
                match self.results.try_send(done) {
                    Ok(()) => published = true,
                    Err(mpsc::TrySendError::Full(done)) => {
                        // A full lane preserves singular ownership until publication frees a slot.
                        self.pending = Some(done);
                        return (false, published);
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => return (false, published),
                }
            }
            // A worker must make progress when frame admission is zero. After one
            // indivisible call, the measured quantum determines whether another fits.
            if published && started.elapsed() + self.unit.p95().unwrap_or_default() >= self.quantum
            {
                return (true, published);
            }
            // An empty mailbox ends work immediately rather than waiting inside the event loop.
            let Ok(job) = self.jobs.try_recv() else {
                return (false, published);
            };
            // Cold parsing belongs to this task and never poisons reusable-call prediction.
            // All first-use parsing and hinter setup stay outside browser frames.
            let loaded = self.faces.load(&job);
            let call = web_time::Instant::now();
            let result = loaded.and_then(|()| self.faces.prepare(&job));
            let cost = call.elapsed();
            self.faces.record(&job, cost);
            self.unit.record(cost);
            // Completion carries the original placement/style together with owned geometry.
            self.pending = Some(Completion {
                placement: job.placement,
                style: job.style,
                result,
            });
        }
    }
}
