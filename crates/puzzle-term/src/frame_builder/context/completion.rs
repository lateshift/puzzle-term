//! Publishes background source ink into the shared caches before its row can disappear.
//!
//! Completion events request frames; the frame-building owner publishes ready ink
//! at those boundaries, independently of Session publications. It drains existing
//! lanes under a time allowance and retains uploads until a frame writes them.
//! Visibility never decides whether successfully extracted ink is retained (P6).
use super::{
    TextContext,
    publication::{Kind, Work},
};

/// Cumulative atlas publication work; no glyph bytes or paths survive in these counters.
#[derive(Clone, Copy, Debug, Default)]
pub struct CompletionMetrics {
    /// Vector mirror growth identifies allocation outliers in packing turns.
    pub growth_units: u64,
    /// Largest unit that increased vector capacity, separately from ordinary packing.
    pub worst_growth_nanos: u64,
    /// Current predicted indivisible unit cost exposes a starved publication lane.
    pub predicted_unit_nanos: u64,
    /// Host allowance distinguishes an expensive unit from a lost refresh edge.
    pub allowance_nanos: u64,
    /// Poll turns, including empty calls, separate host activity from actual results.
    pub turns: u64,
    /// Owned ink variants transferred into vector or raster stores.
    pub ink: u64,
    /// Total polling and atlas preparation time in nanoseconds.
    pub nanos: u64,
    /// Worst single turn detects long publication bursts outside frame assembly.
    pub worst_nanos: u64,
}

/// Worker publication changes row generations without taking a terminal snapshot.
impl TextContext {
    /// Hosts supply refresh slack after accounting for measured ordinary frame work.
    pub fn set_completion_budget(&mut self, budget: std::time::Duration) {
        // Changing the allowance does not consume any retained ready result.
        self.publication.budget = budget;
    }

    /// Actual publication cost is excluded from the next ordinary-frame cost sample.
    #[must_use]
    pub fn completion_time(&self) -> std::time::Duration {
        // This duration measures publication only, not row building or source preparation.
        self.publication.last
    }

    /// Actual first-draw source preparation, measured by the platform source owner.
    #[must_use]
    pub fn source_preparation_time(&self) -> std::time::Duration {
        // The platform driver measures its own inline calls; the core never guesses them.
        self.fonts
            .as_ref()
            .map_or(std::time::Duration::ZERO, |f| f.source_preparation_time())
    }

    /// Ready work, including an idle frozen screen, requests its own next frame.
    pub fn source_completions_ready(&mut self) -> bool {
        // A unit already taken from a result lane still needs a future frame.
        self.publication.ready()
            || self
                .fonts
                .as_mut()
                .is_some_and(|fonts| fonts.sources_ready())
    }
    /// Enables opt-in counters without changing completion admission or scheduling.
    pub fn measure_completions(&mut self) {
        // Observers are explicitly enabled; ordinary contexts retain no metrics owner.
        self.completion_metrics = Some(CompletionMetrics::default());
    }

    /// Returns a cumulative snapshot for native or browser development measurements.
    #[must_use]
    pub fn completion_metrics(&self) -> Option<CompletionMetrics> {
        // Copy scalar counters without exposing borrowed font or atlas data.
        self.completion_metrics
    }

    /// Publishes ready source ink under refresh slack even after output replaced the cell.
    /// Hosts call this at frame boundaries, before collecting any shape addresses.
    /// No font I/O or shaping runs here. Unpublished uploads remain pending for the renderer.
    pub fn process_source_completions(&mut self) -> bool {
        // Publication is time-admitted, independent of channel size or glyph count.
        // The ordinary empty turn performs no font extraction or atlas allocation.
        let started = web_time::Instant::now();
        // Count complete owned units rather than individual layers or pixel bytes.
        let mut ink_count = 0;
        // Coverage facts can change fallback choice even when they carry no drawable ink.
        let mut changed = false;
        // Each ready kind is independently admitted. A declined bitmap leaves
        // the vector lane available rather than imposing its cost on every unit.
        let mut blocked = [false; 2];
        loop {
            // Taking ownership is cheap and reveals the actual publication kind.
            let (work, facts_changed) = self.take_publication(&blocked);
            // A rejected source answer must invalidate its provisional row too.
            changed |= facts_changed;
            // No available producer work ends the turn without requesting polling.
            let Some(work) = work else {
                break;
            };
            // Choose a cost history before performing any atlas mutation.
            let kind = work.kind();
            // Each producer retains at most one declined unit until another refresh.
            let slot = match &work {
                Work::Loaded(_) => 0,
                Work::Source(..) => 1,
            };
            // Elapsed work and predicted next cost must both fit the host allowance.
            if started.elapsed() + self.publication.predicted(kind) >= self.publication.budget {
                // Keep ownership intact; the worker must not extract this glyph again.
                self.publication.waiting[slot] = Some(work);
                // A declined producer cannot overwrite its retained unit in this turn.
                blocked[slot] = true;
                continue;
            }
            // Only an admitted unit can write vector or raster atlas data.
            self.publish_work(work, kind);
            changed = true;
            // The diagnostic count advances after publication, never after a readiness peek.
            ink_count += 1;
        }
        if changed {
            // Existing per-session generation checks invalidate provisional rows.
            self.generation = self.generation.wrapping_add(1);
        }
        // The pacer separates this cost from its ordinary remaining-frame estimate.
        self.publication.last = started.elapsed();
        // Diagnostic accumulation occurs after the timed turn is complete.
        self.record_publication(ink_count);
        changed
    }

