//! Measured publication and post-receipt work reserve the bounded snapshot wait.
use super::*;

/// Publication may wait only inside this refresh's measured remaining opportunity.
/// The caller computes this before `take`, while the reserved target is still present.
/// The target and D-33 lead have different roles: the lead starts the owner request,
/// and the post-receipt p90 reserves enough time to build and submit the returned slot.
/// Publication queue time stays in the complete-frame lead samples, avoiding a
/// second independent prediction that ignores a slow or preempted Session owner.
/// An uncalibrated FIFO still bounds its receive window by one supplied display
/// period; that fallback is a waiting bound, never a claimed display-phase sample.
/// Cold glyph work has no history yet, so it remains complete even if the estimate
/// is too small. That overrun is recorded rather than aborting an unfinished frame.
impl Pacer {
    /// Reserves measured rendering work; the wake margin was spent reaching this turn.
    pub fn build_deadline(&self, now: Instant) -> Instant {
        let target = self
            .refresh
            .filter(|_| self.phase.is_some())
            .map_or(now + self.period, |(_, at)| at);
        let work = self.build_cost();
        // D-33's one-ms wake margin already moves the request deadline earlier in
        // lead(). Reserving it again after waking would consume the very slack
        // provided for OS wake jitter and make ordinary 30-µs replies look late.
        // A missed opportunity never blocks: already queued answers remain eligible.
        target.checked_sub(work).unwrap_or(now).max(now)
    }
    /// Post-receipt work is measured independently of owner waiting and FIFO.
    pub fn build_cost(&self) -> Duration {
        let mut costs = [Duration::ZERO; SAMPLE_COUNT];
        let count = self.build_costs.len();
        for (slot, value) in costs.iter_mut().zip(&self.build_costs) {
            *slot = *value;
        }
        costs[..count].sort_unstable();
        costs[..count]
            .get((count * 9).div_ceil(10).saturating_sub(1))
            .copied()
            .unwrap_or_default()
    }
    /// Diagnostic arithmetic is copied as values without changing the prediction.
    #[cfg(feature = "lock-metrics")]
    pub fn clock_costs(&self) -> [Duration; 2] {
        [self.lead(), self.build_cost()]
    }
    /// Identifies actual rendering work for the next build reserve.
    pub fn snapshot_received(&mut self, at: Instant, wait_and_latency: [Duration; 2]) {
        self.snapshot_received = Some((at, wait_and_latency));
    }
}
