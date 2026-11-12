//! Optional per-call Canvas evidence belongs to one text resource.
//!
//! The task and inline lane observe the same counters, while normal builds
//! retain no call vector. Samples carry predictions and remaining allowance
//! so a readiness policy can be assessed without changing its scheduling.
//! Queue timing is distinct from per-call CPU and eventual cell presentation.
//! Every stage retains the source face and stable glyph identifier for joining.
//! Size keeps transformed rasters distinct from ordinary grid preparations.
//! Wall-clock marks share an epoch with the presentation appearance observer.
//! Sub-millisecond CPU durations still use the monotonic timer at each call.
//! A queue-capacity refusal is recorded before a later accepted ownership mark.
//! Delivery acknowledges ownership, while the retained-frame observer proves ink.
//! No observer dispatches events, requests frames or changes source readiness.
//! The always-present host predictor contains bounded refresh state only.
//! It remains independent of whether per-glyph diagnostic capture is enabled.
//! A poisoned diagnostics mutex cannot make glyph preparation fail.
use std::sync::Mutex;
#[cfg(any(test, feature = "measurements"))]
use std::sync::atomic::{AtomicBool, Ordering};

/// Index evidence is owned by its text resource, never a page-global counter.
pub(in crate::runtime) struct Stats {
    /// The host supplies period, phase and inelastic cost, independent of diagnostics.
    pub(super) host: Mutex<super::budget::Host>,
    /// Cumulative Canvas coverage cost includes admitted first draws and idle jobs.
    pub(in crate::runtime) coverage_us: std::sync::atomic::AtomicUsize,
    /// Count of permanent scalar/style/presentation facts accumulated on demand.
    pub(in crate::runtime) slots: std::sync::atomic::AtomicUsize,
    /// Development observers opt in before retaining per-call timing evidence.
    #[cfg(any(test, feature = "measurements"))]
    pub(in crate::runtime) active: AtomicBool,
    /// Kind, elapsed, median, p95, available nanoseconds, and deadline overrun.
    #[cfg(any(test, feature = "measurements"))]
    pub(in crate::runtime) calls: Mutex<Vec<[u64; 6]>>,
    /// Stage, face, glyph, instance, device size, wall time, coverage ms and draw ms.
    #[cfg(any(test, feature = "measurements"))]
    pub(in crate::runtime) events: Mutex<Vec<[f64; 8]>>,
}
/// Opt-in observations do not change readiness or admission decisions.
impl Stats {
    /// Stages 6–9 retain call counts/costs, contexts, scalar probes and marker warmup.
    pub(super) fn operations(
        &self,
        placement: Option<puzzle_term::text::glyph::GlyphPlacement>,
        before: [u64; 7],
        after: [u64; 7],
    ) {
        // Subtract source totals rather than assigning calls to an unrelated
        // preceding job. Both worker and inline lanes use this same boundary.
        let n = std::array::from_fn::<_, 7, _>(|i| after[i].saturating_sub(before[i]));
        // Counts and durations are separate so API cost can be compared per read.
        self.placement_event(6, placement, [n[0] as f64, n[2] as f64]);
        self.placement_event(
            7,
            placement,
            [n[1] as f64 / 1_000_000.0, n[3] as f64 / 1_000_000.0],
        );
        self.placement_event(8, placement, [n[4] as f64, n[5] as f64]);
        // Marker warmup is not a repeated covered-scalar validation draw.
        // Keeping it distinct proves the one-read path after the marker exists.
        self.placement_event(9, placement, [n[6] as f64, 0.0]);
    }
    /// Retains individual costs so medians cannot conceal one indivisible overrun.
    pub(super) fn call(
        &self,
        kind: u64,
        cost: std::time::Duration,
        predicted: [std::time::Duration; 2],
        left: std::time::Duration,
        overrun: bool,
    ) {
        #[cfg(any(test, feature = "measurements"))]
        // Only an explicitly enabled workload may allocate per-call evidence.
        if self.active.load(Ordering::Relaxed)
            && let Ok(mut calls) = self.calls.lock()
        {
            // Saturate conversion so a diagnostic duration never wraps its counter.
            let ns = |d: std::time::Duration| d.as_nanos().min(u128::from(u64::MAX)) as u64;
            // Keep the offered budget and predictions next to actual cost for attribution.
            calls.push([
                kind,
                ns(cost),
                ns(predicted[0]),
                ns(predicted[1]),
                ns(left),
                u64::from(overrun),
            ]);
        }
        #[cfg(not(any(test, feature = "measurements")))]
        // Production builds erase the sample vector and retain no observer side effects.
        let _ = (kind, cost, predicted, left, overrun);
    }
}

/// Stage timestamps exist only in explicitly activated development observations.
impl Stats {
    /// Coverage-only jobs have no placement and cannot be mistaken for drawn ink.
    pub(super) fn event(
        &self,
        stage: u32,
        job: &puzzle_term::text::deferred::Job,
        costs: [f64; 2],
    ) {
        self.placement_event(stage, job.placement, costs);
    }
    /// Uses a shared browser clock to correlate source work with actual presentation.
    pub(super) fn placement_event(
        &self,
        stage: u32,
        placement: Option<puzzle_term::text::glyph::GlyphPlacement>,
        costs: [f64; 2],
    ) {
        #[cfg(any(test, feature = "measurements"))]
        // Capturing identifiers changes no job ownership or scheduling policy.
        if self.active.load(Ordering::Relaxed)
            && let Some(p) = placement
            && let Ok(mut events) = self.events.lock()
        {
            events.push([
                f64::from(stage),
                f64::from(p.key.face),
                f64::from(p.key.glyph_id),
                f64::from(p.key.instance),
                f64::from(p.size_px),
                js_sys::Date::now(),
                costs[0],
                costs[1],
            ]);
        }
        #[cfg(not(any(test, feature = "measurements")))]
        // Normal builds carry neither the evidence vector nor browser clock calls.
        let _ = (stage, placement, costs);
    }
}

/// Host scheduling observations reach the source without exposing browser objects.
impl Stats {
    /// Every paint updates the measured phase and remaining ordinary cost.
    pub(in crate::runtime) fn refresh(
        &self,
        tick: web_time::Instant,
        period: std::time::Duration,
        cost: std::time::Duration,
    ) {
        if let Ok(mut host) = self.host.lock() {
            host.update(tick, period, cost);
        }
    }
}