    /// Packs one admitted owned unit and records growth apart from ordinary publication.
    fn publish_work(&mut self, work: Work, kind: Kind) {
        // A single complete result is the indivisible admission operation.
        let unit_started = web_time::Instant::now();
        // Capacity changes distinguish allocation work from writes into existing mirrors.
        let bytes = [
            self.backend.atlas().allocated_bytes(),
            self.raster.allocated_bytes(),
        ];
        match work {
            // Loaded faces already own validated preparation; publication performs no font I/O.
            Work::Loaded(done) => self.publish_loaded(done),
            Work::Source(p, ink) => {
                // Invalid geometry keeps its diagnostic instead of pretending to be usable ink.
                if let Err(error) = self.cache_source_ink(&p, ink) {
                    tracing::debug!(event = "text.source_cache_deferred", %error);
                }
            }
        }
        // Capture actual packing/copy cost independently for the next prediction.
        let cost = unit_started.elapsed();
        // Either vector or raster capacity growth belongs to the allocation kind.
        let grew = bytes
            != [
                self.backend.atlas().allocated_bytes(),
                self.raster.allocated_bytes(),
            ];
        // Allocator work is a different operation from packing into existing rows.
        // Recording it separately cannot poison subsequent cheap vector predictions.
        self.publication
            .record(if grew { Kind::Allocation } else { kind }, cost);
        // Growth outliers remain observable even though cheap kinds have separate histories.
        if grew && let Some(metrics) = &mut self.completion_metrics {
            // Count reallocations, not the number of glyphs in the new capacity.
            metrics.growth_units += 1;
            metrics.worst_growth_nanos = metrics
                .worst_growth_nanos
                .max(cost.as_nanos().min(u128::from(u64::MAX)) as u64);
        }
    }

    /// Captures opt-in diagnostics after the timed turn without retaining geometry.
    fn record_publication(&mut self, ink_count: u64) {
        // Without an observer this branch performs no formatting or sample allocation.
        if let Some(metrics) = &mut self.completion_metrics {
            metrics.predicted_unit_nanos = self
                .publication
                .predicted(Kind::Vector)
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64;
            metrics.allowance_nanos =
                self.publication.budget.as_nanos().min(u128::from(u64::MAX)) as u64;
            // Nanoseconds retain cheap turns instead of rounding them down to zero.
            let nanos = self.publication.last.as_nanos().min(u128::from(u64::MAX)) as u64;
            // Empty turns stay visible when diagnosing a stalled handoff.
            metrics.turns += 1;
            // Owned units count once regardless of their number of Slug color layers.
            metrics.ink += ink_count;
            // Cumulative time permits comparison with preparation and result-lane wait.
            metrics.nanos += nanos;
            // Retain the worst turn so averages cannot hide a frame-sized spike.
            metrics.worst_nanos = metrics.worst_nanos.max(nanos);
        }
    }

    /// Publishes one loaded-face job, preserving its original fitting/synthesis policy.
    fn publish_loaded(&mut self, done: puzzle_text::preparation::Completion) {
        // Loaded-face completions share the same deadline and atlas boundary.
        match done.result {
            Ok(Some(puzzle_text::preparation::Ink::Outline(ink))) => {
                // The original placement and synthesis flags survive changes to the visible row.
                let result = self
                    .backend
                    .prepare_outline(
                        &done.placement,
                        ink,
                        done.style[0],
                        done.style[1],
                        done.style[2],
                    )
                    .and_then(|prepared| self.cache_overflow(&done.placement, prepared));
                // A failed loaded shape is reported rather than installed under a final cache key.
                if let Err(error) = result {
                    tracing::warn!(event = "text.loaded_preparation_failed", %error);
                }
            }
            Ok(Some(puzzle_text::preparation::Ink::Color(color))) => {
                // A complete color graph chooses one backend, preserving its layer order.
                match self.backend.prepare_loaded_color(&done.placement, color) {
                    Ok(Some(samples)) => {
                        let _ = self.cache_overflow(
                            &done.placement,
                            puzzle_text::glyph::PreparedGlyph::Rasterized(samples),
                        );
                    }
                    // Whitespace-like glyphs legitimately own no drawable shape.
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(event = "text.loaded_color_failed", %error)
                    }
                }
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(event = "text.loaded_preparation_failed", %error),
        }
    }

    /// Takes cheap completed ownership before deciding whether its kind fits this frame.
    fn take_publication(&mut self, blocked: &[bool; 2]) -> (Option<Work>, bool) {
        // Retry owned ready units before consuming another result from either producer.
        for (slot, blocked) in blocked.iter().enumerate() {
            if !blocked && let Some(work) = self.publication.waiting[slot].take() {
                // Retained ink carries no new coverage-fact change on this retry.
                return (Some(work), false);
            }
        }
        // One retained unit keeps the same result-lane ownership. Refusing its
        // cost never consumes another result into that occupied slot.
        // Alternating the first producer avoids permanent preference for one lane.
        self.publication.next_loaded = !self.publication.next_loaded;
        if self.publication.next_loaded
            && !blocked[0]
            && let Some(done) = self.fonts.as_mut().and_then(|f| f.take_preparation_ready())
        {
            return (Some(Work::Loaded(done)), false);
        }
        let mut changed = false;
        if !blocked[1] {
            // Polling coverage can complete a negative answer without allocating atlas ink.
            changed = self.fonts.as_mut().is_some_and(|f| f.poll_source());
            if let Some((p, ink)) = self.fonts.as_mut().and_then(|f| f.take_source_ready()) {
                return (Some(Work::Source(p, ink)), changed);
            }
        }
        // Never drain a lane whose previous unit still occupies its waiting slot.
        let loaded = if blocked[0] {
            None
        } else {
            self.fonts
                .as_mut()
                .and_then(|f| f.take_preparation_ready())
                .map(Work::Loaded)
        };
        // Propagate a negative coverage change even when both ink lanes are empty.
        (loaded, changed)
    }
}
