//! Bounded recent cost evidence for indivisible glyph work (D-33).
//!
//! Every owner keeps a separate history for the work kind it admits. Cold
//! parsing, image decoding, allocation and cheap vector preparation must not
//! lend their costs to each other. The history follows the pacer's 32-sample
//! horizon; it bounds observation storage, not glyph throughput or timeouts.
//! A recent p95 retires transient stalls and ignores one isolated outlier once
//! enough observations exist. No estimate is the single last or lifetime max.
//! Before any evidence exists the caller must establish cost off-frame.
use std::time::Duration;

/// Fixed storage retains chronological evidence without per-call allocation.
#[derive(Clone, Default)]
pub struct RecentCost {
    /// Only the populated prefix participates before the ring fills.
    samples: [Duration; 32],
    /// Next replacement retires the oldest observation after the first cycle.
    next: usize,
    /// Saturating population separates unobserved zeros from measured calls.
    len: usize,
}
impl RecentCost {
    /// Records a completed call after its real work, regardless of clock granularity.
    pub fn record(&mut self, cost: Duration) {
        // Ring order matters only for retirement; percentile sorting uses a copy.
        self.samples[self.next] = cost;
        // The fixed horizon shares D-33's observation bound, never a batch size.
        self.next = (self.next + 1) % self.samples.len();
        // Empty slots cannot dilute the percentile on startup.
        self.len = (self.len + 1).min(self.samples.len());
    }
    /// Returns recent nearest-rank p95; None keeps unmeasured work on the worker.
    #[must_use]
    pub fn p95(&self) -> Option<Duration> {
        // A face being open alone supplies no evidence about its extraction cost.
        if self.len == 0 {
            return None;
        }
        // A stack copy avoids allocation and preserves chronological ring retirement.
        let mut sorted = self.samples;
        // Only actual observations enter admission; the rest are unused capacity.
        sorted[..self.len].sort_unstable();
        // Nearest-rank p95 is robust to one isolated stall in a full recent window.
        let rank = (self.len * 95).div_ceil(100).saturating_sub(1);
        Some(sorted[rank])
    }
}

#[cfg(test)]
/// Expensive independent kinds and retired noise cannot strand cheaper work.
mod tests {
    use super::*;
    #[test]
    fn independent_recent_percentiles_recover_from_outliers() {
        let mut vector = RecentCost::default();
        let mut allocation = RecentCost::default();
        assert_eq!(vector.p95(), None);
        allocation.record(Duration::from_millis(20));
        for _ in 0..31 {
            vector.record(Duration::from_micros(5));
        }
        vector.record(Duration::from_millis(9));
        // A large allocation and one unusual vector cannot set normal vector cost.
        assert_eq!(vector.p95(), Some(Duration::from_micros(5)));
        assert_eq!(allocation.p95(), Some(Duration::from_millis(20)));
        // Later evidence replaces a temporary expensive estimate without a reset.
        for _ in 0..32 {
            allocation.record(Duration::from_micros(25));
        }
        assert_eq!(allocation.p95(), Some(Duration::from_micros(25)));
    }
}
