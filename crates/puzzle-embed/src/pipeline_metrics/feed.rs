//! Opt-in, bounded PTY/parser/snapshot stage observations for M16.5.
//! Counters measure nested wall-clock stages, not exclusive CPU. They never
//! influence scheduling and serialize only after the workload. Production
//! release builds without lock-metrics contain none of these timers or atoms.
//! Worst-feed metadata retains timing, byte count and a reset-pair flag only;
//! it distinguishes final teardown from warm parsing without recording content.
//! A raw reset flag is evidence of complete bytes, not a second VT parser.
//! Growth arrays name row allocations, requested cell bytes and deque relocations.
//! They neither count GPU memory nor retain application output.
//! Each owner feed preserves its actual semantic operation counts.
//! Pool receive/return intervals are separate channel observations.
//! The work journal retains paired wall/CPU clocks for the same feed.
//! Session phase durations are nested in `feed`, including deferred host capability costs.
//! Observation overhead never changes the fixed 16-KiB slice policy.
//! Compile-time feed/pool observers, separate from runtime-enabled input clocks.
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

/// The six live observation sites exclude the withdrawn shared-lock stages.
const STAGE_COUNT: usize = 6;

use super::hold;

/// Disjoint observation sites; nested read/feed work must not be summed.
#[derive(Clone, Copy)]
pub enum Stage {
    /// Blocking kernel read, excluding waiting for an empty pool buffer.
    Read,
    /// No empty pool buffer was immediately available to the drain.
    Exhausted,
    /// Completed read to parser receipt, including queued buffer residence.
    Queue,
    /// Session::feed including parsing, replies and reconciliation.
    Feed,
    /// Returning an exhausted fixed buffer to the drain channel.
    PoolReturn,
    /// Nonblocking filled-buffer receipt, excluding kernel read and idle wait.
    PoolReceive,
}

/// One fixed-size duration distribution retains exact totals and maxima.
#[derive(Default)]
struct Samples {
    /// Number of observations, including zero-duration operations.
    count: AtomicU64,
    /// Wall nanoseconds summed without truncation at per-read granularity.
    nanos: AtomicU64,
    /// A rare scheduling stall remains visible rather than disappearing in a mean.
    max: AtomicU64,
}

/// Shared by one pool and its Session; no per-byte or per-sample allocation.
pub struct Feed {
    /// The longest owner feed records semantic work and paired wall/CPU clocks.
    pub turn: hold::Turn,
    /// Only actual operations inside the largest VT call, without retained bytes.
    max_feed_operations: [AtomicU64; 11],
    /// Coarse Session phases distinguish parsing from a host wake or service callback.
    max_feed_phases: [AtomicU64; 6],
    /// Each enum value selects its own counter without string lookup.
    stages: [Samples; STAGE_COUNT],
    /// Log2 byte-size bins retain the macOS read-size distribution.
    reads: [AtomicU64; 17],
    /// One local monotonic origin connects first read to final parser feed.
    origin: Instant,
    /// Nanoseconds of first successful read, zero until any data is received.
    first_read: AtomicU64,
    /// Final completed feed timestamp, independent of the observer's poll interval.
    last_feed: AtomicU64,
    /// Actual successful read bytes, independent of requested vector capacity.
    bytes: AtomicU64,
    /// Largest feed's completion time identifies warm output versus final reset work.
    max_feed_at: AtomicU64,
    /// Offered slice size at that maximum, never retained application bytes.
    max_feed_bytes: AtomicU64,
    /// A complete ESC c byte pair was in that slice; split pairs remain unclassified.
    max_feed_ris: AtomicU64,
    /// Row allocations, requested cell bytes and deque relocations inside the worst feed.
    max_feed_growth: [AtomicU64; 3],
    /// Total owner-side growth events remain visible even when not the worst feed.
    owner_growth: [AtomicU64; 3],
}

