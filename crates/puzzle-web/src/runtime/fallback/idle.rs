//! Canvas discovery runs in short, explicitly scheduled event-loop tasks.
//!
//! First Canvas calls run outside rAF; completed worker draws teach later readiness.
//! Each task uses the host's measured remaining refresh slack; an
//! empty queue ends the task entirely. A later cold request arms it once.
//! The Send driver retains Rust channels/strings only, never a JS handle.
//!
//! Cold policy reservations prevent a first row from queuing redundant family discovery.
//! Only one cluster needs to complete before the same policy can reconsider its lane.
//! Script and style distinguish opaque fallback choices within the CSS family stack.
//! A larger untested pixel stage cannot borrow readiness from a smaller bitmap draw.
//! No reservation is retained when the job queue refuses ownership.
//! The next readiness wake lets declined clusters choose inline or background work.
//! Prepared results keep their placement so later cell rewrites cannot change fitting.
//! Coverage facts are permanent scalar metadata, separate from bounded ready samples.
//! Completed samples enter the raster atlas only at a frame publication boundary.
//! An eight-result lane can suspend the task chain without dropping a bitmap.
//! Consuming one result explicitly resumes that suspended chain.
//! Inline and task calls share readiness evidence but have distinct timing kinds.
//! Median cost describes the typical call while recent p95 decides admission.
//! An inline overrun revokes permission until a successful background observation.
//! The resource event name selects only sessions sharing this text context.
//! Waking does not carry glyph bytes or run a Session callback under the work mutex.
//! Normal builds retain no per-call vectors; development observers opt in first.
//! Device-sized bitmaps are decoded once and transferred with their coverage answer.
//! The task owns only Rust channels between calls, satisfying the neutral Send seam.
//! Browser Canvas handles are created and used only by the platform source operation.
//! A failed source answer still wakes rows so another fallback can be selected.
//! A successful job records readiness after its real draw rather than a font-name guess.
//! Background success and inline success cannot fabricate physical face identity.
//! Visibility affects presentation but not bounded ownership of accepted jobs.
//! Empty tasks do not wake the page or leave a recurring timer behind.
//! FIFO result order is preserved across readiness peeks and publication consumption.
//! A single indivisible Canvas call is measured even when it exceeds a task allowance.
//! No fixed glyph count determines the amount of ready ink admitted by a frame.
//! Refused requests keep a provisional box until an actual completion wake retries them.
//! Unsupported scalars retain their final diagnostic, independent of atlas pressure.
//! The worker never clears raster pages while a frame is collecting their addresses.
//! An atlas-full result therefore cannot corrupt another cached row's sampled region.
//! Transformed placements carry effective device geometry before source drawing.
//! Foreground tint is applied later to monochrome masks rather than baked into samples.

