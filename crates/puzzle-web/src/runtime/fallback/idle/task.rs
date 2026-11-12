//! Background Canvas jobs retain ownership across bounded browser tasks.
//!
//! Timing marks separate queue residence, coverage probes, final draw and
//! delivery to the atlas owner. Browser-internal system-font loading has no
//! separate Canvas API timestamp and remains inside the measured calls.
//! Accepted work is FIFO: a refused next call stays ahead of later jobs.
//! The queue entry and prepared completion each have exactly one owner.
//! No retry re-rasterizes a sample which already reached the result lane.
//! A task receives remaining wall-clock time, not a fresh period per glyph.
//! Predicted costs come from successful draws at the same device geometry.
//! Opaque first-use calls have no safe inline estimate and stay here.
//! A background overrun remains measurable even if no rAF frame was active.
//! Result backpressure ends the chain until publication explicitly resumes it.
//! Font readiness and source coverage survive these task-chain restarts.
//! Coverage-only jobs carry no bitmap and cannot fabricate an ink appearance.
//! Diagnostic wall timestamps correlate ownership with the final submitted cell.
//! Monotonic call durations separately retain sub-millisecond draw costs.
use super::*;

/// The idle owner performs coverage and rasterization between presentation turns.
impl Work {
    /// Both admitted and deferred Canvas work update the same resource-owned evidence.
    pub(super) fn record_stats(&self) {
        self.stats
            .coverage_us
            .store(self.source.index_us, Ordering::Relaxed);
        self.stats
            .slots
            .store(self.source.indexed_slots, Ordering::Relaxed);
    }
    /// One bounded task leaves pending results and jobs intact when its budget expires.
    pub(super) fn turn(&mut self) -> (bool, bool) {
        // Callback delay has already consumed time in the host's refresh horizon.
        let start = web_time::Instant::now();
        let (allowance, edge, maximum) = self.stats.host.lock().map_or(
            (std::time::Duration::ZERO, start, std::time::Duration::ZERO),
            |h| h.window(start),
        );
        // This absolute edge includes every earlier source call in this task.
        let deadline = start + allowance;
        let mut published = false;
        loop {
            // Publish the preceding job before allocating another sample buffer.
            // A saturated lane retains just this one result and disarms the timer.
            if let Some(done) = self.pending.take() {
                match self.results.try_send(done) {
                    Ok(()) => published = true,
                    Err(mpsc::TrySendError::Full(done)) => {
                        self.pending = Some(done);
                        return (false, published);
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => return (false, published),
                }
            }
            // Retain singular ownership if this refresh cannot admit the next known call.
            let Some(job) = self.queued.take().or_else(|| self.jobs.try_recv().ok()) else {
                // An empty queue leaves no task chain or periodic frame request (P2).
                return (false, published);
            };
            let request = ClusterRequest {
                cluster: &job.key.cluster,
                style: job.key.style,
                presentation: job.key.presentation,
            };
            // Style, script and tested bitmap area select the measured prediction.
            let predicted = job
                .placement
                .and_then(|p| self.readiness.costs(request, &p));
            // Budget checks use current time rather than subtracting rounded counters.
            let left = deadline.saturating_duration_since(web_time::Instant::now());
            let fits = predicted.is_none_or(|c| puzzle_term::text::preparation::fits(left, c[1]));
            // Opaque first use and a call larger than an entire task allowance stay
            // off-frame and indivisible. They cannot be made safe by inline admission.
            let indivisible = predicted.is_none_or(|c| c[1] > maximum);
            if (!fits && !indivisible) || (published && left.is_zero()) {
                // No preparation happened, so retrying preserves singular ownership.
                self.queued = Some(job);
                // Wait for a new useful refresh instead of spinning on exhausted slack.
                self.resume = Some(edge);
                return (true, published);
            }
            self.finish(job);
        }
    }
    /// One background call validates and draws once, retaining the pixels it produced.
    fn finish(&mut self, job: Job) {
        let r = ClusterRequest {
            cluster: &job.key.cluster,
            style: job.key.style,
            presentation: job.key.presentation,
        };
        // Deferred jobs run outside rAF; admitted first draws use the same source.
        // Permanent scalar negatives survive eviction of whole-cluster metadata.
        self.stats.event(2, &job, [0.0; 2]);
        let operations = self.source.operations.0.get();
        let call = web_time::Instant::now();
        // Exact placement metrics distinguish a transformed raster from grid samples.
        // The validation bitmap itself transfers to drawing; only unseen components
        // of composed clusters require additional probes, cached per scalar.
        let result = if let Some(p) = job.placement {
            self.source.prepare_covered(r, &p, job.validated)
        } else {
            Ok(None)
        };
        let covered =
            matches!(result, Ok(Some(_))) || (job.placement.is_none() && self.source.covers(r));
        let costs = self
            .source
            .preparation_costs
            .map(|d| d.as_secs_f64() * 1000.0);
        self.stats.event(3, &job, [costs[0], 0.0]);
        self.stats.event(4, &job, costs);
        self.record_stats();
        self.stats
            .operations(job.placement, operations, self.source.operations.0.get());
        if let Some(p) = job.placement {
            let elapsed = call.elapsed();
            self.readiness
                .record(r, &p, elapsed, true, matches!(result, Ok(Some(_))), false);
            self.stats.call(
                0,
                elapsed,
                self.readiness.costs(r, &p).unwrap_or_default(),
                std::time::Duration::ZERO,
                false,
            );
        }
        // The original key/placement permits downstream identity-safe publication.
        self.pending = Some(Completion {
            job,
            covered,
            result,
        });
    }
}