/// Constructs fixed counters before the drain and parser share the owner.
impl Default for Feed {
    /// Allocates only the owning Arc, before the child begins output.
    fn default() -> Self {
        Self {
            // Array lengths follow the closed stage enum and maximum 64-KiB read.
            turn: hold::Turn::default(),
            max_feed_operations: std::array::from_fn(|_| AtomicU64::new(0)),
            max_feed_phases: std::array::from_fn(|_| AtomicU64::new(0)),
            stages: std::array::from_fn(|_| Samples::default()),
            reads: std::array::from_fn(|_| AtomicU64::new(0)),
            bytes: AtomicU64::new(0),
            origin: Instant::now(),
            first_read: AtomicU64::new(0),
            last_feed: AtomicU64::new(0),
            max_feed_at: AtomicU64::new(0),
            max_feed_bytes: AtomicU64::new(0),
            max_feed_ris: AtomicU64::new(0),
            max_feed_growth: std::array::from_fn(|_| AtomicU64::new(0)),
            owner_growth: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

/// Observation records owner work without borrowing Session or changing scheduling.
impl Feed {
    /// Keeps the worst feed's phase metadata without owning its source bytes.
    /// One parse worker writes this site; the final report reads after the cohort.
    pub fn feed(
        &self,
        elapsed: Duration,
        bytes: &[u8],
        before: [u64; 3],
        after: [u64; 3],
        operations: [u64; 11],
        phases: [u64; 6],
    ) {
        // A new maximum is rare. Its control-pair scan stays off ordinary warm feeds.
        let largest = elapsed.as_nanos()
            > u128::from(
                self.stages[Stage::Feed as usize]
                    .max
                    .load(Ordering::Relaxed),
            );
        self.record(Stage::Feed, elapsed);
        for index in 0..3 {
            // Counters are observations only. Subtraction handles a reset/reflow
            // reconstructing an owner without inventing negative growth events.
            let delta = after[index].saturating_sub(before[index]);
            self.owner_growth[index].fetch_add(delta, Ordering::Relaxed);
            if largest {
                self.max_feed_growth[index].store(delta, Ordering::Relaxed);
            }
        }
        if largest {
            // Only this owner writes feed metadata. Reports read after the run,
            // so the associated phase and operation arrays describe one feed.
            for (slot, value) in self.max_feed_phases.iter().zip(phases) {
                slot.store(value, Ordering::Relaxed);
            }
            for (slot, value) in self.max_feed_operations.iter().zip(operations) {
                slot.store(value, Ordering::Relaxed);
            }
            // Capture only scalar evidence, not user text or a per-call allocation.
            self.max_feed_at
                .store(self.origin.elapsed().as_nanos() as u64, Ordering::Relaxed);
            self.max_feed_bytes
                .store(bytes.len() as u64, Ordering::Relaxed);
            // A complete RIS can reset row storage; absence cannot rule out a split RIS.
            self.max_feed_ris.store(
                u64::from(bytes.windows(2).any(|pair| pair == b"\x1bc")),
                Ordering::Relaxed,
            );
        }
    }

    /// Retains an exact stage total and maximum without taking another lock.
    pub fn record(&self, stage: Stage, elapsed: Duration) {
        // Enum ordering is stable inside this private module, not an external format.
        let sample = &self.stages[stage as usize];
        // Saturate conversion before the fixed-width diagnostic counters.
        let nanos = elapsed.as_nanos().min(u128::from(u64::MAX)) as u64;
        // These atoms are observations only; relaxed order cannot affect data flow.
        sample.count.fetch_add(1, Ordering::Relaxed);
        sample.nanos.fetch_add(nanos, Ordering::Relaxed);
        sample.max.fetch_max(nanos, Ordering::Relaxed);
        // The last successful feed closes the actual drain-to-parse interval.
        if matches!(stage, Stage::Feed) {
            self.last_feed
                .store(self.origin.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }
    }

    /// Counts a successful read by actual bytes, not the 64-KiB request size.
    pub fn read_size(&self, bytes: usize) {
        // Only the first read wins; later arrivals retain its original timestamp.
        let _ = self.first_read.compare_exchange(
            0,
            self.origin.elapsed().as_nanos() as u64,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        // Logarithmic bins distinguish 1-KiB kernel reads from larger platforms.
        let bin = bytes.max(1).ilog2().min(16) as usize;
        self.reads[bin].fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// Formats every counter after measurement; no parser or GPU is required.
    // Debug counters remain available without allocating a formatted report.
    #[cfg(feature = "lock-metrics")]
    pub(super) fn report(&self, controls: serde_json::Value) -> serde_json::Value {
        // Stable report names are paired with every stage, including unused stages.
        let names = [
            "read",
            "pool_exhaustion",
            "queue_residence",
            "feed",
            "pool_return",
            "pool_receive",
        ];
        // Copy scalar counters before allocating the report object off the hot path.
        let stages: serde_json::Map<_, _> = names
            .iter()
            .zip(&self.stages)
            .map(|(name, sample)| {
                (
                    *name,
                    sample.count.load(Ordering::Relaxed),
                    sample.nanos.load(Ordering::Relaxed),
                    sample.max.load(Ordering::Relaxed),
                )
            })
            .map(|(name, count, nanos, max)| {
                (
                    name.to_owned(),
                    serde_json::json!({
                        "count":count, "total_ms":nanos as f64 / 1e6, "max_ms":max as f64 / 1e6
                    }),
                )
            })
            .collect();
        // Missing output stays null rather than claiming zero elapsed time.
        let first = self.first_read.load(Ordering::Relaxed);
        let last = self.last_feed.load(Ordering::Relaxed);
        let elapsed = (first > 0 && last >= first).then_some((last - first) as f64 / 1e6);
        // Position the worst feed in the same read-to-feed timeline as throughput.
        let maximum = self.max_feed_at.load(Ordering::Relaxed);
        let maximum_ms = (first > 0 && maximum >= first).then_some((maximum - first) as f64 / 1e6);
        // Totals are nested wall durations; consumers must not sum these as CPU.
        serde_json::json!({ "read_to_final_feed_ms":elapsed, "stages":stages, "read_bytes":self.bytes.load(Ordering::Relaxed),
            "max_feed_at_ms":maximum_ms, "max_feed_bytes":self.max_feed_bytes.load(Ordering::Relaxed),
            "max_feed_ring_growth": self.max_feed_growth.iter().map(|value| value.load(Ordering::Relaxed)).collect::<Vec<_>>(),
            "owner_ring_growth": self.owner_growth.iter().map(|value| value.load(Ordering::Relaxed)).collect::<Vec<_>>(),
            "max_owner_feed": self.turn.report(),
            "controls_and_keys": controls,
            "max_feed_operations": hold::operations(&self.max_feed_operations),
            "max_feed_copy_parse_reply_wake_reconcile_pump_ms": self.max_feed_phases.iter().map(|v| v.load(Ordering::Relaxed) as f64/1e6).collect::<Vec<_>>(),
            "max_feed_contains_ris":self.max_feed_ris.load(Ordering::Relaxed) != 0,
            "read_log2_bins":self.reads.iter().map(|bin|bin.load(Ordering::Relaxed)).collect::<Vec<_>>() })
    }
}