use super::BrowserFallback;
use puzzle_term::text::{
    cluster_source::ClusterRequest,
    deferred::{Completion, Deferred, Driver, Job},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use wasm_bindgen::JsCast;
/// Background continuations reserve the same ordinary work and refresh margin.
mod budget;
/// Readiness and call percentiles are bounded independently of scalar coverage facts.
mod readiness;
/// Opt-in counters are separate from Canvas task ownership.
mod stats;
/// Background task execution preserves ownership while yielding to browser paint.
mod task;
pub(in crate::runtime) use stats::Stats;
/// Source front end exchanges only bounded requests and completed owned ink.
struct BrowserDriver {
    /// The shared rAF allowance ends before the host's scheduling margin.
    deadline: Option<web_time::Instant>,
    /// Actual preparation charged to this frame, separate from background jobs.
    frame_time: std::time::Duration,
    /// A full discovery queue declines cold requests without delaying a frame.
    jobs: SyncSender<Job>,
    /// Number of accepted jobs not yet consumed by the frame.
    outstanding: usize,
    /// Completed coverage and samples are drained before the next row snapshot.
    ready: Receiver<Completion>,
    /// Readiness checks preserve one owned completion until a frame budget admits it.
    peeked: Option<Completion>,
    /// First draws and scheduled tasks share permanent Canvas coverage facts.
    work: Arc<Mutex<Work>>,
    /// Several cold glyphs arm one task, rather than one callback per cell.
    armed: Arc<AtomicBool>,
    /// Text-resource-specific event fans completion out to its attached sessions.
    wake: String,
}
/// Shared Canvas work retains no browser handles between operations.
struct Work {
    /// Worker success grants readiness only for the tested script/style and pixel stage.
    readiness: readiness::Readiness,
    /// Atomic observations expose completed idle work without accessing the font owner.
    stats: Arc<Stats>,
    /// Font policy and permanent scalar coverage facts survive idle task restarts.
    source: BrowserFallback,
    /// Queued whole clusters include exact transformed sample geometry.
    jobs: Receiver<Job>,
    /// Eight retained results bound source memory before the atlas owns samples.
    results: SyncSender<Completion>,
    /// A full result lane retains one completion until the next task can publish it.
    pending: Option<Completion>,
    /// A budget refusal retains a queued job without preparing or dropping it.
    queued: Option<Job>,
    /// Exhausted slack defers the next continuation to its next refresh edge.
    resume: Option<web_time::Instant>,
}
/// Constructs a service which performs no Canvas work before a glyph request.
pub(in crate::runtime) fn source(families: &[String], wake: &str) -> (Deferred, Arc<Stats>) {
    // Each context owns its evidence counters; no terminal borrows the Canvas worker.
    // Atomics let diagnostics observe completed tasks without acquiring its mutex.
    let stats = Arc::new(Stats {
        coverage_us: 0.into(),
        slots: 0.into(),
        host: Mutex::new(budget::Host::default()),
        #[cfg(any(test, feature = "measurements"))]
        active: false.into(),
        #[cfg(any(test, feature = "measurements"))]
        calls: Mutex::new(Vec::new()),
        #[cfg(any(test, feature = "measurements"))]
        events: Mutex::new(Vec::new()),
    });
    // Admission is bounded by cluster count; every job contains at most 32 UTF-8 bytes.
    // A refused job leaves its cell provisional and can be retried after a completion.
    let (jobs, input) = mpsc::sync_channel(128);
    // Completed pixels cannot accumulate behind a hidden or slow terminal indefinitely.
    // Backpressure stops the idle chain until the frame consumes this result lane.
    let (results, ready) = mpsc::sync_channel(8);
    let source = Deferred::new(Box::new(BrowserDriver {
        deadline: None,
        frame_time: std::time::Duration::ZERO,
        jobs,
        outstanding: 0,
        ready,
        peeked: None,
        wake: wake.into(),
        armed: Arc::new(AtomicBool::new(false)),
        work: Arc::new(Mutex::new(Work {
            readiness: Default::default(),
            stats: Arc::clone(&stats),
            source: BrowserFallback::for_resource(families, wake),
            jobs: input,
            results,
            pending: None,
            queued: None,
            resume: None,
        })),
    }));
    (source, stats)
}
/// Frame-time preparation is admitted by refresh slack; other requests use channels.
impl Driver for BrowserDriver {
    /// Coverage and sized ink travel together, so a cold draw can avoid a mailbox roundtrip.
    fn validates_during_prepare(&self) -> bool {
        true
    }
    /// The host charges its measured ordinary frame cost before offering this duration.
    fn begin_frame(&mut self, allowance: std::time::Duration) {
        self.frame_time = std::time::Duration::ZERO;
        self.deadline = Some(web_time::Instant::now() + allowance);
    }
    /// Returns this frame's measured source cost rather than the offered allowance.
    fn frame_preparation_time(&self) -> std::time::Duration {
        self.frame_time
    }
    /// Only evidence from a completed worker draw grants inline readiness.
    /// Median/p95 characterize complete calls; an overrun revokes that readiness.
    fn prepare_now(
        &mut self,
        request: ClusterRequest<'_>,
        placement: &puzzle_term::text::glyph::GlyphPlacement,
        validated: bool,
    ) -> Option<Result<Option<puzzle_term::text::cluster_source::ClusterGlyph>, String>> {
        let deadline = self.deadline?;
        let mut work = self.work.try_lock().ok()?;
        let costs = work.readiness.costs(request, placement)?;
        let remaining = deadline.saturating_duration_since(web_time::Instant::now());
        if !puzzle_term::text::preparation::fits(remaining, costs[1]) {
            return None;
        }
        work.stats.placement_event(2, Some(*placement), [0.0; 2]);
        let operations = work.source.operations.0.get();
        let started = web_time::Instant::now();
        // Inline work uses the same validated owned pixels as background work.
        let result = work.source.prepare_covered(request, placement, validated);
        let phases = work
            .source
            .preparation_costs
            .map(|d| d.as_secs_f64() * 1000.0);
        work.stats
            .placement_event(3, Some(*placement), [phases[0], 0.0]);
        work.stats.placement_event(4, Some(*placement), phases);
        work.stats
            .operations(Some(*placement), operations, work.source.operations.0.get());
        let elapsed = started.elapsed();
        let overrun = web_time::Instant::now() > deadline;
        work.readiness.record(
            request,
            placement,
            elapsed,
            false,
            matches!(result, Ok(Some(_))),
            overrun,
        );
        self.frame_time += elapsed;
        work.stats.call(1, elapsed, costs, remaining, overrun);
        work.record_stats();
        if matches!(result, Ok(None)) {
            // A newly disproved source must invalidate provisional rows so the
            // next fallback can be selected. Dispatch after releasing this borrow.
            let wake = self.wake.clone();
            wasm_bindgen_futures::spawn_local(async move { notify(&wake) });
        }
        Some(result)
    }
    /// Ready work requests its own frame; queued Canvas jobs never cause frame polling.
    fn ready(&mut self) -> bool {
        if self.peeked.is_none() {
            self.peeked = self.ready.try_recv().ok();
        }
        self.peeked.is_some()
    }
    /// Never probes coverage or draws samples on the calling frame thread.
    fn submit(&mut self, job: Job) -> bool {
        // try_send must remain the only admission operation: Canvas probing belongs to Work.
        // Count only accepted ownership, because a full queue produces no later completion.
        let Ok(mut work) = self.work.try_lock() else {
            return false;
        };
        let request = ClusterRequest {
            cluster: &job.key.cluster,
            style: job.key.style,
            presentation: job.key.presentation,
        };
        work.stats.event(0, &job, [0.0; 2]);
        if let Some(p) = job.placement
            && !work.readiness.reserve(request, &p)
        {
            return false;
        }
        let diagnostic = job.placement;
        let accepted = match self.jobs.try_send(job) {
            Ok(()) => true,
            Err(error) => {
                let job = match error {
                    mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job) => job,
                };
                work.readiness.cancel(ClusterRequest {
                    cluster: &job.key.cluster,
                    style: job.key.style,
                    presentation: job.key.presentation,
                });
                false
            }
        };
        if accepted {
            work.stats.placement_event(1, diagnostic, [0.0; 2]);
        }
        drop(work);
        self.outstanding += usize::from(accepted);
        if accepted {
            self.arm();
        }
        accepted
    }
    /// Outstanding work can be awaited by evidence tools, outside live frame building.
    fn pending(&self) -> bool {
        self.outstanding != 0
    }
    /// Taking completed work is a nonblocking channel read.
    fn poll(&mut self) -> Option<Completion> {
        // An empty lane returns immediately, retaining the diagnostic box for this frame.
        // Consuming a result frees a slot for suspended discovery without polling it.
        let result = self.peeked.take().or_else(|| self.ready.try_recv().ok());
        if let Some(done) = &result
            && let Ok(work) = self.work.try_lock()
        {
            work.stats.event(5, &done.job, [0.0; 2]);
        }
        self.outstanding = self
            .outstanding
            .saturating_sub(usize::from(result.is_some()));
        // Consuming a published result is an explicit continuation event, never idle polling.
        if result.is_some() {
            self.arm();
        }
        result
    }
}
/// Scheduling belongs to the platform driver, including resumption after backpressure.
impl BrowserDriver {
    /// Arms one macrotask chain; draining a full result lane is what resumes it.
    fn arm(&self) {
        // Several visible missing cells share one scheduled task rather than arming timers
        // per cell. Relaxed ordering suffices because the browser executes on one thread.
        if !self.armed.swap(true, Ordering::Relaxed) {
            // The task owns the service until it empties or reaches result backpressure.
            // It carries no Runtime borrow across an await or completion event.
            let work = Arc::clone(&self.work);
            let armed = Arc::clone(&self.armed);
            let wake = self.wake.clone();
            wasm_bindgen_futures::spawn_local(async move {
                loop {
                    // Timers yield to input and paint; no recursive microtask chain starves rAF.
                    let delay = work
                        .lock()
                        .ok()
                        .and_then(|mut w| w.resume.take())
                        .map_or(std::time::Duration::ZERO, |at| {
                            at.saturating_duration_since(web_time::Instant::now())
                        });
                    yield_for(delay).await;
                    // The mutex contains Rust state only and is never held during dispatch.
                    // A listener can therefore request a frame without re-entering Work.
                    let (more, published) = work.lock().map_or((false, false), |mut w| w.turn());
                    if !more {
                        // Release task ownership before synchronous dispatch. A listener
                        // draining the full lane can now arm its continuation immediately.
                        // Clearing after notify would lose that wake until another frame.
                        armed.store(false, Ordering::Relaxed);
                    }
                    if published {
                        // Only a newly available result wakes a frozen terminal.
                        // Ending an empty task produces no paint or polling timer.
                        notify(&wake);
                    }
                    if !more {
                        break;
                    }
                }
            });
        }
    }
}
/// Readiness tests use real Canvas calls and controlled budgets, without mocked fonts.
#[cfg(test)]
mod tests;
/// A real macrotask boundary, unlike awaiting an already resolved promise.
pub(super) async fn yield_task() {
    // A resolved Promise would only yield to microtasks, allowing a flood to starve
    // paint and input. The timer gives the browser a real event-loop boundary.
    yield_for(std::time::Duration::ZERO).await;
}
/// A queued job yields until its next useful browser task, never waiting for idle priority.
async fn yield_for(delay: std::time::Duration) {
    // Posted messages are tasks, not microtasks. Unlike nested zero-timeout
    // callbacks, they do not acquire a four-millisecond timer-clamping floor.
    // Both ports belong to this await and close as soon as it completes.
    if delay.is_zero()
        && let Ok(channel) = web_sys::MessageChannel::new()
    {
        let receive = channel.port1();
        let send = channel.port2();
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            receive.set_onmessage(Some(&resolve));
            let _ = send.post_message(&wasm_bindgen::JsValue::NULL);
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        receive.set_onmessage(None);
        receive.close();
        send.close();
        return;
    }
    // A spent refresh waits for its next edge, rather than continuously posting
    // tasks with no useful allowance. Channel refusal uses the ordinary timer.
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                resolve.unchecked_ref(),
                delay.as_millis().min(i32::MAX as u128) as i32,
            );
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
/// Completion is a platform fact; the resource listener submits the internal bus command.
pub(super) fn notify(name: &str) {
    // This event carries no glyph bytes; the listener queues GlyphRasterized on the bus.
    // Resource identity in the name keeps independent contexts from waking each other.
    if let (Some(window), Ok(event)) = (web_sys::window(), web_sys::Event::new(name)) {
        let _ = window.dispatch_event(&event);
    }
}
