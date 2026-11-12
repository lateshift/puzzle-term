//! Opt-in frame-gate evidence distinguishes CPU work from missing presentations.
//!
//! A successful GPU readback alone cannot prove that a native surface presented.
//! Record every admitted attempt, including no-damage and surface failures. The
//! observer allocates only after an explicit probe starts; formatting occurs
//! after measurement. Deadline-denied callbacks are counted separately because
//! they must not capture an early snapshot under D-33.
//! D-47 samples bitmap population and evictions alongside vector writes.
//! Each attempt's cumulative counters permit time-windowed cache churn analysis.
//! Samples remain opt-in; ordinary windows do not grow this evidence list.
//! Grid/copy counts distinguish full damaged screens from reused-row snapshots.
//! Actor wait stamps precede driver work, so late OS delivery is not attributed
//! to snapshot copying, glyph lookup or the main thread's AppKit event loop.
//! Current request identity is separate from the source answer's identity: a
//! truthful late flag alone cannot reveal that a previous answer was selected.
//! Native callback timestamps distinguish display delivery from actor wake delay;
//! immediate startup/resize exceptions clear that source instead of inheriting it.
use super::PresentTiming;
/// Scalar clocks are separate from the existing phase-duration evidence.
mod clocks;
use clocks::Clocks;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// One native attempt retains its gate and independent phase durations.
struct Attempt {
    /// Every attempted refresh carries its own receive and presentation arithmetic.
    clocks: Clocks,
    /// Submission ordinal joins this build to the actual native contents assignment.
    submission_serial: Option<u64>,
    /// Predicted display edge; unknown phase never invents one for evidence.
    deadline: Option<Duration>,
    /// Actual queue submission relative to the producer-release clock.
    submitted: Option<Duration>,
    /// New vector keys and upload spans distinguish a cold frame from warmed work.
    cold: Option<[usize; 2]>,
    /// Vector write count measures coalescing independently of raster/paint uploads.
    upload_spans: [usize; 2],
    /// Bitmap population, reserve bytes and page evictions at this attempted frame.
    grid_cache: [u64; 3],
    /// Context epoch, cumulative preparations and vector evictions reveal cache resets.
    context: [u64; 3],
    /// Offset from producer release, retaining failed attempts in the denominator.
    offset: Duration,
    /// Entire redraw duration, including unsuccessful surface acquisition.
    cpu: Duration,
    /// Same-render-thread executed work excludes scheduler and FIFO waiting.
    thread_cpu: Option<Duration>,
    /// Boundary work includes page eviction and publication, before capturing rows.
    boundary: Duration,
    /// Bounded channel receipt time, separate from owner-side copying.
    snapshot: Duration,
    /// Captured columns, visible rows and copied rows; wait/copy timing stays separate.
    snapshot_grid: [usize; 3],
    /// Last completed gate names the reason an attempt did not present.
    outcome: &'static str,
    /// Cache/order, rows, assembly, geometry, overlays and upload collection.
    build_phases: Option<[f64; 6]>,
    /// Metrics, clusters/fallback, shaping/placement and glyph instance preparation.
    row_phases: Option<[f64; 4]>,
    /// Cumulative layer lookups permit exact warm-window differences.
    cache_metrics: Option<[[u64; 6]; 6]>,
    /// Actual row misses distinguish new Unicode screens from unchanged cache hits.
    rebuilt_rows: Option<usize>,
    /// Successful attempts carry build, upload-only, acquire and full submit CPU.
    /// The last cost excludes FIFO wait and reserves the approved warm-frame allowance.
    phases: Option<[f64; 4]>,
}

