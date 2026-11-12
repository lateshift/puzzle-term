//! Paired owner-feed clocks retain actual VT work instead of inferred byte patterns.
//! One Session owner records the five worst wall/CPU intervals without retaining
//! application bytes or allocating per scalar. CPU time excludes preemption;
//! wall time includes it. Neither clock changes the existing parse slice policy.
//! Phase values are nested inside the measured feed and must not be summed as CPU.
//! Reports are read after a finite cohort; atomic fields avoid a diagnostic mutex
//! but do not promise an interleaved coherent report while the owner is writing.
//!
//! A scheduler pause can make the longest wall sample differ from the longest
//! CPU sample. Both pairs are retained, so a report cannot mistake preemption
//! for costly VT work. Operation counts describe the wall maximum's exact feed.
//! They identify full-row scrolls, selective erases, long clusters and resets.
//! The stage array keeps feed and pool observations separate; owner-feed records
//! put zero in the two pool fields because those handoffs now happen outside it.
//! The paired clock freezes before any atomic journal insertion or formatting.
/// The fixed tail reports several outliers rather than one selected example.
pub(super) mod tail;
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// The sole parse writer publishes a bounded description of its longest feed.
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
#[derive(Default)]
pub struct Turn {
    /// Every measured interval competes for the five worst paired wall/CPU slots.
    tail: tail::Tail,
    /// Exact duration encloses one feed; pool handoffs occur outside this interval.
    nanos: AtomicU64,
    /// Paired CPU time of this same worst wall-time feed, absent on unsupported targets.
    cpu: AtomicU64,
    /// Largest actual CPU duration and its paired wall feed remain independent maxima.
    max_cpu: AtomicU64,
    /// Wall time for the same feed that used the most CPU.
    wall_at_max_cpu: AtomicU64,
    /// Offered bytes in this feed, each at most 16 KiB.
    bytes: AtomicU64,
    /// Distinguishes a single expensive call from accumulated cheap calls.
    slices: AtomicU64,
    /// Feed duration plus zero pool fields retain explicit interval ownership.
    stages: [AtomicU64; 3],
    /// Session copy/VT/replies/wake/reconcile/pump totals for this particular feed.
    phases: [AtomicU64; 6],
    /// Fixed counts distinguish recycling/erase/reflow/cluster/control work.
    operations: [AtomicU64; 11],
}

/// Updating this observer cannot extend scheduling policy or shrink a parser slice.
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
impl Turn {
    /// Copies metadata only when this feed exceeds every preceding feed.
    /// Offered bytes include a Session-retained suffix; they are not a byte receipt.
    /// `stages` separates feed from pool work; this owner records only the feed field.
    pub fn record(
        &self,
        times: (Duration, Option<Duration>),
        bytes: usize,
        slices: usize,
        operations: [u64; 11],
        stages: [u64; 3],
        phases: [u64; 6],
    ) {
        self.tail.record(times);
        let nanos = times.0.as_nanos().min(u128::from(u64::MAX)) as u64;
        if let Some(cpu) = times.1 {
            let cpu = cpu.as_nanos().min(u128::from(u64::MAX - 1)) as u64;
            // Both maxima have one parser writer; each pair describes one actual feed.
            if cpu > self.max_cpu.load(Ordering::Relaxed) {
                self.max_cpu.store(cpu, Ordering::Relaxed);
                self.wall_at_max_cpu.store(nanos, Ordering::Relaxed);
            }
        }
        if nanos <= self.nanos.load(Ordering::Relaxed) {
            return;
        }
        // One parse writer owns maxima. These scalars are read after the finite cohort.
        self.nanos.store(nanos, Ordering::Relaxed);
        // MAX is an absence sentinel; a genuine zero CPU interval remains a value.
        self.cpu.store(
            times.1.map_or(u64::MAX, |v| {
                v.as_nanos().min(u128::from(u64::MAX - 1)) as u64
            }),
            Ordering::Relaxed,
        );
        // Scalar metadata identifies this wall maximum without retaining the input.
        self.bytes.store(bytes as u64, Ordering::Relaxed);
        self.slices.store(slices as u64, Ordering::Relaxed);
        for (slot, value) in self.stages.iter().zip(stages) {
            slot.store(value, Ordering::Relaxed);
        }
        // Nested Session phases explain the same interval; they are not independent CPU totals.
        for (slot, value) in self.phases.iter().zip(phases) {
            slot.store(value, Ordering::Relaxed);
        }
        for (slot, value) in self.operations.iter().zip(operations) {
            slot.store(value, Ordering::Relaxed);
        }
    }
    /// Allocates named diagnostic fields off the hot parser path.
    /// Milliseconds retain sub-microsecond evidence; no removed lock budget is inferred.
    /// The six Session phases identify whether VT or a host capability dominated.
    /// Pool timings cover handoffs around those calls, without counting idle reads.
    #[cfg(feature = "lock-metrics")]
    pub(super) fn report(&self) -> serde_json::Value {
        let cpu = self.cpu.load(Ordering::Relaxed);
        serde_json::json!({"worst_five":self.tail.report(),"ms":self.nanos.load(Ordering::Relaxed) as f64/1e6,
            "thread_cpu_ms":(cpu != u64::MAX).then_some(cpu as f64 / 1e6),
            "largest_cpu_ms": self.max_cpu.load(Ordering::Relaxed) as f64 / 1e6,
            "wall_at_largest_cpu_ms": self.wall_at_max_cpu.load(Ordering::Relaxed) as f64 / 1e6,
            "bytes":self.bytes.load(Ordering::Relaxed), "slices":self.slices.load(Ordering::Relaxed),
            "feed_return_receive_ms": self.stages.iter().map(|v| v.load(Ordering::Relaxed) as f64/1e6).collect::<Vec<_>>(),
            "copy_parse_reply_wake_reconcile_pump_ms": self.phases.iter().map(|v| v.load(Ordering::Relaxed) as f64/1e6).collect::<Vec<_>>(),
            "operations":operations(&self.operations)})
    }
}

/// Stable names distinguish selected erase spans from actual overwritten protected cells.
#[cfg(feature = "lock-metrics")]
pub(super) fn operations(values: &[AtomicU64; 11]) -> serde_json::Value {
    let names = [
        "scroll_up_rows",
        "scroll_down_rows",
        "erase_spans",
        "selected_erase_cells",
        "cluster_joins",
        "max_cluster_bytes",
        "osc_handlers",
        "osc_parameter_bytes",
        "reflow_rows",
        "hard_reset_rows",
        "recycled_rows",
    ];
    // Formatting occurs after the cohort; counts were plain words inside VT.
    let map: serde_json::Map<_, _> = names
        .iter()
        .zip(values)
        .map(|(name, value)| {
            (
                (*name).to_owned(),
                serde_json::json!(value.load(Ordering::Relaxed)),
            )
        })
        .collect();
    serde_json::Value::Object(map)
}
