//! Opt-in native discovery timings separate CPU work from complete render-thread preparation.
//!
//! Startup indexing and synchronous preparation write these counters. A development observer reads
//! cumulative snapshots without a lock or a per-glyph log. Ordinary windows
//! omit stage timers; enabling counters does not change candidate selection.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Instant;

/// Request sub-stages and overlapping totals separate discovery from actual ink work.
#[derive(Clone, Copy)]
pub(super) enum Stage {
    /// One-time system enumeration and cmap inversion.
    Index,
    /// Indexed candidate intersection and preference ranking.
    Lookup,
    /// Our charmap validates every visible scalar in the complete cluster.
    Coverage,
    /// Whole-cluster shaping, outline extraction or bitmap decoding.
    Extraction,
    /// Fontique maps a selected candidate's byte owner.
    Mapping,
    /// Candidate byte ownership is released and the mapping cache pruned.
    Unmapping,
    /// Script fallback policy discovery finishes before the source is frame-available.
    Preferences,
}

/// Shared counters retain no fonts, clusters or glyph paths.
#[derive(Default)]
pub(super) struct Timing {
    /// Aggregate nanoseconds, indexed by Stage; job totals overlap their sub-stages.
    nanos: [AtomicU64; 7],
    /// Invocation counts reveal repeated validation and extraction.
    counts: [AtomicU64; 7],
    /// Current loaded face count and mapped bytes, independent of Rust heap/RSS.
    resident: [AtomicU64; 2],
    /// Lifetime high-water marks reveal the working set under real workloads.
    resident_peak: [AtomicU64; 2],
    /// Capacity-driven releases are distinct from candidate failures and index scanning.
    evictions: AtomicU64,
    /// Opt-in per-call samples retain outliers instead of hiding them in totals.
    calls: std::sync::Mutex<Vec<[u64; 2]>>,
}

impl Timing {
    /// Render-owned mapping statistics accompany stage time without per-glyph logging.
    pub(super) fn resident(&self, faces: usize, bytes: usize, evictions: usize) {
        // Resident faces and virtual mapped bytes are different resource dimensions.
        for (i, value) in [faces as u64, bytes as u64].into_iter().enumerate() {
            self.resident[i].store(value, Ordering::Relaxed);
            self.resident_peak[i].fetch_max(value, Ordering::Relaxed);
        }
        // Releases are reported cumulatively, separate from the current owner count.
        self.evictions.store(evictions as u64, Ordering::Relaxed);
    }
    /// Returns an independent timer only when a diagnostic explicitly enables it.
    pub(super) fn start(timing: &Option<Arc<Self>>, stage: Stage) -> Option<Span> {
        // Ordinary windows allocate neither an Arc clone nor a stage timestamp.
        timing.as_ref().map(|owner| Span {
            owner: Arc::clone(owner),
            stage,
            started: Instant::now(),
        })
    }

    /// A snapshot reads monotonically increasing counters; the font owner never waits for it.
    #[cfg(any(test, feature = "lock-metrics"))]
    pub(super) fn report(&self) -> serde_json::Value {
        // Stable stage names pair with fixed enum slots in each development report.
        let names = [
            "index",
            "lookup",
            "candidate_coverage",
            "extraction",
            "mapping",
            "unmapping",
            "preferences",
        ];
        // Stages are reported individually so nested job totals are not falsely added.
        let mut report = serde_json::Value::Object(
            names
                .into_iter()
                .enumerate()
                .map(|(i, name)| {
                    (
                        name.into(),
                        serde_json::json!({
                            "count": self.counts[i].load(Ordering::Relaxed),
                            "milliseconds": self.nanos[i].load(Ordering::Relaxed) as f64 / 1e6,
                        }),
                    )
                })
                .collect(),
        );
        // Mapped-byte totals describe virtual ownership; process RSS/VSZ are measured separately.
        report["loaded_faces"] = serde_json::json!({
            "faces": self.resident[0].load(Ordering::Relaxed),
            "mapped_bytes": self.resident[1].load(Ordering::Relaxed),
            "peak_faces": self.resident_peak[0].load(Ordering::Relaxed),
            "peak_mapped_bytes": self.resident_peak[1].load(Ordering::Relaxed),
            "evictions": self.evictions.load(Ordering::Relaxed),
        });
        // Individual samples preserve tails hidden by cumulative stage totals.
        report["per_call_stage_nanoseconds"] = self
            .calls
            .lock()
            .ok()
            .map_or(serde_json::Value::Null, |calls| serde_json::json!(*calls));
        report
    }
}

/// An owned scope guard avoids borrowing font state while its operation is timed.
pub(super) struct Span {
    /// Keeps the diagnostic counters alive across an individual operation.
    owner: Arc<Timing>,
    /// One fixed slot identifies the preparation operation without formatting strings.
    stage: Stage,
    /// Monotonic wall time includes OS mapping waits where relevant.
    started: Instant,
}

impl Drop for Span {
    /// Completes a stage even when malformed font data causes an early return.
    fn drop(&mut self) {
        // Dropping on every return path counts recoverable candidate failures as well.
        let slot = self.stage as usize;
        // Saturating conversion preserves the u64 counter for unusually long diagnostics.
        let ns = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        // Poisoned diagnostic storage cannot interrupt font fallback or draw publication.
        if let Ok(mut calls) = self.owner.calls.lock() {
            calls.push([slot as u64, ns]);
        }
        // Stage totals overlap: index, mapping and extraction are not additive frame phases.
        self.owner.nanos[slot].fetch_add(ns, Ordering::Relaxed);
        self.owner.counts[slot].fetch_add(1, Ordering::Relaxed);
    }
}