/// Development-only timing owner does not request redraws or change admission.
#[derive(Default)]
pub struct Metrics {
    /// Current clocks are copied into the attempt without allocation.
    clocks: Clocks,
    /// The pacer supplies an immutable plan before the attempt begins.
    plan: Clocks,
    /// The pacer supplies this before a redraw, independently of successful acquisition.
    deadline: Option<Instant>,
    /// None leaves instrumentation inactive in an ordinary window.
    origin: Option<Instant>,
    /// Raw attempt samples are retained without filtering out slow or failed work.
    attempts: Vec<Attempt>,
    /// Winit can deliver expose redraws before the chosen refresh deadline.
    denied: usize,
    /// Current attempt's terminal or surface gate.
    outcome: &'static str,
    /// Render-owned atlas boundary work precedes the owner frame request.
    boundary: Duration,
    /// Publication receipt timing excludes render-owned boundary preparation.
    snapshot: Duration,
    /// Captured columns, visible rows and copied rows; wait/copy timing stays separate.
    snapshot_grid: [usize; 3],
    /// CPU clock is sampled only while this explicit observer is active.
    cpu_started: Option<Duration>,
}

impl Metrics {
    /// Native source clocks remain raw values until cohort reporting finishes.
    #[cfg(target_os = "macos")]
    pub fn refresh_source(&mut self, tick: crate::render_thread::surface::Refresh) {
        self.plan.link = Some(tick);
    }
    /// Retains the predicted display edge independently of complete glyph preparation.
    pub fn prediction(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
    }
    /// Activates observation at the exact producer-release boundary.
    pub fn start(&mut self) {
        self.start_at(Instant::now());
    }

    /// Shares the scroll recorder's origin so callback and acquire clocks align.
    pub fn start_at(&mut self, origin: Instant) {
        self.origin = Some(origin);
        self.attempts = Vec::with_capacity(4096);
    }

    /// Counts scheduling rejections without sampling or building a frame.
    pub fn denied(&mut self) {
        self.denied += usize::from(self.origin.is_some());
    }

    /// Begins one admitted attempt; no glyph or grid state is observed here.
    pub fn begin(&mut self) {
        // Ordinary windows perform no OS clock read for this dormant observer.
        self.cpu_started = self.origin.and_then(|_| crate::thread_cpu::now());
        self.clocks = self.plan;
        self.outcome = "no_snapshot";
        self.boundary = Duration::ZERO;
        self.snapshot = Duration::ZERO;
        self.snapshot_grid = [0; 3];
    }

    /// Names the failed gate without logging once per frame.
    pub fn gate(&mut self, outcome: &'static str) {
        self.outcome = outcome;
    }

    /// Captures render-side atlas preparation before requesting a sealed slot.
    pub fn boundary(&mut self, elapsed: Duration) {
        self.boundary = elapsed;
    }

    /// Captures the actual snapshot operation independently of row building.
    pub fn snapshot(&mut self, elapsed: Duration, grid: [usize; 3]) {
        self.snapshot = elapsed;
        self.snapshot_grid = grid;
    }

    /// Every admitted attempt is retained, whether or not a drawable was returned.
    pub fn finish(
        &mut self,
        started: Instant,
        timing: Option<&PresentTiming>,
        upload_spans: [usize; 2],
        grid_cache: [u64; 3],
        context: [u64; 3],
        submission_serial: Option<u64>,
    ) {
        // Normal windows bypass allocation and sample collection entirely.
        let Some(origin) = self.origin else { return };
        // Stop the clock before evidence allocation so reporting cannot look like
        // expensive rendering. Both reads belong to this one rendering thread.
        let thread_cpu = crate::thread_cpu::elapsed(self.cpu_started);
        let ms = |value: Duration| value.as_secs_f64() * 1000.0;
        // Capture unsuccessful attempts too; filtering them would inflate throughput.
        self.clocks.presentation = timing.map(|t| [t.acquired, t.submitted, t.presented]);
        self.attempts.push(Attempt {
            clocks: self.clocks,
            submission_serial,
            upload_spans,
            grid_cache,
            context,
            // Unknown refresh phase stays absent rather than fabricating a target.
            deadline: self.deadline.map(|at| at.saturating_duration_since(origin)),
            submitted: timing.map(|t| t.submitted.saturating_duration_since(origin)),
            cold: timing.map(|t| [t.work.glyphs, t.work.uploads]),
            offset: started.saturating_duration_since(origin),
            cpu: started.elapsed(),
            thread_cpu,
            boundary: self.boundary,
            snapshot: self.snapshot,
            snapshot_grid: self.snapshot_grid,
            outcome: if timing.is_some() {
                "presented"
            } else {
                self.outcome
            },
            build_phases: timing.map(|t| t.work.build_phases.map(ms)),
            cache_metrics: timing.map(|t| t.work.cache_metrics),
            row_phases: timing.map(|t| t.work.row_phases.map(|ns| ns as f64 / 1e6)),
            rebuilt_rows: timing.map(|t| t.work.rebuilt_rows),
            phases: timing.map(|t| {
                [
                    ms(t.work.build),
                    ms(t.work.upload_cpu),
                    ms(t.blocked),
                    ms(t.work.submit_cpu),
                ]
            }),
        });
    }

    /// Capacity and preparation errors must never abort a complete Unicode frame.
    pub fn aborted_builds(&self) -> usize {
        self.attempts
            .iter()
            .filter(|attempt| attempt.outcome == "build_failed")
            .count()
    }
    /// Formats raw gate evidence only after the timed workload has completed.
    pub fn report(&self) -> Value {
        let ms = |value: Duration| value.as_secs_f64() * 1000.0;
        // JSON allocation happens after the run, outside measured frame turns.
        let attempts: Vec<_> = self
            .attempts
            .iter()
            .map(|a| {
                json!({
                    "cadence": a.clocks.report(self.origin.unwrap_or_else(Instant::now)),
                    "submission_serial":a.submission_serial,
                    "deadline_ms":a.deadline.map(ms),
                    "submitted_ms":a.submitted.map(ms), "cold_keys_upload_spans":a.cold,
                    "vector_write_calls_before_after":a.upload_spans,
                    "grid_cache_entries_bytes_evictions":a.grid_cache,
                    "context_generation_prepared_vector_evictions":a.context,
                    "start_ms":ms(a.offset), "frame_cpu_ms":ms(a.cpu), "outcome":a.outcome,
                    "frame_thread_cpu_ms":a.thread_cpu.map(ms),
                    "boundary_ms":ms(a.boundary), "snapshot_ms":ms(a.snapshot),
                    "snapshot_columns_rows_copied":a.snapshot_grid,
                    "cache_metrics":a.cache_metrics, "build_phases_ms":a.build_phases, "row_phases_ms":a.row_phases, "rebuilt_rows":a.rebuilt_rows,
                    "build_ms":a.phases.map(|p| p[0]), "upload_ms":a.phases.map(|p| p[1]),
                    "acquire_wait_ms":a.phases.map(|p| p[2]),
                    "upload_submit_ms":a.phases.map(|p| p[3]),
                })
            })
            .collect();
        json!({"deadline_denied":self.denied, "attempts":attempts})
    }
}

/// Clock capture stays inert unless the explicit finite observer has started.
impl Metrics {
    /// Copies the worker wait before any recovery, probe or pacing work runs.
    pub fn actor_wake(
        &mut self,
        sample: Option<crate::render_thread::wake::Sample>,
        entered: Instant,
    ) {
        self.plan.wake = sample.map(|sample| (sample, entered));
    }
    /// Retains the exact pacer terms before take consumes the reserved refresh.
    pub fn clock_plan(
        &mut self,
        plan: Option<(Instant, Instant, u64, &'static str)>,
        costs: [Duration; 2],
    ) {
        self.plan.plan = plan;
        if plan.is_none() {
            // Startup/resize exceptions must not inherit an obsolete native target.
            self.plan.link = None;
        }
        self.plan.costs = costs;
    }
    /// The current request remains distinct from any older source drawn this turn.
    pub fn requested(&mut self, at: Instant, deadline: Instant, id: u64) {
        self.clocks.request = Some((at, deadline, id));
    }
    /// Associate the actual answer with the frame, including its truthful late flag.
    pub fn received(&mut self, at: Instant, source: crate::session_thread::PublicationTiming) {
        self.clocks.received = Some(at);
        self.clocks.source = Some(source);
    }
    /// Record work start after snapshot adoption and before glyph preparation.
    pub fn build_started(&mut self) {
        self.clocks.build = Some(Instant::now());
    }
}
